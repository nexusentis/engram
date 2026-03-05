use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::api_config::{ApiExtractorConfig, ApiProvider};
use super::context::ConversationEntityRegistry;
use super::entity_registry::EntityRegistry;
use super::extractor::Extractor;
use super::types::{Conversation, ExtractedEntity, ExtractedFact, Role};
use crate::error::{ExtractionError, Result};
use crate::llm::{AuthStyle, HttpLlmClient, LlmClientConfig};
use crate::types::{EpistemicType, FactType, SourceType};

const EXTRACTION_PROMPT: &str = r#"Extract 5-15 comprehensive facts from this conversation. Extract EVERY distinct piece of information.

For each fact, provide:
1. what: The core information (complete narrative, not fragments)
2. when: Date/time reference if mentioned (absolute or relative), or null
3. where: Location if mentioned, or null
4. who: People/entities involved
5. why: Reason or context if mentioned, or null
6. confidence: 0.0-1.0
7. fact_type: state, event, preference, or relation
8. epistemic_type: world (objective), experience (first-person), opinion (subjective), observation (neutral summary)
9. observation_level: "explicit" (directly stated), "deductive" (logically inferred from context), or "contradiction" (conflicts with previously known information)

CRITICAL RULES:
- Extract EVERY distinct piece of information, even if it seems minor
- Preserve EXACT names, numbers, dates, and quantities
- Do NOT summarize multiple facts into one — split them
- Include implicit context (locations, organizations) from conversation flow

Respond ONLY with valid JSON:
{"facts": [{"what": "...", "when": "...", "where": "...", "who": ["..."], "why": "...", "confidence": 0.9, "fact_type": "state", "epistemic_type": "world", "observation_level": "explicit"}]}"#;

/// Prompt for entity-aware extraction (Pass 2)
/// This prompt includes context about entities from the conversation
const ENTITY_AWARE_EXTRACTION_PROMPT: &str = r#"Extract 5-15 comprehensive facts from this conversation.

CRITICAL: Resolve ALL implicit entity references using the provided conversation context.
When extracting facts, you MUST include relevant context entities even if they are not explicitly mentioned in that specific message.

EXAMPLES OF IMPLICIT REFERENCE RESOLUTION:

Example 1 - Location Resolution:
Conversation context: User mentioned "Target" as their regular store
User says: "I redeemed a coupon on coffee creamer"
WRONG extraction: "User redeemed a coupon on coffee creamer"
CORRECT extraction: "User redeemed a coupon on coffee creamer at Target"

Example 2 - Product/App Resolution:
Conversation context: User discussed "Cartwheel app" owned by Target
User says: "The app saved me $5"
WRONG extraction: "An app saved user $5"
CORRECT extraction: "The Cartwheel app saved user $5"

Example 3 - Pronoun Resolution:
Conversation context: Alice mentioned as user's sister
User says: "She recommended a great restaurant"
WRONG extraction: "Someone recommended a restaurant"
CORRECT extraction: "Alice (user's sister) recommended a restaurant"

Example 4 - Organization Resolution:
Conversation context: User works at Google
User says: "The cafeteria has great food"
WRONG extraction: "A cafeteria has great food"
CORRECT extraction: "Google's cafeteria has great food"

For each fact, provide:
1. what: The core information (complete narrative including implicit context)
2. when: Date/time reference if mentioned, or null
3. where: Location if mentioned, or null
4. who: ALL relevant entities (both explicit AND contextual)
5. why: Reason or context if mentioned, or null
6. confidence: 0.0-1.0
7. fact_type: state, event, preference, or relation
8. epistemic_type: world (objective), experience (first-person), opinion (subjective), observation (neutral summary)
9. observation_level: "explicit" (directly stated), "deductive" (logically inferred from context), or "contradiction" (conflicts with previously known information)

