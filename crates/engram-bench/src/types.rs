//! Benchmark data types
//!
//! Types for benchmark datasets, questions, and results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use engram::types::SessionEntityContext;

/// A benchmark session from the dataset
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkSession {
    /// Unique session identifier
    pub session_id: String,
    /// User identifier
    pub user_id: String,
    /// Messages in the session
    pub messages: Vec<BenchmarkMessage>,
    /// Optional metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl BenchmarkSession {
    /// Create a new benchmark session
    pub fn new(session_id: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            user_id: user_id.into(),
            messages: Vec::new(),
            metadata: None,
        }
    }

    /// Add a message to the session
    pub fn with_message(mut self, message: BenchmarkMessage) -> Self {
        self.messages.push(message);
        self
    }

    /// Get the earliest timestamp in the session
    pub fn earliest_timestamp(&self) -> Option<DateTime<Utc>> {
        self.messages.iter().map(|m| m.timestamp).min()
    }

    /// Get the latest timestamp in the session
    pub fn latest_timestamp(&self) -> Option<DateTime<Utc>> {
        self.messages.iter().map(|m| m.timestamp).max()
    }

    /// Get session duration
    pub fn duration(&self) -> Option<chrono::Duration> {
        match (self.earliest_timestamp(), self.latest_timestamp()) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        }
    }
}

/// A message in a benchmark session
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkMessage {
    /// Message role (user, assistant, system)
    pub role: String,
    /// Message content
    pub content: String,
    /// Original timestamp from dataset
    pub timestamp: DateTime<Utc>,
}

impl BenchmarkMessage {
    /// Create a user message
    pub fn user(content: impl Into<String>, timestamp: DateTime<Utc>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            timestamp,
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>, timestamp: DateTime<Utc>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            timestamp,
        }
    }

    /// Create a system message
    pub fn system(content: impl Into<String>, timestamp: DateTime<Utc>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            timestamp,
        }
    }
}

/// A benchmark question
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BenchmarkQuestion {
    /// Unique question identifier
    pub id: String,
    /// The question text
    pub question: String,
    /// Expected answer
    pub answer: String,
    /// Question category
    pub category: QuestionCategory,
    /// Session IDs containing relevant information (all haystack sessions)
    #[serde(default)]
    pub session_ids: Vec<String>,
    /// Session IDs that contain the answer (subset of session_ids)
    #[serde(default)]
    pub answer_session_ids: Vec<String>,
    /// The date context for this question (when the question is "asked")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_date: Option<DateTime<Utc>>,
    /// Optional metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl BenchmarkQuestion {
    /// Create a new benchmark question
    pub fn new(
        id: impl Into<String>,
        question: impl Into<String>,
        answer: impl Into<String>,
        category: QuestionCategory,
    ) -> Self {
        Self {
            id: id.into(),
            question: question.into(),
            answer: answer.into(),
            category,
            session_ids: Vec::new(),
            answer_session_ids: Vec::new(),
            question_date: None,
            metadata: None,
        }
    }

    /// Add session IDs
    pub fn with_session_ids(mut self, ids: Vec<String>) -> Self {
        self.session_ids = ids;
        self
    }

    /// Set the question date
    pub fn with_question_date(mut self, date: DateTime<Utc>) -> Self {
        self.question_date = Some(date);
        self
    }
}

/// LongMemEval-S question categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionCategory {
    /// Fact extraction questions (single-session-user, assistant, preference)
    Extraction,
    /// Multi-session reasoning questions
    #[serde(alias = "reasoning")]
    MultiSession,
    /// Information update questions
    Updates,
    /// Temporal reasoning questions
    Temporal,
    /// Questions that should be abstained (ID ends with _abs)
    Abstention,
}

impl QuestionCategory {
    /// Get all categories
    pub fn all() -> Vec<Self> {
        vec![
            Self::Extraction,
            Self::MultiSession,
            Self::Updates,
            Self::Temporal,
            Self::Abstention,
        ]
    }

