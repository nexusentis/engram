use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::extraction::{EntityExtractor, ExtractedEntity, TemporalParser};

/// Temporal intent of a query - classifies what temporal aspect the user is asking about
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalIntent {
    /// Asking about current state: "now", "currently", "still", "these days"
    CurrentState,

    /// Asking about past state: "used to", "previously", "before"
    PastState,

    /// Asking about a specific time period (has DateRange constraint)
    PointInTime,

    /// Asking about temporal ordering: "before", "after", "first", "last", "most recent"
    Ordering,

    /// No temporal markers detected - use default retrieval
    None,
}

impl TemporalIntent {
    /// Check if this intent requires is_latest filtering
    pub fn requires_is_latest_filter(&self) -> bool {
        matches!(self, Self::CurrentState)
    }

    /// Check if this intent should include historical (non-latest) data
    pub fn should_include_historical(&self) -> bool {
        matches!(self, Self::PastState | Self::PointInTime | Self::Ordering)
    }

    /// Check if this intent requires time range filtering
    pub fn requires_time_filter(&self) -> bool {
        matches!(self, Self::PointInTime)
    }
}

/// Analyzes queries for temporal intent patterns
struct TemporalIntentAnalyzer {
    current_patterns: Vec<&'static str>,
    past_patterns: Vec<&'static str>,
    ordering_patterns: Vec<&'static str>,
}

impl Default for TemporalIntentAnalyzer {
    fn default() -> Self {
        Self {
            current_patterns: vec![
                "current",
                "currently",
                "now",
                "still",
                "at the moment",
                "these days",
                "right now",
                "presently",
                "at present",
            ],
            past_patterns: vec![
                "used to",
                "previously",
                "previous",
                "before",
                "former",
                "formerly",
                "in the past",
                "back then",
                "at that time",
                "once",
                "was the",
            ],
            ordering_patterns: vec![
                "before",
                "after",
                "first",
                "last",
                "most recent",
                "earliest",
                "latest",
                "prior to",
                "following",
                "older",
                "newer",
                "previous",
                "next",
            ],
        }
    }
}

impl TemporalIntentAnalyzer {
    /// Detect temporal intent from query
    fn detect(&self, query: &str, has_temporal_constraints: bool) -> TemporalIntent {
        let query_lower = query.to_lowercase();

        // If we have explicit temporal constraints (dates, ranges), it's PointInTime
        if has_temporal_constraints {
            return TemporalIntent::PointInTime;
        }

        // Check for ordering patterns (before checking current/past since "before" could be ordering)
        if self.matches_ordering_context(&query_lower) {
            return TemporalIntent::Ordering;
        }

        // Check for current state markers
        if self.matches_patterns(&query_lower, &self.current_patterns) {
            return TemporalIntent::CurrentState;
        }

        // Check for past state markers
        if self.matches_patterns(&query_lower, &self.past_patterns) {
            return TemporalIntent::PastState;
        }

        TemporalIntent::None
    }

    fn matches_patterns(&self, query: &str, patterns: &[&str]) -> bool {
        patterns.iter().any(|p| query.contains(p))
    }

    /// Check if query is asking about ordering (not just containing "before/after")
    fn matches_ordering_context(&self, query: &str) -> bool {
        // Check for explicit ordering patterns first
        for pattern in &self.ordering_patterns {
            if query.contains(*pattern) {
                // "first time", "last time", "most recent", "earliest", "latest" are definite ordering
                if *pattern == "first"
                    || *pattern == "last"
                    || *pattern == "most recent"
                    || *pattern == "earliest"
                    || *pattern == "latest"
                {
                    return true;
                }
            }
        }

        // "before X" or "after X" in comparative context
        // But not "previously before" which is PastState
        if (query.contains("did") || query.contains("happen"))
            && (query.contains(" before ") || query.contains(" after "))
        {
            return true;
        }

        false
    }
}

/// Classification of query intent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    /// Simple factual lookup: "What is Alice's job?"
    Factual,
    /// Time-bounded query: "What did I do last week?"
    Temporal,
    /// User preference query: "What music do I like?"
    Preference,
    /// Relationship query: "Who does Alice work with?"
    Relational,
    /// Multi-hop query: "What project is Alice's manager working on?"
    MultiHop,
    /// Unknown or ambiguous
    Unknown,
}