CRITICAL RULES:
- Extract EVERY distinct piece of information, even if it seems minor
- Preserve EXACT names, numbers, dates, and quantities
- Do NOT summarize multiple facts into one — split them

Respond ONLY with valid JSON:
{"facts": [{"what": "...", "when": "...", "where": "...", "who": ["..."], "why": "...", "confidence": 0.9, "fact_type": "state", "epistemic_type": "world", "observation_level": "explicit"}]}"#;

/// API-based extractor using Claude or GPT models
pub struct ApiExtractor {
    config: ApiExtractorConfig,
}

impl ApiExtractor {
    pub fn new(config: ApiExtractorConfig) -> Self {
        Self { config }
    }

    /// Format conversation as API messages
    fn format_messages(&self, conversation: &Conversation) -> Vec<serde_json::Value> {
        let mut messages = vec![json!({
            "role": "system",
            "content": EXTRACTION_PROMPT
        })];

        let user_content: String = conversation
            .turns
            .iter()
            .map(|t| {
                format!(
                    "{}: {}",
                    match t.role {
                        Role::User => "User",
                        Role::Assistant => "Assistant",
                        Role::System => "System",
                    },
                    t.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        messages.push(json!({
            "role": "user",
            "content": user_content
        }));

        messages
    }

    /// Build an HttpLlmClient from extraction config.
    fn build_llm_client(config: &ApiExtractorConfig) -> Result<HttpLlmClient> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| match config.provider {
                ApiProvider::Anthropic => std::env::var("ANTHROPIC_API_KEY").ok(),
                _ => std::env::var("OPENAI_API_KEY").ok(),
            })
            .ok_or_else(|| ExtractionError::Api("No API key configured".into()))?;

        let url = match config.provider {
            ApiProvider::Anthropic => config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string()),
            ApiProvider::OpenAI | ApiProvider::Custom => config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string()),
        };

        let llm_config = LlmClientConfig {
            max_retries: config.max_retries,
            request_timeout_secs: config.timeout_seconds,
            ..LlmClientConfig::default()
        };

        let mut client = HttpLlmClient::new(api_key)
            .map_err(|e| ExtractionError::Api(e.to_string()))?
            .with_base_url(url)
            .with_llm_config(llm_config)
            .map_err(|e| ExtractionError::Api(e.to_string()))?;

        if matches!(config.provider, ApiProvider::Anthropic) {
            client = client
                .with_auth_style(AuthStyle::Header("x-api-key".into()))
                .with_extra_header("anthropic-version", "2023-06-01");
        }

