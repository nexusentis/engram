//! Entity relationship definitions for entity-aware extraction
//!
//! Tracks relationships between entities in a conversation context.

use serde::{Deserialize, Serialize};

/// Type of relationship between entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    /// Entity A is owned by Entity B (Cartwheel owned by Target)
    OwnedBy,

    /// Entity A is located in Entity B (Alice works in New York)
    LocatedIn,

    /// Entity A works for Entity B (Alice works at Google)
    WorksFor,

    /// Entity A is part of Entity B (Sales is part of Company)
    PartOf,

    /// Entity A is related to Entity B (family member, friend)
    RelatedTo,

    /// Generic association
    AssociatedWith,
}

impl RelationshipType {
    /// Get the verb phrase for this relationship
    pub fn verb_phrase(&self) -> &'static str {
        match self {
            Self::OwnedBy => "is owned by",
            Self::LocatedIn => "is located in",
            Self::WorksFor => "works for",
            Self::PartOf => "is part of",
            Self::RelatedTo => "is related to",
            Self::AssociatedWith => "is associated with",
        }
    }

    /// Get the inverse relationship type if applicable
    pub fn inverse(&self) -> Option<Self> {
        match self {
            Self::OwnedBy => Some(Self::PartOf), // Rough inverse
            Self::PartOf => Some(Self::OwnedBy),
            _ => None,
        }
    }

    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OwnedBy => "owned_by",
            Self::LocatedIn => "located_in",
            Self::WorksFor => "works_for",
            Self::PartOf => "part_of",
            Self::RelatedTo => "related_to",
            Self::AssociatedWith => "associated_with",
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().replace(' ', "_").as_str() {
            "owned_by" | "belongs_to" => Some(Self::OwnedBy),
            "located_in" | "in" | "at" => Some(Self::LocatedIn),
            "works_for" | "employed_by" | "works_at" => Some(Self::WorksFor),
            "part_of" | "member_of" => Some(Self::PartOf),
            "related_to" | "family" | "knows" => Some(Self::RelatedTo),
            "associated_with" | "with" => Some(Self::AssociatedWith),
            _ => None,
        }
    }
}

impl std::fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.verb_phrase())
    }
}

/// A relationship between two entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Subject entity name
    pub subject: String,

    /// Relationship type
    pub relation: RelationshipType,

    /// Object entity name
    pub object: String,

    /// Confidence score (0-1)
    pub confidence: f32,

    /// Turn where relationship was established
    pub source_turn: Option<usize>,
}

impl Relationship {
    /// Create a new relationship
    pub fn new(
        subject: impl Into<String>,
        relation: RelationshipType,
        object: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            relation,
            object: object.into(),
            confidence: 1.0,
            source_turn: None,
        }
    }

    /// Set confidence score
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Set source turn
    pub fn with_source_turn(mut self, turn: usize) -> Self {
        self.source_turn = Some(turn);
        self
    }

    /// Format as human-readable string
    pub fn to_display_string(&self) -> String {
        format!(
            "{} {} {}",
            self.subject,
            self.relation.verb_phrase(),
            self.object
        )
    }

    /// Check if this relationship involves a specific entity
    pub fn involves(&self, entity_name: &str) -> bool {
        let name_lower = entity_name.to_lowercase();
        self.subject.to_lowercase() == name_lower || self.object.to_lowercase() == name_lower
    }

    /// Check if this relationship matches subject and object
    pub fn matches(&self, subject: &str, object: &str) -> bool {
        self.subject.to_lowercase() == subject.to_lowercase()
            && self.object.to_lowercase() == object.to_lowercase()
    }
}

impl std::fmt::Display for Relationship {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relationship_type_verb_phrase() {
        assert_eq!(RelationshipType::OwnedBy.verb_phrase(), "is owned by");
        assert_eq!(RelationshipType::LocatedIn.verb_phrase(), "is located in");
        assert_eq!(RelationshipType::WorksFor.verb_phrase(), "works for");
        assert_eq!(RelationshipType::PartOf.verb_phrase(), "is part of");
        assert_eq!(
            RelationshipType::AssociatedWith.verb_phrase(),
            "is associated with"
        );
    }

    #[test]
    fn test_relationship_type_from_str_loose() {
        assert_eq!(
            RelationshipType::from_str_loose("owned_by"),
            Some(RelationshipType::OwnedBy)
        );
        assert_eq!(
            RelationshipType::from_str_loose("OWNED BY"),
            Some(RelationshipType::OwnedBy)
        );
        assert_eq!(
            RelationshipType::from_str_loose("works_for"),
            Some(RelationshipType::WorksFor)
        );
        assert_eq!(
            RelationshipType::from_str_loose("works at"),
            Some(RelationshipType::WorksFor)
        );
        assert_eq!(RelationshipType::from_str_loose("unknown"), None);
    }

    #[test]
    fn test_relationship_creation() {
        let rel = Relationship::new("Cartwheel", RelationshipType::OwnedBy, "Target")
            .with_confidence(0.95)
            .with_source_turn(3);

        assert_eq!(rel.subject, "Cartwheel");
        assert_eq!(rel.relation, RelationshipType::OwnedBy);
        assert_eq!(rel.object, "Target");
        assert!((rel.confidence - 0.95).abs() < 0.01);
        assert_eq!(rel.source_turn, Some(3));
    }

    #[test]
    fn test_relationship_display() {
        let rel = Relationship::new("Cartwheel", RelationshipType::OwnedBy, "Target");
        assert_eq!(rel.to_display_string(), "Cartwheel is owned by Target");
        assert_eq!(format!("{}", rel), "Cartwheel is owned by Target");
    }

    #[test]
    fn test_relationship_involves() {
        let rel = Relationship::new("Alice", RelationshipType::WorksFor, "Google");

        assert!(rel.involves("Alice"));
        assert!(rel.involves("alice")); // Case insensitive
        assert!(rel.involves("Google"));
        assert!(!rel.involves("Bob"));
    }

    #[test]
    fn test_relationship_matches() {
        let rel = Relationship::new("Alice", RelationshipType::WorksFor, "Google");

        assert!(rel.matches("Alice", "Google"));
        assert!(rel.matches("alice", "google")); // Case insensitive
        assert!(!rel.matches("Google", "Alice")); // Order matters
    }

    #[test]
    fn test_relationship_serialization() {
        let rel = Relationship::new("Cartwheel", RelationshipType::OwnedBy, "Target");
        let json = serde_json::to_string(&rel).unwrap();

        assert!(json.contains("\"subject\":\"Cartwheel\""));
        assert!(json.contains("\"relation\":\"owned_by\""));
        assert!(json.contains("\"object\":\"Target\""));

        let deserialized: Relationship = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.subject, "Cartwheel");
        assert_eq!(deserialized.relation, RelationshipType::OwnedBy);
        assert_eq!(deserialized.object, "Target");
    }
}