impl QueryIntent {
    /// Get epistemic type collections to search for this intent.
    ///
    /// Returns collections in priority order - first collection is most likely to contain
    /// the answer, subsequent collections are fallbacks.
    pub fn epistemic_collections(&self) -> Vec<&'static str> {
        match self {
            // Factual queries → primarily world facts
            QueryIntent::Factual => vec!["world", "observation"],
            // Temporal queries → could be in any collection
            QueryIntent::Temporal => vec!["world", "experience", "opinion", "observation"],
            // Preference queries → primarily opinions
            QueryIntent::Preference => vec!["opinion", "world"],
            // Relational queries → world facts about relationships
            QueryIntent::Relational => vec!["world", "observation"],
            // Multi-hop → need all collections
            QueryIntent::MultiHop => vec!["world", "experience", "opinion", "observation"],
            // Unknown → search all collections
            QueryIntent::Unknown => vec!["world", "experience", "opinion", "observation"],
        }
    }

    /// Check if this intent should search all collections
    pub fn is_broad_search(&self) -> bool {
        matches!(
            self,
            QueryIntent::Unknown | QueryIntent::MultiHop | QueryIntent::Temporal
        )
    }
}

/// Temporal constraint extracted from query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalConstraint {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub expression: String,
    pub confidence: f32,
}

/// Complete analysis of a query
#[derive(Debug, Clone)]
pub struct QueryAnalysis {
    /// Original query text
    pub original_query: String,
    /// Cleaned/normalized query
    pub normalized_query: String,
    /// Classified intent
    pub intent: QueryIntent,
    /// Temporal intent for temporal retrieval strategy
    pub temporal_intent: TemporalIntent,
    /// Extracted entities
    pub entities: Vec<ExtractedEntity>,
    /// Temporal constraints
    pub temporal_constraints: Vec<TemporalConstraint>,
    /// Keywords for BM25 search
    pub keywords: Vec<String>,
    /// Whether query appears to be multi-hop
    pub is_multi_hop: bool,
    /// Suggested search channels to activate
    pub active_channels: SearchChannels,
}

impl QueryAnalysis {
    /// Check if query requires is_latest filtering
    pub fn requires_is_latest(&self) -> bool {
        self.temporal_intent.requires_is_latest_filter()
    }

    /// Check if query should include historical data
    pub fn should_include_historical(&self) -> bool {
        self.temporal_intent.should_include_historical()
    }

    /// Get time range constraint if present
    pub fn time_range(&self) -> Option<&TemporalConstraint> {
        self.temporal_constraints.first()
    }
}

/// Flags for which search channels to use
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchChannels {
    pub semantic: bool,
    pub keyword: bool,
    pub temporal: bool,
    pub entity: bool,
}

impl SearchChannels {
    /// All channels active
    pub fn all() -> Self {
        Self {
            semantic: true,
            keyword: true,
            temporal: true,
            entity: true,
        }
    }

    /// Only semantic search
    pub fn semantic_only() -> Self {
        Self {
            semantic: true,
            ..Default::default()
        }
    }

    /// Semantic and keyword search
    pub fn semantic_keyword() -> Self {
        Self {
            semantic: true,
            keyword: true,
            ..Default::default()
        }
    }

    /// Count active channels
    pub fn count(&self) -> usize {
        [self.semantic, self.keyword, self.temporal, self.entity]
            .iter()
            .filter(|&&b| b)
            .count()
    }
}

/// Query analyzer for pre-processing search queries
pub struct QueryAnalyzer {
    temporal_parser: TemporalParser,
    temporal_intent_analyzer: TemporalIntentAnalyzer,
    entity_extractor: EntityExtractor,
    stop_words: Vec<&'static str>,
}