        Ok(client)
    }

    /// Call the API with retry logic (delegated to HttpLlmClient) and extraction cache.
    async fn call_api(&self, messages: Vec<serde_json::Value>) -> Result<String> {
        // Build request body based on provider
        let body = match self.config.provider {
            ApiProvider::Anthropic => {
                json!({
                    "model": self.config.model,
                    "max_tokens": 1024,
                    "messages": &messages[1..], // Skip system for Anthropic
                    "system": messages[0]["content"],
                })
            }
            ApiProvider::OpenAI | ApiProvider::Custom => {
                let supports_temp = self.config.supports_temperature.unwrap_or_else(|| {
                    !self.config.model.starts_with("gpt-5")
                        && !self.config.model.starts_with("o")
                });
                let mut b = if supports_temp {
                    let temp = self.config.temperature.unwrap_or(0.1);
                    json!({
                        "model": self.config.model,
                        "messages": messages,
                        "temperature": temp,
                    })
                } else {
                    json!({
                        "model": self.config.model,
                        "messages": messages,
                    })
                };
                if supports_temp {
                    if let Some(seed) = self.config.seed {
                        b["seed"] = json!(seed);
                    }
                }
                b
            }
        };

        // Extraction cache: check for cached response
        let url_for_cache = match self.config.provider {
            ApiProvider::Anthropic => self.config.base_url.as_deref()
                .unwrap_or("https://api.anthropic.com/v1/messages"),
            _ => self.config.base_url.as_deref()
                .unwrap_or("https://api.openai.com/v1/chat/completions"),
        };
        let cache_key = if self.config.cache_dir.is_some() {
            if let Ok(body_str) = serde_json::to_string(&body) {
                let mut hasher = Sha256::new();
                hasher.update(url_for_cache.as_bytes());
                hasher.update(b"\0");
                hasher.update(body_str.as_bytes());
                Some(hex::encode(hasher.finalize()))
            } else {
                None
            }
        } else {
            None
        };

        if let (Some(cache_dir), Some(key)) = (&self.config.cache_dir, &cache_key) {
            let cache_path = cache_dir.join(format!("{}.json", key));
            if cache_path.exists() {
                if let Ok(cached) = tokio::fs::read_to_string(&cache_path).await {
                    if !cached.is_empty() {
                        tracing::debug!("Extraction cache HIT: {}", key);
                        return Ok(cached);
                    }
                }
            }
        }

        // Send request via HttpLlmClient (handles retry/backoff/401)
        let llm_client = Self::build_llm_client(&self.config)?;
        let json = llm_client
            .send_request(&body)
            .await
            .map_err(|e| ExtractionError::Api(e.to_string()))?;

        // Extract content based on provider
        let content = match self.config.provider {
            ApiProvider::Anthropic => json["content"][0]["text"].as_str(),
            _ => json["choices"][0]["message"]["content"].as_str(),
        };

        let result = content
            .map(String::from)
            .ok_or_else(|| ExtractionError::Api("Empty response".into()))?;

        // Save to extraction cache (atomic write: unique temp file + rename)
        if let (Some(cache_dir), Some(key)) = (&self.config.cache_dir, &cache_key) {
            let cache_path = cache_dir.join(format!("{}.json", key));
            if !cache_path.exists() {
                let tmp_path = cache_dir.join(format!(".tmp_{}", uuid::Uuid::now_v7()));
                if let Err(e) = tokio::fs::write(&tmp_path, &result).await {
                    tracing::warn!("Failed to write extraction cache: {}", e);
                } else if let Err(e) = tokio::fs::rename(&tmp_path, &cache_path).await {
                    tracing::warn!("Failed to rename extraction cache: {}", e);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                }
            }
        }

        Ok(result)
    }

    /// Format a 5-dimension fact into a single content string
    fn format_5dim_fact(fact: &serde_json::Value) -> String {
        let what = fact["what"].as_str().unwrap_or("");
        let when = fact["when"].as_str().unwrap_or("");
        let where_ = fact["where"].as_str().unwrap_or("");
        let who = fact["who"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let why = fact["why"].as_str().unwrap_or("");

        let mut content = what.to_string();
        if !when.is_empty() && when != "null" {
            content.push_str(&format!(" | When: {}", when));
        }
        if !where_.is_empty() && where_ != "null" {
            content.push_str(&format!(" | Where: {}", where_));
        }
        if !who.is_empty() {
            content.push_str(&format!(" | Involving: {}", who));
        }
        if !why.is_empty() && why != "null" {
            content.push_str(&format!(" | {}", why));
        }
        content
    }

    /// Extract entities from the "who" array in 5-dimension facts
    fn extract_entities_from_who(fact: &serde_json::Value) -> Vec<ExtractedEntity> {
        // Try "who" array first (5-dim format), fall back to "entities"
        let entities_array = fact["who"]
            .as_array()
            .or_else(|| fact["entities"].as_array());

        entities_array
            .map(|e| {
                e.iter()
                    .filter_map(|v| {
                        let name = v.as_str()?.to_string();
                        Some(ExtractedEntity {
                            normalized_id: name.to_lowercase().replace(' ', "_"),
                            name,
                            entity_type: "unknown".to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parse API response to extracted facts
    /// Supports both 5-dimension format (what/when/where/who/why) and legacy (content) format
    fn parse_response(&self, response: &str) -> Vec<ExtractedFact> {
        // Find JSON in response (model might include extra text)
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());

        match serde_json::from_str::<serde_json::Value>(&response[json_start..json_end]) {
            Ok(json) => json["facts"]
                .as_array()
                .map(|facts| {
                    facts
                        .iter()
                        .filter_map(|f| {
                            // Support both 5-dim (what) and legacy (content) formats
                            let content = if f["what"].is_string() {
                                Self::format_5dim_fact(f)
                            } else {
                                f["content"].as_str()?.to_string()
                            };

                            if content.is_empty() {
                                return None;
                            }

                            Some(ExtractedFact {
                                content,
                                confidence: f["confidence"].as_f64().unwrap_or(0.8) as f32,
                                source_type: SourceType::UserExplicit,
                                fact_type: match f["fact_type"].as_str() {
                                    Some("event") => FactType::Event,
                                    Some("preference") => FactType::Preference,
                                    Some("relation") => FactType::Relation,
                                    _ => FactType::State,
                                },
                                epistemic_type: match f["epistemic_type"].as_str() {
                                    Some("experience") => EpistemicType::Experience,
                                    Some("opinion") => EpistemicType::Opinion,
                                    Some("observation") => EpistemicType::Observation,
                                    _ => EpistemicType::World,
                                },
                                entities: Self::extract_entities_from_who(f),
                                temporal_markers: vec![],
                                t_valid: None,
                                observation_level: f["observation_level"]
                                    .as_str()
                                    .unwrap_or("explicit")
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!("Failed to parse API response: {}", e);
                vec![]
            }
        }
    }

    /// Two-pass entity-aware extraction
    ///
    /// Pass 1: Build entity registry from conversation
    /// Pass 2: Extract facts with entity context for implicit reference resolution
    ///
    /// This solves the "Target coupon" problem where contextual entities are lost
    /// because they were mentioned earlier but not in the specific message.
    pub async fn extract_with_context(
        &self,
        conversation: &Conversation,
    ) -> Result<(Vec<ExtractedFact>, ConversationEntityRegistry)> {
        tracing::debug!(
            "ApiExtractor ({}) processing {} turns with context",
            self.config.model,
            conversation.turns.len()
        );

        // Pass 1: Build entity registry (graceful degradation on parse failure)
        let registry =
            match ConversationEntityRegistry::from_conversation(conversation, &self.config).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Entity extraction failed, falling back to empty registry: {}",
                        e
                    );
                    ConversationEntityRegistry::new()
                }
            };

        tracing::debug!(
            "Entity registry built: {} entities, primary_location={:?}",
            registry.entities.len(),
            registry.primary_location
        );

        // Pass 2: Extract with context
        let context_string = registry.to_context_string();
        let messages = self.format_messages_with_context(conversation, &context_string);
        let response = self.call_api(messages).await?;
        let facts = self.parse_response(&response);

        // Validate entity coverage
        if !facts.is_empty() {
            let coverage = self.verify_entity_coverage(&facts, &registry);
            tracing::debug!(
                "Entity coverage in extracted facts: {:.1}%",
                coverage * 100.0
            );
        }

        Ok((facts, registry))
    }

    /// Format messages with entity context for Pass 2
    fn format_messages_with_context(
        &self,
        conversation: &Conversation,
        entity_context: &str,
    ) -> Vec<serde_json::Value> {
        // Build the enhanced system prompt
        let system_prompt = format!(
            "{}\n\nCONVERSATION CONTEXT:\n{}",
            ENTITY_AWARE_EXTRACTION_PROMPT, entity_context
        );

        let mut messages = vec![json!({
            "role": "system",
            "content": system_prompt
        })];

        let user_content: String = conversation
            .turns
            .iter()
            .map(|t| {
                format!(
                    "{}: {}",
                    match t.role {
                        Role::User => "User",
                        Role::Assistant => "Assistant",
                        Role::System => "System",
                    },
                    t.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        messages.push(json!({
            "role": "user",
            "content": user_content
        }));

        messages
    }

    /// Verify that extracted facts include entities from the registry
    ///
    /// Returns the percentage of registry entities that appear in the extracted facts.
    fn verify_entity_coverage(
        &self,
        facts: &[ExtractedFact],
        registry: &ConversationEntityRegistry,
    ) -> f32 {
        if registry.entities.is_empty() {
            return 1.0;
        }

        let all_fact_entities: std::collections::HashSet<String> = facts
            .iter()
            .flat_map(|f| f.entities.iter())
            .map(|e| e.name.to_lowercase())
            .collect();

        let registry_entities: std::collections::HashSet<String> =
            registry.entities.keys().cloned().collect();

        let covered = registry_entities
            .iter()
            .filter(|e| all_fact_entities.contains(*e))
            .count();

        covered as f32 / registry_entities.len() as f32
    }

    /// Get the config (for testing)
    pub fn config(&self) -> &ApiExtractorConfig {
        &self.config
    }

    /// Two-pass entity-aware extraction with typed EntityRegistry
    ///
    /// This is the enhanced version that returns the typed EntityRegistry
    /// with proper EntityType enums and Relationship structs.
    ///
    /// Pass 1: Build typed entity registry from conversation
    /// Pass 2: Extract facts with entity context for implicit reference resolution
    pub async fn extract_with_typed_registry(
        &self,
        conversation: &Conversation,
    ) -> Result<(Vec<ExtractedFact>, EntityRegistry)> {
        tracing::debug!(
            "ApiExtractor ({}) processing {} turns with typed registry",
            self.config.model,
            conversation.turns.len()
        );

        // Pass 1: Build typed entity registry (graceful degradation on parse failure)
        let typed_registry = match ConversationEntityRegistry::build_typed_registry(
            conversation,
            &self.config,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Typed entity extraction failed, falling back to empty registry: {}",
                    e
                );
                EntityRegistry::new()
            }
        };

        tracing::debug!(
            "Typed entity registry built: {} entities, {} relationships, primary_location={:?}, primary_org={:?}",
            typed_registry.entity_count(),
            typed_registry.all_relationships().len(),
            typed_registry.primary_location,
            typed_registry.primary_organization
        );

        // Pass 2: Extract with context (using typed registry's prompt context)
        let context_string = typed_registry.to_prompt_context();
        let messages = self.format_messages_with_context(conversation, &context_string);
        let response = self.call_api(messages).await?;
        let facts = self.parse_response(&response);

        // Validate entity coverage using typed registry
        if !facts.is_empty() {
            let coverage = self.verify_typed_entity_coverage(&facts, &typed_registry);
            tracing::debug!(
                "Entity coverage in extracted facts: {:.1}%",
                coverage * 100.0
            );
        }

        Ok((facts, typed_registry))
    }

    /// Verify that extracted facts include entities from the typed registry
    fn verify_typed_entity_coverage(
        &self,
        facts: &[ExtractedFact],
        registry: &EntityRegistry,
    ) -> f32 {
        if registry.entity_count() == 0 {
            return 1.0;
        }

        let all_fact_entities: std::collections::HashSet<String> = facts
            .iter()
            .flat_map(|f| f.entities.iter())
            .map(|e| e.name.to_lowercase())
            .collect();

        let covered = registry
            .all_entities()
            .filter(|e| all_fact_entities.contains(&e.name.to_lowercase()))
            .count();

        covered as f32 / registry.entity_count() as f32
    }
}

#[async_trait]
impl Extractor for ApiExtractor {
    async fn extract(&self, conversation: &Conversation) -> Result<Vec<ExtractedFact>> {
        tracing::debug!(
            "ApiExtractor ({}) processing {} turns",
            self.config.model,
            conversation.turns.len()
        );

        let messages = self.format_messages(conversation);
        let response = self.call_api(messages).await?;
        Ok(self.parse_response(&response))
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn confidence_threshold(&self) -> f32 {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::types::ConversationTurn;

    #[test]
    fn test_format_messages() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let conv = Conversation::new(
            "user_1",
            vec![ConversationTurn {
                role: Role::User,
                content: "I work at Google".to_string(),
                timestamp: None,
            }],
        );

        let messages = extractor.format_messages(&conv);
        assert_eq!(messages.len(), 2); // system + user
        assert!(messages[0]["content"].as_str().unwrap().contains("Extract"));
        assert!(messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("I work at Google"));
    }

    #[test]
    fn test_format_messages_multiple_turns() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let conv = Conversation::new(
            "user_1",
            vec![
                ConversationTurn::user("Hello"),
                ConversationTurn::assistant("Hi there!"),
                ConversationTurn::user("I live in NYC"),
            ],
        );

        let messages = extractor.format_messages(&conv);
        let content = messages[1]["content"].as_str().unwrap();
        assert!(content.contains("User: Hello"));
        assert!(content.contains("Assistant: Hi there!"));
        assert!(content.contains("User: I live in NYC"));
    }

    #[test]
    fn test_parse_response_valid() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = r#"{"facts": [{"content": "User works at Google", "confidence": 0.9, "fact_type": "state", "epistemic_type": "world", "entities": ["Google"]}]}"#;

        let facts = extractor.parse_response(response);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User works at Google");
        assert!((facts[0].confidence - 0.9).abs() < 0.01);
        assert_eq!(facts[0].fact_type, FactType::State);
        assert_eq!(facts[0].epistemic_type, EpistemicType::World);
        assert_eq!(facts[0].entities.len(), 1);
        assert_eq!(facts[0].entities[0].name, "Google");
    }

    #[test]
    fn test_parse_response_with_prefix() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response =
            r#"Here is the extraction: {"facts": [{"content": "Test fact", "confidence": 0.85}]}"#;

        let facts = extractor.parse_response(response);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Test fact");
    }

    #[test]
    fn test_parse_response_invalid_json() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = "not valid json";

        let facts = extractor.parse_response(response);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_response_empty_facts() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = r#"{"facts": []}"#;

        let facts = extractor.parse_response(response);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_parse_response_all_fact_types() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = r#"{"facts": [
            {"content": "State fact", "fact_type": "state", "epistemic_type": "world"},
            {"content": "Event fact", "fact_type": "event", "epistemic_type": "experience"},
            {"content": "Preference fact", "fact_type": "preference", "epistemic_type": "opinion"},
            {"content": "Relation fact", "fact_type": "relation", "epistemic_type": "observation"}
        ]}"#;

        let facts = extractor.parse_response(response);
        assert_eq!(facts.len(), 4);
        assert_eq!(facts[0].fact_type, FactType::State);
        assert_eq!(facts[1].fact_type, FactType::Event);
        assert_eq!(facts[1].epistemic_type, EpistemicType::Experience);
        assert_eq!(facts[2].fact_type, FactType::Preference);
        assert_eq!(facts[2].epistemic_type, EpistemicType::Opinion);
        assert_eq!(facts[3].fact_type, FactType::Relation);
        assert_eq!(facts[3].epistemic_type, EpistemicType::Observation);
    }

    #[test]
    fn test_extractor_model_name() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        let extractor = ApiExtractor::new(config);
        assert_eq!(extractor.model_name(), "gpt-4o-mini");
    }

    #[test]
    fn test_extractor_confidence_threshold() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        assert_eq!(extractor.confidence_threshold(), 0.5);
    }

    #[tokio::test]
    #[ignore] // Requires ANTHROPIC_API_KEY
    async fn test_api_call_anthropic() {
        let config = ApiExtractorConfig {
            provider: ApiProvider::Anthropic,
            model: "claude-3-haiku-20240307".to_string(),
            ..Default::default()
        };
        let extractor = ApiExtractor::new(config);
        let conv = Conversation::new(
            "user_1",
            vec![ConversationTurn {
                role: Role::User,
                content: "I started a new job at Microsoft yesterday".to_string(),
                timestamp: None,
            }],
        );

        let facts = extractor.extract(&conv).await.unwrap();
        assert!(!facts.is_empty());
    }

    #[tokio::test]
    #[ignore] // Requires OPENAI_API_KEY
    async fn test_api_call_openai() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        let extractor = ApiExtractor::new(config);
        let conv = Conversation::new(
            "user_1",
            vec![ConversationTurn {
                role: Role::User,
                content: "I started a new job at Microsoft yesterday. I'm really excited about it!"
                    .to_string(),
                timestamp: None,
            }],
        );

        let facts = extractor.extract(&conv).await.unwrap();
        assert!(!facts.is_empty(), "Should extract at least one fact");

        // Verify facts have content
        for fact in &facts {
            assert!(!fact.content.is_empty(), "Fact content should not be empty");
            assert!(fact.confidence > 0.0, "Confidence should be positive");
        }

        println!("Extracted {} facts:", facts.len());
        for (i, fact) in facts.iter().enumerate() {
            println!(
                "  {}. {} (conf: {:.2}, type: {:?})",
                i + 1,
                fact.content,
                fact.confidence,
                fact.fact_type
            );
        }
    }

    #[test]
    fn test_verify_entity_coverage_empty_registry() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let registry = ConversationEntityRegistry::new();

        let facts = vec![ExtractedFact {
            content: "Test fact".to_string(),
            confidence: 0.9,
            source_type: SourceType::UserExplicit,
            fact_type: FactType::State,
            epistemic_type: EpistemicType::World,
            entities: vec![],
            temporal_markers: vec![],
            t_valid: None,
            observation_level: "explicit".to_string(),
        }];

        let coverage = extractor.verify_entity_coverage(&facts, &registry);
        assert_eq!(coverage, 1.0);
    }

    #[test]
    fn test_verify_entity_coverage_partial() {
        use crate::extraction::context::ContextualEntity;

        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);

        let mut registry = ConversationEntityRegistry::new();
        registry.entities.insert(
            "target".to_string(),
            ContextualEntity::new("Target", "store"),
        );
        registry.entities.insert(
            "walmart".to_string(),
            ContextualEntity::new("Walmart", "store"),
        );

        let facts = vec![ExtractedFact {
            content: "User shops at Target".to_string(),
            confidence: 0.9,
            source_type: SourceType::UserExplicit,
            fact_type: FactType::State,
            epistemic_type: EpistemicType::World,
            entities: vec![ExtractedEntity {
                name: "Target".to_string(),
                entity_type: "store".to_string(),
                normalized_id: "target".to_string(),
            }],
            temporal_markers: vec![],
            t_valid: None,
            observation_level: "explicit".to_string(),
        }];

        let coverage = extractor.verify_entity_coverage(&facts, &registry);
        assert!((coverage - 0.5).abs() < 0.01); // 1 of 2 entities covered
    }

    #[test]
    fn test_verify_typed_entity_coverage() {
        use crate::extraction::entity_registry::EntityRegistry;
        use crate::extraction::entity_types::{ConversationEntity, EntityType};

        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);

        let mut registry = EntityRegistry::new();
        registry.add_entity(ConversationEntity::new(
            "Target",
            EntityType::Organization,
            0,
        ));
        registry.add_entity(ConversationEntity::new("Alice", EntityType::Person, 1));

        let facts = vec![ExtractedFact {
            content: "Alice shops at Target".to_string(),
            confidence: 0.9,
            source_type: SourceType::UserExplicit,
            fact_type: FactType::State,
            epistemic_type: EpistemicType::World,
            entities: vec![
                ExtractedEntity {
                    name: "Target".to_string(),
                    entity_type: "organization".to_string(),
                    normalized_id: "target".to_string(),
                },
                ExtractedEntity {
                    name: "Alice".to_string(),
                    entity_type: "person".to_string(),
                    normalized_id: "alice".to_string(),
                },
            ],
            temporal_markers: vec![],
            t_valid: None,
            observation_level: "explicit".to_string(),
        }];

        let coverage = extractor.verify_typed_entity_coverage(&facts, &registry);
        assert_eq!(coverage, 1.0); // Both entities covered
    }

    #[test]
    fn test_5dim_fact_formatting() {
        let fact = json!({"what": "Bob started a new job at Google", "who": ["Bob"], "why": "career change"});
        let formatted = ApiExtractor::format_5dim_fact(&fact);
        assert!(formatted.contains("Bob started"));
        assert!(formatted.contains("Involving: Bob"));
        assert!(formatted.contains("career change"));
    }

    #[test]
    fn test_5dim_fact_formatting_full() {
        let fact = json!({
            "what": "Alice moved to NYC",
            "when": "March 2023",
            "where": "New York City",
            "who": ["Alice"],
            "why": "new job"
        });
        let formatted = ApiExtractor::format_5dim_fact(&fact);
        assert!(formatted.contains("Alice moved to NYC"));
        assert!(formatted.contains("When: March 2023"));
        assert!(formatted.contains("Where: New York City"));
        assert!(formatted.contains("Involving: Alice"));
        assert!(formatted.contains("new job"));
    }

    #[test]
    fn test_5dim_fact_formatting_nulls() {
        let fact = json!({"what": "Simple fact", "when": "null", "where": "null", "who": [], "why": "null"});
        let formatted = ApiExtractor::format_5dim_fact(&fact);
        assert_eq!(formatted, "Simple fact");
    }

    #[test]
    fn test_extraction_prompt_requests_5_15() {
        assert!(EXTRACTION_PROMPT.contains("5-15"));
        assert!(EXTRACTION_PROMPT.contains("EVERY distinct"));
    }

    #[test]
    fn test_parse_response_5dim_format() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = r#"{"facts": [{"what": "User works at Google", "who": ["Google"], "confidence": 0.9, "fact_type": "state", "epistemic_type": "world"}]}"#;

        let facts = extractor.parse_response(response);
        assert_eq!(facts.len(), 1);
        assert!(facts[0].content.contains("User works at Google"));
        assert!(facts[0].content.contains("Involving: Google"));
        assert_eq!(facts[0].entities.len(), 1);
        assert_eq!(facts[0].entities[0].name, "Google");
    }

    #[test]
    fn test_parse_response_legacy_format() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let response = r#"{"facts": [{"content": "User works at Google", "entities": ["Google"], "confidence": 0.9}]}"#;

        let facts = extractor.parse_response(response);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User works at Google");
        assert_eq!(facts[0].entities.len(), 1);
    }

    #[test]
    fn test_format_messages_with_context() {
        let config = ApiExtractorConfig::default();
        let extractor = ApiExtractor::new(config);
        let conv = Conversation::new(
            "user_1",
            vec![ConversationTurn::user(
                "I redeemed a coupon on coffee creamer",
            )],
        );

        let context =
            "PRIMARY LOCATION/STORE: Target\n\nENTITIES IN CONVERSATION:\n- Target [store]";
        let messages = extractor.format_messages_with_context(&conv, context);

        assert_eq!(messages.len(), 2);
        let system_content = messages[0]["content"].as_str().unwrap();
        assert!(system_content.contains("CRITICAL: Resolve ALL implicit entity references"));
        assert!(system_content.contains("PRIMARY LOCATION/STORE: Target"));
        assert!(system_content.contains("Example 1 - Location Resolution"));
    }
}
