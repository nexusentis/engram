//! Batch request generation and result parsing for OpenAI Batch API
//!
//! Generates JSONL batch request files and parses results back to ExtractedFacts.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::api_config::ApiExtractorConfig;
use super::types::{Conversation, ExtractedEntity, ExtractedFact, Role};
use crate::error::{ExtractionError, Result};
use crate::types::{EpistemicType, FactType, SourceType};

/// Same extraction prompt used by ApiExtractor
const EXTRACTION_PROMPT: &str = r#"Extract 2-5 comprehensive narrative facts from this conversation. Each fact should:
1. Be a complete narrative (not sentence fragments)
2. Preserve context and relationships
3. Include confidence score (0.0-1.0)
4. Classify fact_type: state, event, preference, or relation
5. Classify epistemic_type: world (objective), experience (first-person), opinion (subjective), observation (neutral summary)
6. List mentioned entities

Respond ONLY with valid JSON:
{"facts": [{"content": "...", "confidence": 0.9, "fact_type": "state", "epistemic_type": "world", "entities": ["entity1"]}]}"#;

/// A single request in the JSONL batch file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    pub custom_id: String,
    pub method: String,
    pub url: String,
    pub body: BatchRequestBody,
}

/// The body of a batch request (matches OpenAI chat completions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequestBody {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// A single result line from the batch output
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResultLine {
    pub id: String,
    pub custom_id: String,
    pub response: Option<BatchResponse>,
    pub error: Option<BatchError>,
}

/// The response portion of a batch result
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResponse {
    pub status_code: u16,
    pub body: BatchResponseBody,
}

/// The body of a batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResponseBody {
    pub id: String,
    pub choices: Vec<BatchChoice>,
    pub usage: Option<BatchUsage>,
}

/// A choice in the batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchChoice {
    pub index: u32,
    pub message: BatchMessage,
    pub finish_reason: String,
}

/// A message in the batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchMessage {
    pub role: String,
    pub content: String,
}

/// Usage information in the batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Error in the batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchError {
    pub code: String,
    pub message: String,
}

/// Batch extractor for generating JSONL requests and parsing results
pub struct BatchExtractor {
    config: ApiExtractorConfig,
}

impl BatchExtractor {
    /// Create a new batch extractor with the given config
    pub fn new(config: ApiExtractorConfig) -> Self {
        Self { config }
    }