impl QueryAnalyzer {
    pub fn new() -> Self {
        Self {
            temporal_parser: TemporalParser::new(),
            temporal_intent_analyzer: TemporalIntentAnalyzer::default(),
            entity_extractor: EntityExtractor::new(),
            stop_words: vec![
                "the", "a", "an", "is", "are", "was", "were", "what", "who", "where", "when",
                "how", "why", "do", "does", "did", "have", "has", "had", "can", "could", "would",
                "should", "will", "to", "of", "in", "for", "on", "with", "at", "by", "from", "my",
                "your", "their", "our", "i", "you", "he", "she", "it", "we", "they", "me", "him",
                "her", "us", "them", "that", "this", "these", "those", "be", "been", "being", "am",
            ],
        }
    }

    /// Analyze a query and extract all relevant information
    pub async fn analyze(&self, query: &str) -> QueryAnalysis {
        self.analyze_with_reference_time(query, Utc::now()).await
    }

    /// Analyze a query with a specific reference time (for benchmark replay)
    pub async fn analyze_with_reference_time(
        &self,
        query: &str,
        reference_time: DateTime<Utc>,
    ) -> QueryAnalysis {
        let normalized = self.normalize_query(query);
        let temporal_constraints = self.extract_temporal_constraints_at(query, reference_time);
        let entities = self.extract_entities(query).await;
        let keywords = self.extract_keywords(&normalized);
        let is_multi_hop = self.detect_multi_hop(query);
        let intent = self.classify_intent(query, &temporal_constraints, &entities, is_multi_hop);
        let active_channels = self.determine_channels(&intent, &temporal_constraints, &entities);

        // Detect temporal intent for filtering strategy
        let temporal_intent = self
            .temporal_intent_analyzer
            .detect(query, !temporal_constraints.is_empty());

        QueryAnalysis {
            original_query: query.to_string(),
            normalized_query: normalized,
            intent,
            temporal_intent,
            entities,
            temporal_constraints,
            keywords,
            is_multi_hop,
            active_channels,
        }
    }

    /// Normalize query text
    fn normalize_query(&self, query: &str) -> String {
        query.to_lowercase().trim().to_string()
    }

    /// Extract temporal constraints from query using a specific reference time
    fn extract_temporal_constraints_at(
        &self,
        query: &str,
        reference_time: DateTime<Utc>,
    ) -> Vec<TemporalConstraint> {
        self.temporal_parser
            .parse(query, reference_time)
            .into_iter()
            .map(|range| TemporalConstraint {
                start: Some(range.start),
                end: Some(range.end),
                expression: range.expression,
                confidence: range.confidence,
            })
            .collect()
    }

    /// Extract entities from query
    async fn extract_entities(&self, query: &str) -> Vec<ExtractedEntity> {
        self.entity_extractor
            .extract(query)
            .await
            .unwrap_or_default()
    }