    /// Get category as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extraction => "extraction",
            Self::MultiSession => "multi_session",
            Self::Updates => "updates",
            Self::Temporal => "temporal",
            Self::Abstention => "abstention",
        }
    }

    /// Get display name for reporting
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Extraction => "Extraction",
            Self::MultiSession => "Multi-Session",
            Self::Updates => "Knowledge Updates",
            Self::Temporal => "Temporal Reasoning",
            Self::Abstention => "Abstention",
        }
    }

    /// Parse from string (for internal use, prefer FromStr trait)
    pub fn parse_str(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl std::str::FromStr for QuestionCategory {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "extraction" => Ok(Self::Extraction),
            "multi_session" | "multisession" | "reasoning" => Ok(Self::MultiSession),
            "updates" => Ok(Self::Updates),
            "temporal" => Ok(Self::Temporal),
            "abstention" => Ok(Self::Abstention),
            _ => Err(format!("unknown category: {}", s)),
        }
    }
}

impl std::fmt::Display for QuestionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Benchmark run configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Name of the benchmark run
    pub name: String,
    /// Extraction mode to use
    pub extraction_mode: String,
    /// Whether to use LLM for answer generation
    pub use_llm_answers: bool,
    /// Model for generating answers
    pub answer_model: String,
    /// Model for judging correctness
    pub judge_model: String,
    /// Maximum questions to evaluate (0 = all)
    pub max_questions: usize,
    /// Categories to include (empty = all)
    pub categories: Vec<QuestionCategory>,
    /// Random seed for reproducibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            extraction_mode: "local-fast".to_string(),
            use_llm_answers: true,
            answer_model: "gpt-4o".to_string(),
            judge_model: "gpt-4o".to_string(),
            max_questions: 0,
            categories: vec![],
            seed: None,
        }
    }
}

impl BenchmarkConfig {
    /// Create a new config with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set extraction mode
    pub fn with_extraction_mode(mut self, mode: impl Into<String>) -> Self {
        self.extraction_mode = mode.into();
        self
    }

    /// Set answer model
    pub fn with_answer_model(mut self, model: impl Into<String>) -> Self {
        self.answer_model = model.into();
        self
    }

    /// Set judge model
    pub fn with_judge_model(mut self, model: impl Into<String>) -> Self {
        self.judge_model = model.into();
        self
    }

    /// Set max questions
    pub fn with_max_questions(mut self, max: usize) -> Self {
        self.max_questions = max;
        self
    }

    /// Set categories
    pub fn with_categories(mut self, categories: Vec<QuestionCategory>) -> Self {
        self.categories = categories;
        self
    }

    /// Set seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Check if a category should be included
    pub fn includes_category(&self, category: QuestionCategory) -> bool {
        self.categories.is_empty() || self.categories.contains(&category)
    }
}

/// Result of answering a single question
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResult {
    /// Question ID
    pub question_id: String,
    /// Question category
    pub category: QuestionCategory,
    /// The question text
    pub question: String,
    /// Expected answer
    pub expected_answer: String,
    /// Generated answer
    pub generated_answer: String,
    /// Retrieved memories used
    pub retrieved_memories: Vec<RetrievedMemoryInfo>,
    /// Whether the answer was judged correct
    pub is_correct: bool,
    /// Judge score (0.0 - 1.0)
    pub judge_score: f32,
    /// Judge reasoning
    pub judge_reasoning: String,
    /// Retrieval time in milliseconds
    pub retrieval_time_ms: u64,
    /// Answer generation time in milliseconds
    pub answer_time_ms: u64,
    /// Total time in milliseconds
    pub total_time_ms: u64,
}

/// Information about a retrieved memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedMemoryInfo {
    /// Memory ID
    pub id: Uuid,
    /// Memory content
    pub content: String,
    /// Retrieval score (vector similarity)
    pub score: f32,
    /// Cross-encoder reranker score (if reranking enabled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reranker_score: Option<f32>,
    /// Session entity context for implicit reference resolution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_entity_context: Option<SessionEntityContext>,
    /// Session ID this memory belongs to (for session-level expansion)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Validity timestamp from the original session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t_valid: Option<DateTime<Utc>>,
    /// Whether this is a raw message (vs extracted fact)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_message: Option<bool>,
    /// Role of the speaker (user/assistant) for messages
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Turn index within the session for ordering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<i64>,
}

