//! Conversation-level entity tracking for context-aware extraction
//!
//! This module provides LLM-based entity extraction from conversations.
//!
//! # Architecture: Two Entity Registries
//!
//! The extraction pipeline uses two complementary registries:
//!
//! ## `ConversationEntityRegistry` (this module)
//! - **Purpose**: LLM-extracted entities from a single conversation
//! - **Entity type**: `ContextualEntity` (string-typed, flexible schema from LLM)
//! - **Relationships**: Raw strings from LLM output
//! - **Key method**: `async fn extract()` - calls LLM to identify entities
//! - **Conversion**: `to_typed_registry()` converts to `EntityRegistry`
//!
//! ## `EntityRegistry` (entity_registry.rs)
//! - **Purpose**: Type-safe storage for entities and relationships
//! - **Entity type**: `ConversationEntity` (enum-typed, validated)
//! - **Relationships**: `Relationship` structs with typed `RelationshipType`
//! - **Key methods**: `add_entity()`, `merge()`, `to_prompt_context()`
//! - **Usage**: Passed to LLM prompts, merged across sessions
//!
//! # Data Flow
//!
//! ```text
//! Conversation → LLM extract() → ConversationEntityRegistry
//!                                         ↓ to_typed_registry()
//!                                    EntityRegistry
//!                                         ↓ to_prompt_context()
//!                                    LLM prompt string
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::api_config::{ApiExtractorConfig, ApiProvider};
use super::entity_registry::EntityRegistry;
use super::entity_types::{ConversationEntity, EntityType};
use super::relationships::{Relationship, RelationshipType};
use super::types::{Conversation, Role};
use crate::error::{ExtractionError, Result};
use crate::llm::{AuthStyle, HttpLlmClient, LlmClientConfig};

/// An entity extracted from conversation context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualEntity {
    /// The canonical name of the entity
    pub name: String,
    /// Type of entity (e.g., "store", "person", "organization", "location")
    pub entity_type: String,
    /// Which turn (0-indexed) this entity first appeared
    pub first_turn: usize,
    /// Semantic roles this entity plays (e.g., "store where user shops", "employer")
    pub roles: Vec<String>,
    /// Alternative names/references for this entity
    pub aliases: Vec<String>,
}

impl ContextualEntity {
    /// Create a new contextual entity
    pub fn new(name: impl Into<String>, entity_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            entity_type: entity_type.into(),
            first_turn: 0,
            roles: Vec::new(),
            aliases: Vec::new(),
        }
    }

    /// Builder: set first turn
    pub fn with_first_turn(mut self, turn: usize) -> Self {
        self.first_turn = turn;
        self
    }

    /// Builder: add a role
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Builder: add an alias
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }
}

/// Relationship data extracted from conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    /// Subject entity name
    pub subject: String,
    /// Relationship type
    pub relation: String,
    /// Object entity name
    pub object: String,
}

/// Registry of entities from a conversation session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationEntityRegistry {
    /// All entities keyed by normalized name
    pub entities: HashMap<String, ContextualEntity>,
    /// Relationships between entities
    pub relationships: Vec<ExtractedRelationship>,
    /// Primary location context (e.g., "Target" when shopping at Target)
    pub primary_location: Option<String>,
    /// Primary organization context (e.g., employer being discussed)
    pub primary_organization: Option<String>,
    /// Primary person context (other than the user)
    pub primary_person: Option<String>,
}

/// Prompt for entity extraction pass
const ENTITY_REGISTRY_PROMPT: &str = r#"Analyze this conversation and extract ALL entities mentioned, with their semantic roles.

For each entity, identify:
1. name: The canonical name
2. entity_type: One of: person, organization, location, store, product, event, time_period
3. first_turn: Turn number where entity first appears (0-indexed)
4. roles: Semantic roles like "store where user shops", "user's employer", "product purchased"
5. aliases: Alternative names used (e.g., "mom" -> "mother", "MSFT" -> "Microsoft")

Also identify relationships between entities:
- subject: First entity name
- relation: One of: owned_by, located_in, works_for, part_of, related_to, associated_with
- object: Second entity name

Also identify:
- primary_location: The main physical location/store being discussed (if any)
- primary_organization: The main organization/company being discussed (if any)

