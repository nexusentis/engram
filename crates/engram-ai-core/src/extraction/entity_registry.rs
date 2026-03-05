//! Type-safe entity registry for extraction pipelines
//!
//! Provides storage and querying for entities and relationships with type-safe enums.
//!
//! # Architecture: Two Entity Registries
//!
//! See [`context`](super::context) module for full architecture documentation.
//!
//! ## This module (`EntityRegistry`)
//! - **Purpose**: Type-safe storage after LLM extraction is complete
//! - **Entity type**: `ConversationEntity` with enum-based `EntityType`
//! - **Relationships**: `Relationship` structs with typed `RelationshipType`
//! - **Source**: Created from `ConversationEntityRegistry::to_typed_registry()`
//! - **Usage**: Merged across sessions, converted to LLM prompt context
//!
//! ## vs `ConversationEntityRegistry` (context.rs)
//! - `ConversationEntityRegistry`: Raw LLM output with string types
//! - `EntityRegistry`: Validated, type-safe, ready for storage/querying
//!
//! # Example
//!
//! ```ignore
//! // After LLM extraction
//! let llm_registry = ConversationEntityRegistry::extract(&conversation, &config).await?;
//!
//! // Convert to type-safe registry
//! let typed_registry = llm_registry.to_typed_registry();
//!
//! // Query entities
//! let organizations = typed_registry.entities_of_type(EntityType::Organization);
//!
//! // Generate prompt context for next LLM call
//! let context = typed_registry.to_prompt_context();
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::entity_types::{ConversationEntity, EntityType};
use super::relationships::{Relationship, RelationshipType};

/// Registry of entities extracted from a conversation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntityRegistry {
    /// All entities by canonical name (lowercase)
    entities: HashMap<String, ConversationEntity>,

    /// Relationships between entities
    relationships: Vec<Relationship>,

    /// Primary location mentioned in conversation (if any)
    pub primary_location: Option<String>,

    /// Primary organization mentioned (if any)
    pub primary_organization: Option<String>,
}

