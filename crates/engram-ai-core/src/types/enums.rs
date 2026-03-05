use serde::{Deserialize, Serialize};

/// How the memory was learned
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// User directly stated the fact
    UserExplicit,
    /// Strongly implied by user's words
    UserImplied,
    /// AI mentioned it, user didn't confirm
    AssistantStated,
    /// Inferred from other memories
    Derived,
}

impl SourceType {
    /// Confidence multiplier for this source type
    pub fn confidence_multiplier(&self) -> f32 {
        match self {
            Self::UserExplicit => 1.0,
            Self::UserImplied => 0.8,
            Self::AssistantStated => 0.3,
            Self::Derived => 0.6,
        }
    }
}

/// Semantic category of the fact
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactType {
    /// Current state of entity ("Alice works at Google")
    State,
    /// Something that happened ("Alice got promoted")
    Event,
    /// Subjective preference ("Alice prefers Python")
    Preference,
    /// Relationship between entities ("Bob is Alice's manager")
    Relation,
}

/// Which Qdrant collection this memory belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicType {
    /// Objective external facts
    World,
    /// First-person agent experiences
    Experience,
    /// Subjective beliefs with confidence
    Opinion,
    /// Preference-neutral summaries
    Observation,
}

impl EpistemicType {
    /// Collection name in Qdrant
    pub fn collection_name(&self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Experience => "experience",
            Self::Opinion => "opinion",
            Self::Observation => "observation",
        }
    }
}

/// Entity type categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Organization,
    Location,
    Topic,
    Event,
}
