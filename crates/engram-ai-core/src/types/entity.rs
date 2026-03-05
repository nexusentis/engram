use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::EntityType;

/// Named entity tracked across memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub user_id: String,
    pub entity_type: EntityType,
    pub canonical_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub mention_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Entity {
    pub fn new(
        user_id: impl Into<String>,
        entity_type: EntityType,
        canonical_name: impl Into<String>,
    ) -> Self {
        let canonical = canonical_name.into();
        let now = Utc::now();
        Self {
            id: Self::generate_id(&canonical),
            user_id: user_id.into(),
            entity_type,
            canonical_name: canonical,
            aliases: Vec::new(),
            first_seen: now,
            last_seen: now,
            mention_count: 1,
            metadata: None,
        }
    }

    /// Generate stable ID from canonical name
    fn generate_id(name: &str) -> String {
        name.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .replace(' ', "_")
    }

    /// Record another mention of this entity
    pub fn record_mention(&mut self) {
        self.last_seen = Utc::now();
        self.mention_count += 1;
    }

    /// Add an alias for this entity
    pub fn add_alias(&mut self, alias: impl Into<String>) {
        let alias = alias.into();
        if !self.aliases.contains(&alias) && alias != self.canonical_name {
            self.aliases.push(alias);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_creation() {
        let entity = Entity::new("user_123", EntityType::Person, "Alice Smith");

        assert_eq!(entity.id, "alice_smith");
        assert_eq!(entity.canonical_name, "Alice Smith");
        assert_eq!(entity.mention_count, 1);
    }

    #[test]
    fn test_entity_record_mention() {
        let mut entity = Entity::new("user", EntityType::Organization, "Google");
        let initial_count = entity.mention_count;

        entity.record_mention();

        assert_eq!(entity.mention_count, initial_count + 1);
    }

    #[test]
    fn test_entity_add_alias() {
        let mut entity = Entity::new("user", EntityType::Person, "Robert");

        entity.add_alias("Bob");
        entity.add_alias("Bobby");
        entity.add_alias("Bob"); // Duplicate

        assert_eq!(entity.aliases.len(), 2);
        assert!(entity.aliases.contains(&"Bob".to_string()));
        assert!(entity.aliases.contains(&"Bobby".to_string()));
    }

    #[test]
    fn test_entity_serialization() {
        let entity = Entity::new("user", EntityType::Location, "San Francisco");

        let json = serde_json::to_string(&entity).unwrap();
        let deserialized: Entity = serde_json::from_str(&json).unwrap();

        assert_eq!(entity.id, deserialized.id);
        assert_eq!(entity.canonical_name, deserialized.canonical_name);
    }
}