impl EntityRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update an entity
    ///
    /// If the entity already exists (by normalized name), increments its
    /// mention count and merges any new aliases.
    pub fn add_entity(&mut self, entity: ConversationEntity) {
        let key = entity.name.to_lowercase();

        if let Some(existing) = self.entities.get_mut(&key) {
            existing.increment_mention();
            // Merge aliases
            for alias in &entity.aliases {
                if !existing.aliases.contains(alias) {
                    existing.aliases.push(alias.clone());
                }
            }
            // Update confidence if new one is higher
            if entity.confidence > existing.confidence {
                existing.confidence = entity.confidence;
            }
        } else {
            self.entities.insert(key, entity);
        }
    }

    /// Add a relationship between entities
    pub fn add_relationship(&mut self, relationship: Relationship) {
        // Check for duplicates
        let exists = self.relationships.iter().any(|r| {
            r.subject.to_lowercase() == relationship.subject.to_lowercase()
                && r.object.to_lowercase() == relationship.object.to_lowercase()
                && r.relation == relationship.relation
        });

        if !exists {
            self.relationships.push(relationship);
        }
    }

    /// Get entity by name (case-insensitive)
    pub fn get(&self, name: &str) -> Option<&ConversationEntity> {
        self.entities.get(&name.to_lowercase())
    }

    /// Get mutable reference to entity by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut ConversationEntity> {
        self.entities.get_mut(&name.to_lowercase())
    }

    /// Check if entity exists
    pub fn contains(&self, name: &str) -> bool {
        self.entities.contains_key(&name.to_lowercase())
    }

    /// Get all entities
    pub fn all_entities(&self) -> impl Iterator<Item = &ConversationEntity> {
        self.entities.values()
    }

    /// Get number of entities
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Get all relationships
    pub fn all_relationships(&self) -> &[Relationship] {
        &self.relationships
    }

    /// Get entities of a specific type
    pub fn entities_of_type(&self, entity_type: EntityType) -> Vec<&ConversationEntity> {
        self.entities
            .values()
            .filter(|e| e.entity_type == entity_type)
            .collect()
    }

    /// Get salient entities (mention count >= threshold)
    pub fn salient_entities(&self, threshold: usize) -> Vec<&ConversationEntity> {
        self.entities
            .values()
            .filter(|e| e.is_salient(threshold))
            .collect()
    }

    /// Get entities sorted by mention count (descending)
    pub fn entities_by_salience(&self) -> Vec<&ConversationEntity> {
        let mut entities: Vec<_> = self.entities.values().collect();
        entities.sort_by(|a, b| b.mention_count.cmp(&a.mention_count));
        entities
    }

    /// Determine primary location and organization based on mention frequency
    pub fn compute_primaries(&mut self) {
        // Primary location: most mentioned location
        self.primary_location = self
            .entities_of_type(EntityType::Location)
            .into_iter()
            .max_by_key(|e| e.mention_count)
            .map(|e| e.name.clone());

        // Primary organization: most mentioned organization
        self.primary_organization = self
            .entities_of_type(EntityType::Organization)
            .into_iter()
            .max_by_key(|e| e.mention_count)
            .map(|e| e.name.clone());
    }

    /// Get relationships involving a specific entity
    pub fn relationships_for(&self, entity_name: &str) -> Vec<&Relationship> {
        self.relationships
            .iter()
            .filter(|r| r.involves(entity_name))
            .collect()
    }

    /// Get relationships of a specific type
    pub fn relationships_of_type(&self, rel_type: RelationshipType) -> Vec<&Relationship> {
        self.relationships
            .iter()
            .filter(|r| r.relation == rel_type)
            .collect()
    }

    /// Find entity by alias (case-insensitive)
    pub fn find_by_alias(&self, alias: &str) -> Option<&ConversationEntity> {
        let alias_lower = alias.to_lowercase();
        self.entities.values().find(|e| {
            e.name.to_lowercase() == alias_lower
                || e.aliases.iter().any(|a| a.to_lowercase() == alias_lower)
        })
    }

    /// Merge another registry into this one
    pub fn merge(&mut self, other: &EntityRegistry) {
        for entity in other.entities.values() {
            self.add_entity(entity.clone());
        }

        for relationship in &other.relationships {
            self.add_relationship(relationship.clone());
        }

        // Prefer existing primaries, or use other's if we don't have them
        if self.primary_location.is_none() {
            self.primary_location = other.primary_location.clone();
        }
        if self.primary_organization.is_none() {
            self.primary_organization = other.primary_organization.clone();
        }
    }

    /// Format registry for inclusion in LLM prompt
    pub fn to_prompt_context(&self) -> String {
        let mut lines = vec!["## Entity Context".to_string()];

        // List salient entities (mentioned at least twice)
        let salient = self.salient_entities(2);
        if !salient.is_empty() {
            lines.push("\nEntities established in this conversation:".into());
            for entity in salient {
                lines.push(format!(
                    "- {} ({}): mentioned {} times",
                    entity.name,
                    entity.entity_type.as_str(),
                    entity.mention_count
                ));
            }
        }

        // List relationships
        if !self.relationships.is_empty() {
            lines.push("\nRelationships:".into());
            for rel in &self.relationships {
                lines.push(format!("- {}", rel.to_display_string()));
            }
        }

        // Primary references
        if let Some(ref loc) = self.primary_location {
            lines.push(format!("\nPrimary location: {}", loc));
        }
        if let Some(ref org) = self.primary_organization {
            lines.push(format!("Primary organization: {}", org));
        }

        lines.join("\n")
    }

    /// Convert to compact format for storage
    pub fn to_compact_string(&self) -> String {
        let entity_list: Vec<String> = self
            .entities
            .values()
            .map(|e| format!("{}:{}", e.name, e.entity_type.as_str()))
            .collect();

        let rel_list: Vec<String> = self
            .relationships
            .iter()
            .map(|r| r.to_display_string())
            .collect();

        format!(
            "entities=[{}] relationships=[{}] primary_loc={:?} primary_org={:?}",
            entity_list.join(", "),
            rel_list.join("; "),
            self.primary_location,
            self.primary_organization
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = EntityRegistry::new();
        assert_eq!(registry.entity_count(), 0);
        assert!(registry.all_relationships().is_empty());
        assert!(registry.primary_location.is_none());
        assert!(registry.primary_organization.is_none());
    }

    #[test]
    fn test_registry_add_entity() {
        let mut registry = EntityRegistry::new();

        registry.add_entity(ConversationEntity::new(
            "Target",
            EntityType::Organization,
            0,
        ));
        assert_eq!(registry.entity_count(), 1);

        let target = registry.get("target").unwrap();
        assert_eq!(target.name, "Target");
        assert_eq!(target.mention_count, 1);
    }

    #[test]
    fn test_registry_add_entity_duplicate() {
        let mut registry = EntityRegistry::new();

        registry.add_entity(ConversationEntity::new(
            "Target",
            EntityType::Organization,
            0,
        ));
        registry.add_entity(ConversationEntity::new(
            "Target",
            EntityType::Organization,
            2,
        ));

        assert_eq!(registry.entity_count(), 1);

        let target = registry.get("target").unwrap();
        assert_eq!(target.mention_count, 2);
    }

    #[test]
    fn test_registry_add_entity_merge_aliases() {
        let mut registry = EntityRegistry::new();

        registry.add_entity(
            ConversationEntity::new("Alice", EntityType::Person, 0).with_alias("Dr. Alice"),
        );
        registry
            .add_entity(ConversationEntity::new("Alice", EntityType::Person, 2).with_alias("A."));

        let alice = registry.get("alice").unwrap();
        assert_eq!(alice.aliases.len(), 2);
        assert!(alice.aliases.contains(&"Dr. Alice".to_string()));
        assert!(alice.aliases.contains(&"A.".to_string()));
    }

    #[test]
    fn test_registry_contains() {
        let mut registry = EntityRegistry::new();
        registry.add_entity(ConversationEntity::new(
            "Target",
            EntityType::Organization,
            0,
        ));

        assert!(registry.contains("Target"));
        assert!(registry.contains("target")); // Case insensitive
        assert!(!registry.contains("Walmart"));
    }

    #[test]
    fn test_registry_entities_of_type() {
        let mut registry = EntityRegistry::new();
        registry.add_entity(ConversationEntity::new("Alice", EntityType::Person, 0));
        registry.add_entity(ConversationEntity::new("Bob", EntityType::Person, 1));
        registry.add_entity(ConversationEntity::new(
            "Google",
            EntityType::Organization,
            2,
        ));

        let people = registry.entities_of_type(EntityType::Person);
        assert_eq!(people.len(), 2);

        let orgs = registry.entities_of_type(EntityType::Organization);
        assert_eq!(orgs.len(), 1);
    }

    #[test]
    fn test_registry_salient_entities() {
        let mut registry = EntityRegistry::new();

        let mut target = ConversationEntity::new("Target", EntityType::Organization, 0);
        target.mention_count = 5;
        registry.add_entity(target);

        registry.add_entity(ConversationEntity::new(
            "Walmart",
            EntityType::Organization,
            1,
        ));

        let salient = registry.salient_entities(3);
        assert_eq!(salient.len(), 1);
        assert_eq!(salient[0].name, "Target");
    }

    #[test]
    fn test_registry_compute_primaries() {
        let mut registry = EntityRegistry::new();

        // Add locations with different frequencies
        let mut ny = ConversationEntity::new("New York", EntityType::Location, 0);
        ny.mention_count = 5;
        registry.add_entity(ny);

        registry.add_entity(ConversationEntity::new(
            "Los Angeles",
            EntityType::Location,
            1,
        ));

        // Add organizations
        let mut google = ConversationEntity::new("Google", EntityType::Organization, 0);
        google.mention_count = 3;
        registry.add_entity(google);

        registry.compute_primaries();

        assert_eq!(registry.primary_location, Some("New York".to_string()));
        assert_eq!(registry.primary_organization, Some("Google".to_string()));
    }

    #[test]
    fn test_registry_add_relationship() {
        let mut registry = EntityRegistry::new();

        registry.add_relationship(Relationship::new(
            "Cartwheel",
            RelationshipType::OwnedBy,
            "Target",
        ));

        assert_eq!(registry.all_relationships().len(), 1);
    }

    #[test]
    fn test_registry_add_relationship_no_duplicates() {
        let mut registry = EntityRegistry::new();

        registry.add_relationship(Relationship::new("A", RelationshipType::OwnedBy, "B"));
        registry.add_relationship(Relationship::new("A", RelationshipType::OwnedBy, "B"));

        assert_eq!(registry.all_relationships().len(), 1);
    }

    #[test]
    fn test_registry_relationships_for() {
        let mut registry = EntityRegistry::new();

        registry.add_relationship(Relationship::new(
            "Alice",
            RelationshipType::WorksFor,
            "Google",
        ));
        registry.add_relationship(Relationship::new(
            "Bob",
            RelationshipType::WorksFor,
            "Google",
        ));
        registry.add_relationship(Relationship::new(
            "Alice",
            RelationshipType::LocatedIn,
            "NY",
        ));

        let alice_rels = registry.relationships_for("Alice");
        assert_eq!(alice_rels.len(), 2);

        let google_rels = registry.relationships_for("Google");
        assert_eq!(google_rels.len(), 2);
    }

    #[test]
    fn test_registry_find_by_alias() {
        let mut registry = EntityRegistry::new();
        registry.add_entity(
            ConversationEntity::new("Alice Smith", EntityType::Person, 0)
                .with_alias("Alice")
                .with_alias("Dr. Smith"),
        );

        assert!(registry.find_by_alias("Alice Smith").is_some());
        assert!(registry.find_by_alias("Alice").is_some());
        assert!(registry.find_by_alias("Dr. Smith").is_some());
        assert!(registry.find_by_alias("Bob").is_none());
    }

    #[test]
    fn test_registry_merge() {
        let mut registry1 = EntityRegistry::new();
        registry1.add_entity(ConversationEntity::new("Alice", EntityType::Person, 0));
        registry1.primary_location = Some("New York".to_string());

        let mut registry2 = EntityRegistry::new();
        registry2.add_entity(ConversationEntity::new("Bob", EntityType::Person, 0));
        registry2.add_relationship(Relationship::new(
            "Bob",
            RelationshipType::WorksFor,
            "Google",
        ));
        registry2.primary_organization = Some("Google".to_string());

        registry1.merge(&registry2);

        assert_eq!(registry1.entity_count(), 2);
        assert!(registry1.contains("Alice"));
        assert!(registry1.contains("Bob"));
        assert_eq!(registry1.all_relationships().len(), 1);
        assert_eq!(registry1.primary_location, Some("New York".to_string()));
        assert_eq!(registry1.primary_organization, Some("Google".to_string()));
    }

    #[test]
    fn test_registry_to_prompt_context() {
        let mut registry = EntityRegistry::new();

        let mut target = ConversationEntity::new("Target", EntityType::Organization, 0);
        target.mention_count = 3;
        registry.add_entity(target);

        registry.add_relationship(Relationship::new(
            "Cartwheel",
            RelationshipType::OwnedBy,
            "Target",
        ));

        registry.primary_organization = Some("Target".to_string());

        let context = registry.to_prompt_context();

        assert!(context.contains("Entity Context"));
        assert!(context.contains("Target (organization)"));
        assert!(context.contains("Cartwheel is owned by Target"));
        assert!(context.contains("Primary organization: Target"));
    }

    #[test]
    fn test_registry_serialization() {
        let mut registry = EntityRegistry::new();
        registry.add_entity(ConversationEntity::new("Alice", EntityType::Person, 0));
        registry.add_relationship(Relationship::new(
            "Alice",
            RelationshipType::WorksFor,
            "Google",
        ));
        registry.primary_organization = Some("Google".to_string());

        let json = serde_json::to_string(&registry).unwrap();
        let deserialized: EntityRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.entity_count(), 1);
        assert!(deserialized.contains("Alice"));
        assert_eq!(deserialized.all_relationships().len(), 1);
        assert_eq!(
            deserialized.primary_organization,
            Some("Google".to_string())
        );
    }

    #[test]
    fn test_registry_entities_by_salience() {
        let mut registry = EntityRegistry::new();

        let mut a = ConversationEntity::new("A", EntityType::Person, 0);
        a.mention_count = 1;
        registry.add_entity(a);

        let mut b = ConversationEntity::new("B", EntityType::Person, 1);
        b.mention_count = 5;
        registry.add_entity(b);

        let mut c = ConversationEntity::new("C", EntityType::Person, 2);
        c.mention_count = 3;
        registry.add_entity(c);

        let sorted = registry.entities_by_salience();
        assert_eq!(sorted[0].name, "B");
        assert_eq!(sorted[1].name, "C");
        assert_eq!(sorted[2].name, "A");
    }
}