    /// Get the model name
    pub fn model(&self) -> &str {
        &self.config.model
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

    /// Generate a single JSONL line for a session
    ///
    /// The custom_id is set to the session_id for correlation when parsing results.
    pub fn create_request(&self, session_id: &str, conversation: &Conversation) -> String {
        let messages = self.format_messages(conversation);

        // Use config field if set, else auto-detect from model name
        let supports_temp = self.config.supports_temperature.unwrap_or_else(|| {
            !self.config.model.starts_with("gpt-5")
                && !self.config.model.starts_with("o")
                && !self.config.model.contains("nano")
        });
        let temperature = if supports_temp {
            Some(self.config.temperature.unwrap_or(0.1))
        } else {
            None
        };

        let request = BatchRequest {
            custom_id: session_id.to_string(),
            method: "POST".to_string(),
            url: "/v1/chat/completions".to_string(),
            body: BatchRequestBody {
                model: self.config.model.clone(),
                messages,
                temperature,
            },
        };

        serde_json::to_string(&request).expect("BatchRequest should serialize")
    }

    /// Parse a batch result line back to ExtractedFacts
    ///
    /// Returns (session_id, extracted_facts) on success.
    pub fn parse_result(&self, result_line: &str) -> Result<(String, Vec<ExtractedFact>)> {
        let result: BatchResultLine = serde_json::from_str(result_line)
            .map_err(|e| ExtractionError::Api(format!("Failed to parse batch result: {}", e)))?;

        let session_id = result.custom_id;

        // Check for error
        if let Some(error) = result.error {
            return Err(ExtractionError::Api(format!(
                "Batch error for {}: {}",
                session_id, error.message
            ))
            .into());
        }

        // Get response
        let response = result.response.ok_or_else(|| {
            ExtractionError::Api(format!("No response for session {}", session_id))
        })?;

        if response.status_code != 200 {
            return Err(ExtractionError::Api(format!(
                "Non-200 status for session {}: {}",
                session_id, response.status_code
            ))
            .into());
        }

        // Extract content from response
        let content = response
            .body
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .ok_or_else(|| {
                ExtractionError::Api(format!("No choices in response for session {}", session_id))
            })?;

        // Parse the content to facts
        let facts = self.parse_response(content);

        Ok((session_id, facts))
    }

    /// Parse API response to extracted facts (same logic as ApiExtractor)
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
                            Some(ExtractedFact {
                                content: f["content"].as_str()?.to_string(),
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
                                entities: f["entities"]
                                    .as_array()
                                    .map(|e| {
                                        e.iter()
                                            .filter_map(|v| {
                                                let name = v.as_str()?.to_string();
                                                Some(ExtractedEntity {
                                                    normalized_id: name
                                                        .to_lowercase()
                                                        .replace(' ', "_"),
                                                    name,
                                                    entity_type: "unknown".to_string(),
                                                })
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                temporal_markers: vec![],
                                t_valid: None,
                                observation_level: "explicit".to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!("Failed to parse batch response: {}", e);
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::types::ConversationTurn;

    #[test]
    fn test_create_request() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        let extractor = BatchExtractor::new(config);

        let conv = Conversation::new("user_1", vec![ConversationTurn::user("I work at Google")]);

        let jsonl = extractor.create_request("session_123", &conv);

        // Verify it's valid JSON
        let parsed: BatchRequest = serde_json::from_str(&jsonl).unwrap();
        assert_eq!(parsed.custom_id, "session_123");
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.url, "/v1/chat/completions");
        assert_eq!(parsed.body.model, "gpt-4o-mini");
        assert_eq!(parsed.body.temperature, Some(0.1));
        assert_eq!(parsed.body.messages.len(), 2); // system + user
    }

    #[test]
    fn test_create_request_nano_no_temperature() {
        let config = ApiExtractorConfig::openai("gpt-5-nano");
        let extractor = BatchExtractor::new(config);

        let conv = Conversation::new("user_1", vec![ConversationTurn::user("Test")]);

        let jsonl = extractor.create_request("session_123", &conv);
        let parsed: BatchRequest = serde_json::from_str(&jsonl).unwrap();
        assert_eq!(parsed.body.temperature, None);
    }

    #[test]
    fn test_parse_result_success() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        let extractor = BatchExtractor::new(config);

        let result_line = r#"{
            "id": "batch_req_123",
            "custom_id": "session_456",
            "response": {
                "status_code": 200,
                "body": {
                    "id": "chatcmpl-123",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "{\"facts\": [{\"content\": \"User works at Google\", \"confidence\": 0.9, \"fact_type\": \"state\", \"epistemic_type\": \"world\", \"entities\": [\"Google\"]}]}"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 100,
                        "completion_tokens": 50,
                        "total_tokens": 150
                    }
                }
            },
            "error": null
        }"#;

        let (session_id, facts) = extractor.parse_result(result_line).unwrap();
        assert_eq!(session_id, "session_456");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "User works at Google");
        assert_eq!(facts[0].entities.len(), 1);
        assert_eq!(facts[0].entities[0].name, "Google");
    }

    #[test]
    fn test_parse_result_error() {
        let config = ApiExtractorConfig::openai("gpt-4o-mini");
        let extractor = BatchExtractor::new(config);

        let result_line = r#"{
            "id": "batch_req_123",
            "custom_id": "session_456",
            "response": null,
            "error": {
                "code": "rate_limit_exceeded",
                "message": "Rate limit exceeded"
            }
        }"#;

        let result = extractor.parse_result(result_line);
        assert!(result.is_err());
    }
}