    /// Extract keywords for BM25 search
    fn extract_keywords(&self, normalized_query: &str) -> Vec<String> {
        normalized_query
            .split_whitespace()
            .filter(|word| {
                let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
                !self.stop_words.contains(&clean) && clean.len() > 2
            })
            .map(|word| {
                word.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| !w.is_empty())
            .collect()
    }

    /// Classify query intent
    fn classify_intent(
        &self,
        query: &str,
        temporal: &[TemporalConstraint],
        entities: &[ExtractedEntity],
        is_multi_hop: bool,
    ) -> QueryIntent {
        let lower = query.to_lowercase();

        // Check for multi-hop first
        if is_multi_hop {
            return QueryIntent::MultiHop;
        }

        // Temporal queries
        if !temporal.is_empty()
            || lower.contains("when")
            || lower.contains("last")
            || lower.contains("yesterday")
            || lower.contains("recently")
            || lower.contains("ago")
        {
            return QueryIntent::Temporal;
        }

        // Preference queries
        if lower.contains("like")
            || lower.contains("prefer")
            || lower.contains("favorite")
            || lower.contains("favourite")
            || lower.contains("enjoy")
            || lower.contains("love")
        {
            return QueryIntent::Preference;
        }

        // Relational queries
        if lower.contains("work with")
            || lower.contains("works with")
            || lower.contains("knows")
            || lower.contains("know")
            || lower.contains("related to")
            || lower.contains("connected to")
            || lower.contains("team")
            || lower.contains("colleague")
            || (lower.contains("who") && !entities.is_empty())
        {
            return QueryIntent::Relational;
        }

        // Default to factual if entities present or starts with question word
        if !entities.is_empty()
            || lower.starts_with("what")
            || lower.starts_with("where")
            || lower.starts_with("which")
        {
            QueryIntent::Factual
        } else {
            QueryIntent::Unknown
        }
    }

    /// Detect multi-hop queries
    fn detect_multi_hop(&self, query: &str) -> bool {
        let lower = query.to_lowercase();

        // Possessive chains: "Alice's manager's project"
        let possessive_count = query.matches("'s").count();
        if possessive_count >= 2 {
            return true;
        }

        // "of" chains: "project of manager of Alice"
        let of_count = lower.matches(" of ").count();
        if of_count >= 2 {
            return true;
        }

        // Combined possessive and "of"
        if possessive_count >= 1 && of_count >= 1 {
            return true;
        }

        // Explicit multi-hop patterns
        let multi_hop_patterns = [
            "manager of",
            "boss of",
            "team of",
            "company of",
            "department of",
            "project of",
            "who does",
            "who is",
            "where does",
        ];

        // Check for pattern + possessive combination
        for pattern in &multi_hop_patterns {
            if lower.contains(pattern) && possessive_count >= 1 {
                return true;
            }
        }

        false
    }

    /// Determine which search channels to activate
    fn determine_channels(
        &self,
        intent: &QueryIntent,
        temporal: &[TemporalConstraint],
        entities: &[ExtractedEntity],
    ) -> SearchChannels {
        match intent {
            QueryIntent::Temporal => SearchChannels {
                semantic: true,
                keyword: true,
                temporal: true,
                entity: !entities.is_empty(),
            },
            QueryIntent::Relational | QueryIntent::MultiHop => SearchChannels {
                semantic: true,
                keyword: true,
                temporal: !temporal.is_empty(),
                entity: true,
            },
            QueryIntent::Preference => SearchChannels {
                semantic: true,
                keyword: true,
                temporal: false,
                entity: !entities.is_empty(),
            },
            QueryIntent::Factual => SearchChannels {
                semantic: true,
                keyword: true,
                temporal: !temporal.is_empty(),
                entity: !entities.is_empty(),
            },
            QueryIntent::Unknown => SearchChannels::all(),
        }
    }
}

impl Default for QueryAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_channels_all() {
        let channels = SearchChannels::all();
        assert!(channels.semantic);
        assert!(channels.keyword);
        assert!(channels.temporal);
        assert!(channels.entity);
        assert_eq!(channels.count(), 4);
    }

    #[test]
    fn test_search_channels_semantic_only() {
        let channels = SearchChannels::semantic_only();
        assert!(channels.semantic);
        assert!(!channels.keyword);
        assert_eq!(channels.count(), 1);
    }

    #[test]
    fn test_search_channels_default() {
        let channels = SearchChannels::default();
        assert_eq!(channels.count(), 0);
    }

    #[tokio::test]
    async fn test_factual_query() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What is Alice's job?").await;