impl RetrievedMemoryInfo {
    /// Create a new retrieved memory info
    pub fn new(id: Uuid, content: impl Into<String>, score: f32) -> Self {
        Self {
            id,
            content: content.into(),
            score,
            reranker_score: None,
            session_entity_context: None,
            session_id: None,
            t_valid: None,
            is_message: None,
            role: None,
            turn_index: None,
        }
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set reranker score
    pub fn with_reranker_score(mut self, score: f32) -> Self {
        self.reranker_score = Some(score);
        self
    }

    /// Set session entity context
    pub fn with_session_entity_context(mut self, context: SessionEntityContext) -> Self {
        self.session_entity_context = Some(context);
        self
    }

    /// Set validity timestamp
    pub fn with_t_valid(mut self, t_valid: DateTime<Utc>) -> Self {
        self.t_valid = Some(t_valid);
        self
    }

    /// Set role
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Set turn index
    pub fn with_turn_index(mut self, idx: i64) -> Self {
        self.turn_index = Some(idx);
        self
    }

    /// Get the effective score (reranker if available, else vector similarity)
    pub fn effective_score(&self) -> f32 {
        self.reranker_score.unwrap_or(self.score)
    }

    /// Get primary location from session entity context
    pub fn primary_location(&self) -> Option<&str> {
        self.session_entity_context
            .as_ref()
            .and_then(|ctx| ctx.primary_location.as_deref())
    }

    /// Get primary organization from session entity context
    pub fn primary_organization(&self) -> Option<&str> {
        self.session_entity_context
            .as_ref()
            .and_then(|ctx| ctx.primary_organization.as_deref())
    }
}

/// Summary of a benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Unique run ID
    pub run_id: Uuid,
    /// Benchmark name
    pub benchmark_name: String,
    /// Configuration used
    pub config: BenchmarkConfig,
    /// When the run started
    pub started_at: DateTime<Utc>,
    /// When the run completed
    pub completed_at: DateTime<Utc>,
    /// Total questions evaluated
    pub total_questions: usize,
    /// Correct answer count
    pub correct_count: usize,
    /// Overall accuracy
    pub accuracy: f32,
    /// Per-category scores
    pub category_scores: Vec<CategoryScore>,
    /// Individual question results
    pub question_results: Vec<QuestionResult>,
    /// Total time in seconds
    pub total_time_seconds: f32,
    /// Estimated cost in USD
    pub estimated_cost_usd: f32,
}

impl BenchmarkResult {
    /// Create a new benchmark result (to be filled in)
    pub fn new(benchmark_name: impl Into<String>, config: BenchmarkConfig) -> Self {
        Self {
            run_id: Uuid::now_v7(),
            benchmark_name: benchmark_name.into(),
            config,
            started_at: Utc::now(),
            completed_at: Utc::now(),
            total_questions: 0,
            correct_count: 0,
            accuracy: 0.0,
            category_scores: Vec::new(),
            question_results: Vec::new(),
            total_time_seconds: 0.0,
            estimated_cost_usd: 0.0,
        }
    }

    /// Add a question result
    pub fn add_result(&mut self, result: QuestionResult) {
        self.question_results.push(result);
    }

    /// Calculate scores from question results
    pub fn calculate_scores(mut self) -> Self {
        self.correct_count = self
            .question_results
            .iter()
            .filter(|r| r.is_correct)
            .count();
        self.total_questions = self.question_results.len();
        self.accuracy = if self.total_questions > 0 {
            self.correct_count as f32 / self.total_questions as f32
        } else {
            0.0
        };

        // Calculate per-category scores
        self.category_scores.clear();
        for category in QuestionCategory::all() {
            let category_results: Vec<_> = self
                .question_results
                .iter()
                .filter(|r| r.category == category)
                .collect();

            let total = category_results.len();
            let correct = category_results.iter().filter(|r| r.is_correct).count();

            self.category_scores.push(CategoryScore {
                category,
                total,
                correct,
                accuracy: if total > 0 {
                    correct as f32 / total as f32
                } else {
                    0.0
                },
            });
        }

        self.completed_at = Utc::now();
        self.total_time_seconds =
            (self.completed_at - self.started_at).num_milliseconds() as f32 / 1000.0;

        self
    }