IMPORTANT: Include entities that are mentioned ANYWHERE in the conversation, even if only once.
These entities provide context for understanding implicit references later.

Respond ONLY with valid JSON:
{
  "entities": [
    {"name": "Target", "entity_type": "store", "first_turn": 0, "roles": ["store where user shops"], "aliases": []},
    {"name": "Cartwheel", "entity_type": "product", "first_turn": 0, "roles": ["app used"], "aliases": ["the app"]}
  ],
  "relationships": [
    {"subject": "Cartwheel", "relation": "owned_by", "object": "Target"}
  ],
  "primary_location": "Target",
  "primary_organization": null
}"#;

impl ConversationEntityRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Build entity registry from a conversation using LLM
    pub async fn from_conversation(
        conversation: &Conversation,
        config: &ApiExtractorConfig,
    ) -> Result<Self> {
        // Build conversation text
        let conversation_text: String = conversation
            .turns
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "[Turn {}] {}: {}",
                    i,
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

        // Call LLM for entity extraction
        let response = Self::call_llm(config, &conversation_text).await?;

        // Parse response
        Self::parse_response(&response)
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

    /// Call the LLM API with extraction cache support.
    async fn call_llm(config: &ApiExtractorConfig, conversation_text: &str) -> Result<String> {
        // Build request body based on provider
        let body = match config.provider {
            ApiProvider::Anthropic => json!({
                "model": config.model,
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": conversation_text}],
                "system": ENTITY_REGISTRY_PROMPT,
            }),
            ApiProvider::OpenAI | ApiProvider::Custom => {
                let supports_temp = config.supports_temperature.unwrap_or_else(|| {
                    !config.model.starts_with("gpt-5")
                        && !config.model.starts_with("o")
                        && !config.model.contains("nano")
                });
                let mut b = json!({
                    "model": config.model,
                    "messages": [
                        {"role": "system", "content": ENTITY_REGISTRY_PROMPT},
                        {"role": "user", "content": conversation_text}
                    ],
                });
                if supports_temp {
                    b["temperature"] = json!(config.temperature.unwrap_or(0.1));
                }
                b
            }
        };

        // Extraction cache: check for cached response
        let cache_key = if let Some(ref cache_dir) = config.cache_dir {
            if let Ok(body_str) = serde_json::to_string(&body) {
                let url = match config.provider {
                    ApiProvider::Anthropic => config.base_url.as_deref()
                        .unwrap_or("https://api.anthropic.com/v1/messages"),
                    _ => config.base_url.as_deref()
                        .unwrap_or("https://api.openai.com/v1/chat/completions"),
                };
                let mut hasher = Sha256::new();
                hasher.update(url.as_bytes());
                hasher.update(b"\0");
                hasher.update(body_str.as_bytes());
                let key = hex::encode(hasher.finalize());
                let cache_path = cache_dir.join(format!("{}.json", key));
                if cache_path.exists() {
                    if let Ok(cached) = tokio::fs::read_to_string(&cache_path).await {
                        if !cached.is_empty() {
                            tracing::debug!("Entity extraction cache HIT: {}", key);
                            return Ok(cached);
                        }
                    }
                }
                Some(key)
            } else {
                None
            }
        } else {
            None
        };

        // Send request via HttpLlmClient (handles retry/backoff/401)
        let client = Self::build_llm_client(config)?;
        let json = client
            .send_request(&body)
            .await
            .map_err(|e| ExtractionError::Api(e.to_string()))?;

        // Extract content based on provider
        let content = match config.provider {
            ApiProvider::Anthropic => json["content"][0]["text"].as_str(),
            _ => json["choices"][0]["message"]["content"].as_str(),
        };

        let result = content
            .map(String::from)
            .ok_or_else(|| ExtractionError::Api("Empty response".into()))?;

        // Save to extraction cache
        if let (Some(ref cache_dir), Some(ref key)) = (&config.cache_dir, &cache_key) {
            let cache_path = cache_dir.join(format!("{}.json", key));
            if !cache_path.exists() {
                let tmp_path = cache_dir.join(format!(".tmp_{}", uuid::Uuid::now_v7()));
                if let Err(e) = tokio::fs::write(&tmp_path, &result).await {
                    tracing::warn!("Failed to write entity cache: {}", e);
                } else if let Err(e) = tokio::fs::rename(&tmp_path, &cache_path).await {
                    tracing::warn!("Failed to rename entity cache: {}", e);
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                }
            }
        }

        Ok(result)
    }

    /// Parse LLM response into registry
    fn parse_response(response: &str) -> Result<Self> {
        // Find JSON in response
        let json_start = response.find('{').unwrap_or(0);
        let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());

        let json: serde_json::Value = serde_json::from_str(&response[json_start..json_end])
            .map_err(|e| ExtractionError::Api(format!("Failed to parse entity registry: {}", e)))?;

        let mut registry = ConversationEntityRegistry::new();

        // Parse entities
        if let Some(entities) = json["entities"].as_array() {
            for (i, e) in entities.iter().enumerate() {
                let name = e["name"].as_str().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }

                let entity_type = e["entity_type"].as_str().unwrap_or("unknown").to_string();
                let first_turn = e["first_turn"].as_u64().unwrap_or(i as u64) as usize;
                let roles: Vec<String> = e["roles"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let aliases: Vec<String> = e["aliases"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let entity = ContextualEntity {
                    name: name.clone(),
                    entity_type,
                    first_turn,
                    roles,
                    aliases,
                };

                registry.entities.insert(name.to_lowercase(), entity);
            }
        }

        // Parse relationships
        if let Some(relationships) = json["relationships"].as_array() {
            for r in relationships {
                let subject = r["subject"].as_str().unwrap_or("").to_string();
                let relation = r["relation"]
                    .as_str()
                    .unwrap_or("associated_with")
                    .to_string();
                let object = r["object"].as_str().unwrap_or("").to_string();

                if !subject.is_empty() && !object.is_empty() {
                    registry.relationships.push(ExtractedRelationship {
                        subject,
                        relation,
                        object,
                    });
                }
            }
        }

        // Parse primary context
        registry.primary_location = json["primary_location"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "null")
            .map(String::from);
        registry.primary_organization = json["primary_organization"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "null")
            .map(String::from);

        Ok(registry)
    }

    /// Format registry as context string for extraction prompt
    pub fn to_context_string(&self) -> String {
        let mut parts = Vec::new();

        // Add primary context
        if let Some(ref loc) = self.primary_location {
            parts.push(format!("PRIMARY LOCATION/STORE: {}", loc));
        }
        if let Some(ref org) = self.primary_organization {
            parts.push(format!("PRIMARY ORGANIZATION: {}", org));
        }

        // Add entity list
        if !self.entities.is_empty() {
            parts.push("\nENTITIES IN CONVERSATION:".to_string());
            for entity in self.entities.values() {
                let roles_str = if entity.roles.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", entity.roles.join(", "))
                };
                parts.push(format!(
                    "- {} [{}]{}",
                    entity.name, entity.entity_type, roles_str
                ));
            }
        }

        parts.join("\n")
    }

    /// Check if an entity is present in the registry
    pub fn has_entity(&self, name: &str) -> bool {
        self.entities.contains_key(&name.to_lowercase())
    }

    /// Get an entity by name (case-insensitive)
    pub fn get_entity(&self, name: &str) -> Option<&ContextualEntity> {
        self.entities.get(&name.to_lowercase())
    }

    /// Get all entity names
    pub fn entity_names(&self) -> Vec<String> {
        self.entities.values().map(|e| e.name.clone()).collect()
    }

    /// Convert to typed EntityRegistry
    ///
    /// Converts the string-based entity representation to the typed
    /// EntityRegistry with EntityType enums and Relationship structs.
    pub fn to_typed_registry(&self) -> EntityRegistry {
        let mut registry = EntityRegistry::new();

        // Convert entities
        for entity in self.entities.values() {
            let entity_type = EntityType::from_str_loose(&entity.entity_type);
            let mut typed_entity =
                ConversationEntity::new(&entity.name, entity_type, entity.first_turn);

            // Add aliases
            for alias in &entity.aliases {
                typed_entity = typed_entity.with_alias(alias);
            }

            registry.add_entity(typed_entity);
        }

        // Convert relationships
        for rel in &self.relationships {
            if let Some(rel_type) = RelationshipType::from_str_loose(&rel.relation) {
                let relationship = Relationship::new(&rel.subject, rel_type, &rel.object);
                registry.add_relationship(relationship);
            }
        }

        // Set primary references
        registry.primary_location = self.primary_location.clone();
        registry.primary_organization = self.primary_organization.clone();

        registry
    }

    /// Count mentions of entities in conversation and update the typed registry
    pub fn count_mentions_in_conversation(
        registry: &mut EntityRegistry,
        conversation: &Conversation,
    ) {
        // Collect entity info to avoid borrow issues
        let entity_info: Vec<(String, Vec<String>)> = registry
            .all_entities()
            .map(|e| (e.name.clone(), e.aliases.clone()))
            .collect();

        for (name, aliases) in &entity_info {
            let name_lower = name.to_lowercase();
            let mut total_mentions: usize = 0;

            for turn in &conversation.turns {
                let content_lower = turn.content.to_lowercase();

                // Count direct mentions
                total_mentions += content_lower.matches(&name_lower).count();

                // Count alias mentions
                for alias in aliases {
                    total_mentions += content_lower.matches(&alias.to_lowercase()).count();
                }
            }

            // Update count (entity was created with mention_count=1, so add extra mentions)
            if total_mentions > 1 {
                if let Some(entity) = registry.get_mut(name) {
                    for _ in 0..total_mentions.saturating_sub(1) {
                        entity.increment_mention();
                    }
                }
            }
        }
    }

    /// Build a typed EntityRegistry from conversation using LLM
    ///
    /// This is the main entry point for Pass 1 entity extraction.
    /// It calls the LLM, parses the response, counts mentions,
    /// and returns a fully populated typed EntityRegistry.
    pub async fn build_typed_registry(
        conversation: &Conversation,
        config: &ApiExtractorConfig,
    ) -> Result<EntityRegistry> {
        // Get string-based registry from LLM
        let string_registry = Self::from_conversation(conversation, config).await?;

        // Convert to typed registry
        let mut typed_registry = string_registry.to_typed_registry();

        // Count actual mentions in conversation text
        Self::count_mentions_in_conversation(&mut typed_registry, conversation);

        // Compute primaries based on mention frequency
        typed_registry.compute_primaries();

        Ok(typed_registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contextual_entity_builder() {
        let entity = ContextualEntity::new("Target", "store")
            .with_first_turn(0)
            .with_role("store where user shops")
            .with_alias("Target Store");

        assert_eq!(entity.name, "Target");
        assert_eq!(entity.entity_type, "store");
        assert_eq!(entity.first_turn, 0);
        assert_eq!(entity.roles, vec!["store where user shops"]);
        assert_eq!(entity.aliases, vec!["Target Store"]);
    }

    #[test]
    fn test_registry_to_context_string() {
        let mut registry = ConversationEntityRegistry::new();
        registry.primary_location = Some("Target".to_string());
        registry.entities.insert(
            "target".to_string(),
            ContextualEntity::new("Target", "store").with_role("store where user shops"),
        );
        registry.entities.insert(
            "coffee creamer".to_string(),
            ContextualEntity::new("Coffee Creamer", "product").with_role("product purchased"),
        );

        let context = registry.to_context_string();
        assert!(context.contains("PRIMARY LOCATION/STORE: Target"));
        assert!(context.contains("ENTITIES IN CONVERSATION:"));
        assert!(context.contains("Target [store]"));
    }

    #[test]
    fn test_parse_response() {
        let response = r#"{"entities": [
            {"name": "Target", "entity_type": "store", "roles": ["shopping location"], "aliases": []},
            {"name": "Coffee", "entity_type": "product", "roles": [], "aliases": []}
        ], "primary_location": "Target", "primary_organization": null}"#;

        let registry = ConversationEntityRegistry::parse_response(response).unwrap();
        assert_eq!(registry.primary_location, Some("Target".to_string()));
        assert!(registry.has_entity("Target"));
        assert!(registry.has_entity("Coffee"));
        assert!(!registry.has_entity("Unknown"));
    }

    #[test]
    fn test_entity_names() {
        let mut registry = ConversationEntityRegistry::new();
        registry.entities.insert(
            "target".to_string(),
            ContextualEntity::new("Target", "store"),
        );
        registry.entities.insert(
            "walmart".to_string(),
            ContextualEntity::new("Walmart", "store"),
        );

        let names = registry.entity_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Target".to_string()));
        assert!(names.contains(&"Walmart".to_string()));
    }

    #[test]
    fn test_parse_response_with_relationships() {
        let response = r#"{"entities": [
            {"name": "Target", "entity_type": "organization", "first_turn": 0, "roles": [], "aliases": []},
            {"name": "Cartwheel", "entity_type": "product", "first_turn": 0, "roles": [], "aliases": ["the app"]}
        ], "relationships": [
            {"subject": "Cartwheel", "relation": "owned_by", "object": "Target"}
        ], "primary_location": null, "primary_organization": "Target"}"#;

        let registry = ConversationEntityRegistry::parse_response(response).unwrap();
        assert_eq!(registry.relationships.len(), 1);
        assert_eq!(registry.relationships[0].subject, "Cartwheel");
        assert_eq!(registry.relationships[0].relation, "owned_by");
        assert_eq!(registry.relationships[0].object, "Target");
    }

    #[test]
    fn test_to_typed_registry() {
        let mut registry = ConversationEntityRegistry::new();
        registry.entities.insert(
            "alice".to_string(),
            ContextualEntity::new("Alice", "person")
                .with_first_turn(0)
                .with_alias("Dr. Alice"),
        );
        registry.entities.insert(
            "google".to_string(),
            ContextualEntity::new("Google", "organization").with_first_turn(1),
        );
        registry.relationships.push(ExtractedRelationship {
            subject: "Alice".to_string(),
            relation: "works_for".to_string(),
            object: "Google".to_string(),
        });
        registry.primary_organization = Some("Google".to_string());

        let typed = registry.to_typed_registry();

        assert!(typed.contains("Alice"));
        assert!(typed.contains("Google"));

        let alice = typed.get("Alice").unwrap();
        assert_eq!(alice.entity_type, super::EntityType::Person);
        assert!(alice.aliases.contains(&"Dr. Alice".to_string()));

        let google = typed.get("Google").unwrap();
        assert_eq!(google.entity_type, super::EntityType::Organization);

        assert_eq!(typed.all_relationships().len(), 1);
        assert_eq!(typed.primary_organization, Some("Google".to_string()));
    }

    #[test]
    fn test_count_mentions_in_conversation() {
        use super::super::types::ConversationTurn;

        let conversation = Conversation::new(
            "user_1",
            vec![
                ConversationTurn::user("I went to Target today."),
                ConversationTurn::assistant("How was Target?"),
                ConversationTurn::user("Target was busy. I love Target's deals."),
            ],
        );

        let mut typed_registry = super::EntityRegistry::new();
        typed_registry.add_entity(super::ConversationEntity::new(
            "Target",
            super::EntityType::Organization,
            0,
        ));

        ConversationEntityRegistry::count_mentions_in_conversation(
            &mut typed_registry,
            &conversation,
        );

        let target = typed_registry.get("Target").unwrap();
        // "Target" appears 4 times, initial count is 1, so final should be 4
        assert_eq!(target.mention_count, 4);
    }

    #[test]
    fn test_count_mentions_with_aliases() {
        use super::super::types::ConversationTurn;

        let conversation = Conversation::new(
            "user_1",
            vec![ConversationTurn::user(
                "I talked to Alice. Dr. Alice said hello.",
            )],
        );

        let mut typed_registry = super::EntityRegistry::new();
        typed_registry.add_entity(
            super::ConversationEntity::new("Alice", super::EntityType::Person, 0)
                .with_alias("Dr. Alice"),
        );

        ConversationEntityRegistry::count_mentions_in_conversation(
            &mut typed_registry,
            &conversation,
        );

        let alice = typed_registry.get("Alice").unwrap();
        // "Alice" appears twice (in "Alice" and "Dr. Alice"), "Dr. Alice" appears once
        // Total: 3 mentions (but "Alice" in "Dr. Alice" is counted separately)
        assert!(alice.mention_count >= 2);
    }
}
