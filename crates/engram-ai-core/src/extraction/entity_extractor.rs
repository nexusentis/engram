use crate::error::Result;

use super::types::ExtractedEntity;

/// Entity extractor using GLiNER for zero-shot NER
/// Falls back to pattern matching when ONNX model is unavailable
pub struct EntityExtractor {
    /// Supported entity types for extraction
    entity_types: Vec<String>,
}

impl EntityExtractor {
    pub fn new() -> Self {
        Self {
            entity_types: vec![
                "person".to_string(),
                "organization".to_string(),
                "location".to_string(),
                "topic".to_string(),
                "event".to_string(),
            ],
        }
    }

    /// Create with custom entity types
    pub fn with_types(types: Vec<String>) -> Self {
        Self {
            entity_types: types,
        }
    }

    /// Get supported entity types
    pub fn entity_types(&self) -> &[String] {
        &self.entity_types
    }

    /// Extract entities from text
    /// Uses GLiNER ONNX model when available, falls back to pattern matching
    pub async fn extract(&self, text: &str) -> Result<Vec<ExtractedEntity>> {
        // TODO(Task 002-04): Integrate GLiNER ONNX model for zero-shot NER
        // For now, use pattern-based extraction as fallback
        let entities = self.extract_with_patterns(text);
        Ok(entities)
    }

    /// Pattern-based entity extraction (fallback when model unavailable)
    fn extract_with_patterns(&self, text: &str) -> Vec<ExtractedEntity> {
        let mut entities = Vec::new();

        // Strategy 1: Capitalized words that aren't sentence starters
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            let word = words[i];

            // Check if this is a capitalized word (not at sentence start)
            let is_capitalized = word
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false);
            let is_sentence_start =
                i == 0 || words.get(i - 1).map(|w| w.ends_with('.')).unwrap_or(false);

            if is_capitalized && !is_sentence_start {
                // Try to capture multi-word entities (e.g., "San Francisco", "Alice Smith")
                let mut entity_words = vec![word];
                let mut j = i + 1;

                while j < words.len() {
                    let next_word = words[j];
                    let next_is_capitalized = next_word
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false);

                    if next_is_capitalized {
                        // Continue with capitalized word
                        entity_words.push(next_word);
                        j += 1;
                    } else if is_connecting_word(next_word) {
                        // Check if there's a capitalized word after the connecting word
                        if j + 1 < words.len() {
                            let word_after = words[j + 1];
                            let after_is_capitalized = word_after
                                .chars()
                                .next()
                                .map(|c| c.is_uppercase())
                                .unwrap_or(false);
                            if after_is_capitalized {
                                entity_words.push(next_word);
                                j += 1;
                                continue;
                            }
                        }
                        break;
                    } else {
                        break;
                    }
                }

                // Clean and add entity
                let full_name: String = entity_words.join(" ");
                let clean_name = clean_entity_name(&full_name);

                if clean_name.len() > 1 {
                    let entity_type = self.infer_type(&clean_name);
                    entities.push(ExtractedEntity {
                        name: clean_name.clone(),
                        entity_type,
                        normalized_id: Self::normalize_id(&clean_name),
                    });
                }