    /// Get accuracy for a specific category
    pub fn category_accuracy(&self, category: QuestionCategory) -> Option<f32> {
        self.category_scores
            .iter()
            .find(|s| s.category == category)
            .map(|s| s.accuracy)
    }

    /// Check if the target accuracy is met
    pub fn meets_target(&self, target: f32) -> bool {
        self.accuracy >= target
    }
}

/// Per-category score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryScore {
    /// Category
    pub category: QuestionCategory,
    /// Total questions in category
    pub total: usize,
    /// Correct answers in category
    pub correct: usize,
    /// Accuracy for this category
    pub accuracy: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_question_category_all() {
        let categories = QuestionCategory::all();
        assert_eq!(categories.len(), 5);
    }

    #[test]
    fn test_question_category_as_str() {
        assert_eq!(QuestionCategory::Extraction.as_str(), "extraction");
        assert_eq!(QuestionCategory::MultiSession.as_str(), "multi_session");
        assert_eq!(QuestionCategory::Updates.as_str(), "updates");
        assert_eq!(QuestionCategory::Temporal.as_str(), "temporal");
        assert_eq!(QuestionCategory::Abstention.as_str(), "abstention");
    }

    #[test]
    fn test_question_category_from_str() {
        assert_eq!(
            "extraction".parse::<QuestionCategory>().ok(),
            Some(QuestionCategory::Extraction)
        );
        assert_eq!(
            "multi_session".parse::<QuestionCategory>().ok(),
            Some(QuestionCategory::MultiSession)
        );
        // Backward compatibility with "reasoning"
        assert_eq!(
            "REASONING".parse::<QuestionCategory>().ok(),
            Some(QuestionCategory::MultiSession)
        );
        assert!("invalid".parse::<QuestionCategory>().is_err());
    }

    #[test]
    fn test_question_category_display_name() {
        assert_eq!(QuestionCategory::Extraction.display_name(), "Extraction");
        assert_eq!(
            QuestionCategory::MultiSession.display_name(),
            "Multi-Session"
        );
        assert_eq!(
            QuestionCategory::Updates.display_name(),
            "Knowledge Updates"
        );
        assert_eq!(
            QuestionCategory::Temporal.display_name(),
            "Temporal Reasoning"
        );
        assert_eq!(QuestionCategory::Abstention.display_name(), "Abstention");
    }

    #[test]
    fn test_benchmark_config_default() {
        let config = BenchmarkConfig::default();
        assert_eq!(config.name, "default");
        assert_eq!(config.extraction_mode, "local-fast");
        assert!(config.use_llm_answers);
    }

    #[test]
    fn test_benchmark_config_builder() {
        let config = BenchmarkConfig::new("test")
            .with_extraction_mode("api")
            .with_max_questions(100)
            .with_seed(42);

        assert_eq!(config.name, "test");
        assert_eq!(config.extraction_mode, "api");
        assert_eq!(config.max_questions, 100);
        assert_eq!(config.seed, Some(42));
    }

    #[test]
    fn test_benchmark_config_includes_category() {
        let config = BenchmarkConfig::default();
        assert!(config.includes_category(QuestionCategory::Extraction));

        let config = BenchmarkConfig::default().with_categories(vec![
            QuestionCategory::Extraction,
            QuestionCategory::MultiSession,
        ]);
        assert!(config.includes_category(QuestionCategory::Extraction));
        assert!(!config.includes_category(QuestionCategory::Temporal));
    }

    #[test]
    fn test_benchmark_result_calculate_scores() {
        let config = BenchmarkConfig::default();
        let mut result = BenchmarkResult::new("test", config);

        result.add_result(QuestionResult {
            question_id: "q1".to_string(),
            category: QuestionCategory::Extraction,
            question: "Test?".to_string(),
            expected_answer: "Yes".to_string(),
            generated_answer: "Yes".to_string(),
            retrieved_memories: vec![],
            is_correct: true,
            judge_score: 1.0,
            judge_reasoning: "Correct".to_string(),
            retrieval_time_ms: 50,
            answer_time_ms: 100,
            total_time_ms: 150,
        });

        result.add_result(QuestionResult {
            question_id: "q2".to_string(),
            category: QuestionCategory::Extraction,
            question: "Test2?".to_string(),
            expected_answer: "No".to_string(),
            generated_answer: "Yes".to_string(),
            retrieved_memories: vec![],
            is_correct: false,
            judge_score: 0.0,
            judge_reasoning: "Incorrect".to_string(),
            retrieval_time_ms: 50,
            answer_time_ms: 100,
            total_time_ms: 150,
        });

        let result = result.calculate_scores();

        assert_eq!(result.total_questions, 2);
        assert_eq!(result.correct_count, 1);
        assert_eq!(result.accuracy, 0.5);
        assert_eq!(
            result.category_accuracy(QuestionCategory::Extraction),
            Some(0.5)
        );
    }

    #[test]
    fn test_benchmark_result_meets_target() {
        let config = BenchmarkConfig::default();
        let mut result = BenchmarkResult::new("test", config);
        result.accuracy = 0.92;

        assert!(result.meets_target(0.90));
        assert!(!result.meets_target(0.95));
    }

    #[test]
    fn test_benchmark_session() {
        let now = Utc::now();
        let later = now + chrono::Duration::hours(1);

        let session = BenchmarkSession::new("sess-1", "user-1")
            .with_message(BenchmarkMessage::user("Hello", now))
            .with_message(BenchmarkMessage::assistant("Hi there!", later));

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.earliest_timestamp(), Some(now));
        assert_eq!(session.latest_timestamp(), Some(later));
    }

    #[test]
    fn test_benchmark_message() {
        let now = Utc::now();

        let user_msg = BenchmarkMessage::user("Hello", now);
        assert_eq!(user_msg.role, "user");

        let assistant_msg = BenchmarkMessage::assistant("Hi", now);
        assert_eq!(assistant_msg.role, "assistant");

        let system_msg = BenchmarkMessage::system("System", now);
        assert_eq!(system_msg.role, "system");
    }

    #[test]
    fn test_benchmark_question() {
        let question =
            BenchmarkQuestion::new("q1", "What is 2+2?", "4", QuestionCategory::Extraction)
                .with_session_ids(vec!["sess-1".to_string()]);

        assert_eq!(question.id, "q1");
        assert_eq!(question.session_ids.len(), 1);
    }

    #[test]
    fn test_category_score_serialization() {
        let score = CategoryScore {
            category: QuestionCategory::Extraction,
            total: 10,
            correct: 8,
            accuracy: 0.8,
        };

        let json = serde_json::to_string(&score).unwrap();
        let parsed: CategoryScore = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.category, QuestionCategory::Extraction);
        assert_eq!(parsed.accuracy, 0.8);
    }

    #[test]
    fn test_retrieved_memory_info_with_session_context() {
        let context = SessionEntityContext::new()
            .with_primary_location("Target")
            .with_primary_organization("Google");

        let info = RetrievedMemoryInfo::new(Uuid::now_v7(), "test content", 0.8)
            .with_session_entity_context(context);

        assert_eq!(info.primary_location(), Some("Target"));
        assert_eq!(info.primary_organization(), Some("Google"));
    }

    #[test]
    fn test_retrieved_memory_info_without_session_context() {
        let info = RetrievedMemoryInfo::new(Uuid::now_v7(), "test content", 0.8);

        assert!(info.primary_location().is_none());
        assert!(info.primary_organization().is_none());
        assert!(info.session_entity_context.is_none());
    }

    #[test]
    fn test_retrieved_memory_info_serialization_with_context() {
        let context = SessionEntityContext::new().with_primary_location("Costco");

        let info = RetrievedMemoryInfo::new(Uuid::now_v7(), "test", 0.9)
            .with_session_entity_context(context);

        let json = serde_json::to_string(&info).unwrap();
        let parsed: RetrievedMemoryInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.primary_location(), Some("Costco"));
    }
}