        assert_eq!(analysis.intent, QueryIntent::Factual);
        assert!(analysis.active_channels.semantic);
    }

    #[tokio::test]
    async fn test_temporal_query() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What did I do last week?").await;

        assert_eq!(analysis.intent, QueryIntent::Temporal);
        assert!(!analysis.temporal_constraints.is_empty());
        assert!(analysis.active_channels.temporal);
    }

    #[tokio::test]
    async fn test_temporal_query_yesterday() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What happened yesterday?").await;

        assert_eq!(analysis.intent, QueryIntent::Temporal);
        assert!(analysis.active_channels.temporal);
    }

    #[tokio::test]
    async fn test_preference_query() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What music do I like?").await;

        assert_eq!(analysis.intent, QueryIntent::Preference);
        assert!(analysis.keywords.contains(&"music".to_string()));
    }

    #[tokio::test]
    async fn test_preference_query_favorite() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What is my favorite food?").await;

        assert_eq!(analysis.intent, QueryIntent::Preference);
    }

    #[tokio::test]
    async fn test_relational_query() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("Who does Alice work with?").await;

        assert_eq!(analysis.intent, QueryIntent::Relational);
        assert!(analysis.active_channels.entity);
    }

    #[tokio::test]
    async fn test_multi_hop_detection_possessive_chain() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer
            .analyze("What project is Alice's manager's team working on?")
            .await;

        assert!(analysis.is_multi_hop);
        assert_eq!(analysis.intent, QueryIntent::MultiHop);
    }

    #[tokio::test]
    async fn test_multi_hop_detection_of_chain() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer
            .analyze("What is the project of the manager of Alice?")
            .await;

        assert!(analysis.is_multi_hop);
    }

    #[tokio::test]
    async fn test_not_multi_hop_simple() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("What is Alice's job?").await;

        assert!(!analysis.is_multi_hop);
    }

    #[tokio::test]
    async fn test_keyword_extraction() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer
            .analyze("What programming languages does Bob know?")
            .await;

        assert!(analysis.keywords.contains(&"programming".to_string()));
        assert!(analysis.keywords.contains(&"languages".to_string()));
        // Stop words excluded
        assert!(!analysis.keywords.contains(&"what".to_string()));
        assert!(!analysis.keywords.contains(&"does".to_string()));
    }

    #[tokio::test]
    async fn test_keyword_extraction_filters_short_words() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("Is it ok to go?").await;

        // Short words like "is", "it", "ok", "to", "go" filtered
        assert!(analysis.keywords.is_empty() || analysis.keywords.iter().all(|k| k.len() > 2));
    }

    #[tokio::test]
    async fn test_unknown_intent() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("hello there").await;

        assert_eq!(analysis.intent, QueryIntent::Unknown);
        // Unknown should activate all channels
        assert!(analysis.active_channels.semantic);
        assert!(analysis.active_channels.keyword);
    }

    #[tokio::test]
    async fn test_normalized_query() {
        let analyzer = QueryAnalyzer::new();
        let analysis = analyzer.analyze("  WHAT Is Alice's JOB?  ").await;

        assert_eq!(analysis.normalized_query, "what is alice's job?");
    }

    #[test]
    fn test_query_intent_serialization() {
        let intent = QueryIntent::Temporal;
        let json = serde_json::to_string(&intent).unwrap();
        assert_eq!(json, "\"temporal\"");

        let parsed: QueryIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, QueryIntent::Temporal);
    }

    #[test]
    fn test_temporal_constraint_serialization() {
        let constraint = TemporalConstraint {
            start: Some(Utc::now()),
            end: Some(Utc::now()),
            expression: "yesterday".to_string(),
            confidence: 0.95,
        };

        let json = serde_json::to_string(&constraint).unwrap();
        assert!(json.contains("yesterday"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_query_intent_epistemic_collections_factual() {
        let collections = QueryIntent::Factual.epistemic_collections();
        assert_eq!(collections[0], "world");
        assert!(collections.contains(&"world"));
        assert!(!collections.contains(&"experience"));
    }

    #[test]
    fn test_query_intent_epistemic_collections_preference() {
        let collections = QueryIntent::Preference.epistemic_collections();
        assert_eq!(collections[0], "opinion");
        assert!(collections.contains(&"opinion"));
        assert!(collections.contains(&"world")); // fallback
    }

    #[test]
    fn test_query_intent_epistemic_collections_unknown() {
        let collections = QueryIntent::Unknown.epistemic_collections();
        assert_eq!(collections.len(), 4);
        assert!(collections.contains(&"world"));
        assert!(collections.contains(&"experience"));
        assert!(collections.contains(&"opinion"));
        assert!(collections.contains(&"observation"));
    }

    #[test]
    fn test_query_intent_is_broad_search() {
        assert!(!QueryIntent::Factual.is_broad_search());
        assert!(!QueryIntent::Preference.is_broad_search());
        assert!(QueryIntent::Unknown.is_broad_search());
        assert!(QueryIntent::Temporal.is_broad_search());
        assert!(QueryIntent::MultiHop.is_broad_search());
    }

    // TemporalIntent tests
    #[test]
    fn test_temporal_intent_current_state() {
        let analyzer = TemporalIntentAnalyzer::default();

        assert_eq!(
            analyzer.detect("What is user's current job?", false),
            TemporalIntent::CurrentState
        );
        assert_eq!(
            analyzer.detect("Where does Alice live now?", false),
            TemporalIntent::CurrentState
        );
        assert_eq!(
            analyzer.detect("Is the user still working at Google?", false),
            TemporalIntent::CurrentState
        );
        assert_eq!(
            analyzer.detect("What is my phone number currently?", false),
            TemporalIntent::CurrentState
        );
    }

    #[test]
    fn test_temporal_intent_past_state() {
        let analyzer = TemporalIntentAnalyzer::default();

        assert_eq!(
            analyzer.detect("Where did user used to work?", false),
            TemporalIntent::PastState
        );
        assert_eq!(
            analyzer.detect("What was the user's previous job?", false),
            TemporalIntent::PastState
        );
        assert_eq!(
            analyzer.detect("Where did she live formerly?", false),
            TemporalIntent::PastState
        );
    }

    #[test]
    fn test_temporal_intent_ordering() {
        let analyzer = TemporalIntentAnalyzer::default();

        assert_eq!(
            analyzer.detect("What was my most recent purchase?", false),
            TemporalIntent::Ordering
        );
        assert_eq!(
            analyzer.detect("When did X happen for the first time?", false),
            TemporalIntent::Ordering
        );
        assert_eq!(
            analyzer.detect("What was my earliest job?", false),
            TemporalIntent::Ordering
        );
    }

    #[test]
    fn test_temporal_intent_point_in_time() {
        let analyzer = TemporalIntentAnalyzer::default();

        // When temporal constraints are present, it's PointInTime
        assert_eq!(
            analyzer.detect("What did I buy last week?", true),
            TemporalIntent::PointInTime
        );
        assert_eq!(
            analyzer.detect("What happened in 2024?", true),
            TemporalIntent::PointInTime
        );
    }

    #[test]
    fn test_temporal_intent_none() {
        let analyzer = TemporalIntentAnalyzer::default();

        assert_eq!(
            analyzer.detect("Tell me about Paris", false),
            TemporalIntent::None
        );
        assert_eq!(
            analyzer.detect("What is Alice's favorite food?", false),
            TemporalIntent::None
        );
    }

    #[test]
    fn test_temporal_intent_requires_is_latest() {
        assert!(TemporalIntent::CurrentState.requires_is_latest_filter());
        assert!(!TemporalIntent::PastState.requires_is_latest_filter());
        assert!(!TemporalIntent::PointInTime.requires_is_latest_filter());
        assert!(!TemporalIntent::Ordering.requires_is_latest_filter());
        assert!(!TemporalIntent::None.requires_is_latest_filter());
    }

    #[test]
    fn test_temporal_intent_should_include_historical() {
        assert!(!TemporalIntent::CurrentState.should_include_historical());
        assert!(TemporalIntent::PastState.should_include_historical());
        assert!(TemporalIntent::PointInTime.should_include_historical());
        assert!(TemporalIntent::Ordering.should_include_historical());
        assert!(!TemporalIntent::None.should_include_historical());
    }

    #[tokio::test]
    async fn test_query_analysis_temporal_intent() {
        let analyzer = QueryAnalyzer::new();

        let analysis = analyzer.analyze("What is user's current job?").await;
        assert_eq!(analysis.temporal_intent, TemporalIntent::CurrentState);
        assert!(analysis.requires_is_latest());

        let analysis = analyzer.analyze("Where did user used to work?").await;
        assert_eq!(analysis.temporal_intent, TemporalIntent::PastState);
        assert!(analysis.should_include_historical());
    }

    #[tokio::test]
    async fn test_query_analysis_with_time_range() {
        let analyzer = QueryAnalyzer::new();

        let analysis = analyzer.analyze("What did I buy last week?").await;
        // Has temporal constraints, so it should be PointInTime
        assert_eq!(analysis.temporal_intent, TemporalIntent::PointInTime);
        assert!(analysis.time_range().is_some());
    }

    #[test]
    fn test_temporal_intent_serialization() {
        let intent = TemporalIntent::CurrentState;
        let json = serde_json::to_string(&intent).unwrap();
        assert_eq!(json, "\"CurrentState\"");

        let parsed: TemporalIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TemporalIntent::CurrentState);
    }
}