                i = j;
            } else {
                i += 1;
            }
        }

        // Strategy 2: Known patterns (locations, organizations)
        self.extract_known_patterns(text, &mut entities);

        // Deduplicate by normalized_id
        entities.sort_by(|a, b| a.normalized_id.cmp(&b.normalized_id));
        entities.dedup_by(|a, b| a.normalized_id == b.normalized_id);

        entities
    }

    /// Extract entities matching known patterns
    fn extract_known_patterns(&self, text: &str, entities: &mut Vec<ExtractedEntity>) {
        // Organization patterns
        let org_suffixes = [
            "Inc", "Inc.", "Corp", "Corp.", "LLC", "Ltd", "Ltd.", "Company", "Co",
        ];
        for suffix in &org_suffixes {
            if let Some(pos) = text.find(suffix) {
                // Find the organization name before the suffix
                let before = &text[..pos];
                let words: Vec<&str> = before.split_whitespace().collect();
                if let Some(last_word) = words.last() {
                    if last_word
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false)
                    {
                        let name = format!("{} {}", last_word, suffix);
                        entities.push(ExtractedEntity {
                            name: name.clone(),
                            entity_type: "organization".to_string(),
                            normalized_id: Self::normalize_id(&name),
                        });
                    }
                }
            }
        }

        // Location patterns (common city/country names could be added)
        // This is a simplified version - real implementation would use GLiNER
    }

    /// Infer entity type from name patterns
    fn infer_type(&self, name: &str) -> String {
        let lower = name.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        let last_word = words.last().copied().unwrap_or("");

        // Known locations (check first to avoid false positives like "co" in "Francisco")
        let known_locations = [
            "new york",
            "san francisco",
            "los angeles",
            "chicago",
            "seattle",
            "boston",
            "london",
            "paris",
            "tokyo",
            "berlin",
        ];
        if known_locations.iter().any(|loc| lower.contains(loc)) {
            return "location".to_string();
        }

        // Location patterns (common suffixes)
        let location_words = [
            "city",
            "state",
            "country",
            "street",
            "avenue",
            "road",
            "boulevard",
            "park",
            "beach",
            "mountain",
            "river",
            "lake",
        ];
        if location_words.iter().any(|s| lower.contains(s)) {
            return "location".to_string();
        }

        // Organization patterns (check whole last word, not just suffix)
        let org_suffixes = [
            "inc", "inc.", "corp", "corp.", "llc", "ltd", "ltd.", "company", "co.", "co",
        ];
        if org_suffixes.contains(&last_word) {
            return "organization".to_string();
        }

        // Known tech companies and organizations
        let tech_orgs = [
            "google",
            "microsoft",
            "apple",
            "amazon",
            "meta",
            "facebook",
            "netflix",
            "twitter",
            "tesla",
            "nvidia",
            "intel",
            "ibm",
        ];
        if tech_orgs
            .iter()
            .any(|org| lower == *org || words.contains(org))
        {
            return "organization".to_string();
        }

        // Default to person for proper nouns
        "person".to_string()
    }

    /// Normalize entity name to a stable ID
    pub fn normalize_id(name: &str) -> String {
        name.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .trim()
            .replace(' ', "_")
    }

    /// Merge entities that are likely aliases of each other
    pub fn merge_aliases(entities: &mut Vec<ExtractedEntity>) {
        if entities.is_empty() {
            return;
        }

        // Sort by normalized_id for grouping
        entities.sort_by(|a, b| a.normalized_id.cmp(&b.normalized_id));

        // Merge entities where one ID is a prefix of another
        // e.g., "alice" and "alice_smith" -> keep "alice_smith"
        let mut i = 0;
        while i < entities.len().saturating_sub(1) {
            let current_id = &entities[i].normalized_id;
            let next_id = &entities[i + 1].normalized_id;

            // If next starts with current (and current ends with _, or next continues with _)
            if next_id.starts_with(current_id)
                && (next_id.len() == current_id.len()
                    || next_id.chars().nth(current_id.len()) == Some('_'))
            {
                // Remove the shorter one (keep the more specific name)
                entities.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Group entities by type
    pub fn group_by_type(
        entities: &[ExtractedEntity],
    ) -> std::collections::HashMap<String, Vec<&ExtractedEntity>> {
        let mut groups: std::collections::HashMap<String, Vec<&ExtractedEntity>> =
            std::collections::HashMap::new();

        for entity in entities {
            groups
                .entry(entity.entity_type.clone())
                .or_default()
                .push(entity);
        }

        groups
    }
}

/// Clean entity name by removing punctuation
fn clean_entity_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if a word is a connecting word in entity names
fn is_connecting_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    ["of", "the", "and", "for"].contains(&lower.as_str())
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_extractor_creation() {
        let extractor = EntityExtractor::new();
        assert_eq!(extractor.entity_types().len(), 5);
        assert!(extractor.entity_types().contains(&"person".to_string()));
        assert!(extractor
            .entity_types()
            .contains(&"organization".to_string()));
    }

    #[test]
    fn test_custom_entity_types() {
        let extractor = EntityExtractor::with_types(vec!["custom".to_string()]);
        assert_eq!(extractor.entity_types().len(), 1);
        assert!(extractor.entity_types().contains(&"custom".to_string()));
    }

    #[tokio::test]
    async fn test_entity_extraction_basic() {
        let extractor = EntityExtractor::new();
        let text = "I work at Google in San Francisco";

        let entities = extractor.extract(text).await.unwrap();

        assert!(entities.iter().any(|e| e.name == "Google"));
        assert!(entities.iter().any(|e| e.name == "San Francisco"));
    }

    #[tokio::test]
    async fn test_entity_extraction_person() {
        let extractor = EntityExtractor::new();
        let text = "Yesterday I met Alice at the coffee shop";

        let entities = extractor.extract(text).await.unwrap();

        assert!(entities.iter().any(|e| e.name == "Alice"));
        let alice = entities.iter().find(|e| e.name == "Alice").unwrap();
        assert_eq!(alice.entity_type, "person");
    }

    #[tokio::test]
    async fn test_entity_extraction_multi_word() {
        let extractor = EntityExtractor::new();
        let text = "I visited New York last summer";

        let entities = extractor.extract(text).await.unwrap();

        assert!(entities
            .iter()
            .any(|e| e.name.contains("New") || e.name.contains("York")));
    }

    #[test]
    fn test_normalize_id() {
        assert_eq!(EntityExtractor::normalize_id("Alice Smith"), "alice_smith");
        assert_eq!(EntityExtractor::normalize_id("Google Inc."), "google_inc");
        assert_eq!(
            EntityExtractor::normalize_id("San Francisco"),
            "san_francisco"
        );
        assert_eq!(EntityExtractor::normalize_id("  Trimmed  "), "trimmed");
        assert_eq!(
            EntityExtractor::normalize_id("Special@#$Chars!"),
            "specialchars"
        );
    }

    #[test]
    fn test_normalize_id_consistency() {
        // Same entity mentioned differently should normalize to same ID
        assert_eq!(
            EntityExtractor::normalize_id("alice"),
            EntityExtractor::normalize_id("Alice")
        );
        assert_eq!(
            EntityExtractor::normalize_id("GOOGLE"),
            EntityExtractor::normalize_id("Google")
        );
    }

    #[test]
    fn test_infer_type_organization() {
        let extractor = EntityExtractor::new();
        assert_eq!(extractor.infer_type("Acme Inc"), "organization");
        assert_eq!(extractor.infer_type("Google"), "organization");
        assert_eq!(extractor.infer_type("Microsoft Corp"), "organization");
    }

    #[test]
    fn test_infer_type_location() {
        let extractor = EntityExtractor::new();
        assert_eq!(extractor.infer_type("New York City"), "location");
        assert_eq!(extractor.infer_type("San Francisco"), "location");
        assert_eq!(extractor.infer_type("Main Street"), "location");
    }

    #[test]
    fn test_infer_type_person_default() {
        let extractor = EntityExtractor::new();
        assert_eq!(extractor.infer_type("Alice"), "person");
        assert_eq!(extractor.infer_type("Bob Smith"), "person");
    }

    #[test]
    fn test_merge_aliases_basic() {
        let mut entities = vec![
            ExtractedEntity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "alice".to_string(),
            },
            ExtractedEntity {
                name: "Alice Smith".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "alice_smith".to_string(),
            },
        ];

        EntityExtractor::merge_aliases(&mut entities);

        // Should keep only "Alice Smith" (more specific)
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Alice Smith");
    }

    #[test]
    fn test_merge_aliases_different_entities() {
        let mut entities = vec![
            ExtractedEntity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "alice".to_string(),
            },
            ExtractedEntity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "bob".to_string(),
            },
        ];

        EntityExtractor::merge_aliases(&mut entities);

        // Both should remain (different entities)
        assert_eq!(entities.len(), 2);
    }

    #[test]
    fn test_merge_aliases_empty() {
        let mut entities: Vec<ExtractedEntity> = vec![];
        EntityExtractor::merge_aliases(&mut entities);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_group_by_type() {
        let entities = vec![
            ExtractedEntity {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "alice".to_string(),
            },
            ExtractedEntity {
                name: "Google".to_string(),
                entity_type: "organization".to_string(),
                normalized_id: "google".to_string(),
            },
            ExtractedEntity {
                name: "Bob".to_string(),
                entity_type: "person".to_string(),
                normalized_id: "bob".to_string(),
            },
        ];

        let groups = EntityExtractor::group_by_type(&entities);

        assert_eq!(groups.get("person").unwrap().len(), 2);
        assert_eq!(groups.get("organization").unwrap().len(), 1);
    }

    #[test]
    fn test_clean_entity_name() {
        assert_eq!(clean_entity_name("Alice!"), "Alice");
        assert_eq!(clean_entity_name("Google, Inc."), "Google Inc");
        assert_eq!(clean_entity_name("San-Francisco"), "SanFrancisco");
    }

    #[tokio::test]
    async fn test_deduplication() {
        let extractor = EntityExtractor::new();
        // Text with repeated entity
        let text = "I told Alice about Alice's job at Google where Alice works";

        let entities = extractor.extract(text).await.unwrap();

        // Should have deduplicated Alice
        let alice_count = entities.iter().filter(|e| e.name == "Alice").count();
        assert!(alice_count <= 1);
    }
}
