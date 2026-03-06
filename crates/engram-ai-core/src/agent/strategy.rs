//! Question strategy detection for category-specific prompts.

/// Question strategy for category-specific prompts.
///
/// Detected from question text using heuristic keyword matching.
/// Ordering: Update → Temporal → Enumeration → Preference → Default.
#[derive(Debug, Clone, PartialEq)]
pub enum QuestionStrategy {
    Enumeration,
    Update,
    Temporal,
    Preference,
    Default,
}

/// Detect question strategy from question text using heuristic keyword matching.
pub fn detect_question_strategy(question: &str) -> QuestionStrategy {
    let q = question.to_lowercase();

    // Update-first check: "current" signals Update when the question is about
    // the current STATE of something (duration, status), not when "current" is just
    // a modifier in a temporal sub-clause like "before I started my current job"
    let has_current = q.contains("current ") || q.contains("currently ");
    let current_is_incidental = q.contains("before my current")
        || q.contains("before i started my current")
        || q.contains("after my current")
        || q.contains("since my current")
        || q.contains("started my current")
        || q.contains("began my current");
    if has_current && !current_is_incidental {
        return QuestionStrategy::Update;
    }

    // Temporal patterns — checked BEFORE Enumeration because "how many days/weeks/months"
    // is temporal, not enumeration. This is the most important ordering decision.
    let temporal_patterns = [
        "when did",
        "when was",
        "first time",
        "last time",
        "what date",
        "what month",
        "how long ago",
        "since when",
        "how long have",
        "how long had",
        "how long was",
        "how long did",
        "days ago",
        "weeks ago",
        "months ago",
        "years ago",
        "had passed",
        "has passed",
        "have passed",
        "elapsed",
        "valentine",
        "on the day",
    ];
    if temporal_patterns.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Temporal;
    }
    // "how many days/weeks/months/years" is temporal ONLY when followed by temporal context
    let duration_units = [
        "how many days",
        "how many weeks",
        "how many months",
        "how many years",
    ];
    let temporal_context = ["ago", "since", "passed", "elapsed", "between", "until"];
    if duration_units.iter().any(|u| q.contains(u))
        && temporal_context.iter().any(|c| q.contains(c))
    {
        return QuestionStrategy::Temporal;
    }
    // "before" and "after" are temporal only as standalone temporal markers
    if q.contains("before i")
        || q.contains("after i")
        || q.contains("before my")
        || q.contains("after my")
    {
        return QuestionStrategy::Temporal;
    }
    // Match "in 20XX" year patterns
    if let Some(pos) = q.find("in 20") {
        if q.len() >= pos + 7 && q[pos + 5..pos + 7].chars().all(|c| c.is_ascii_digit()) {
            return QuestionStrategy::Temporal;
        }
    }

    // Enumeration patterns (after temporal to avoid "how many days" matching here)
    let enum_patterns = [
        "list all",
        "how many",
        "what are all",
        "name every",
        "enumerate",
        "what are the",
        "which ones",
        "tell me all",
    ];
    if enum_patterns.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Enumeration;
    }

    // Update patterns (note: "current" already handled above)
    let update_patterns = [
        "latest",
        "now ",
        "most recent",
        "changed",
        "updated",
        "still ",
        "switched",
        "moved to",
        "these days",
        "at the moment",
        "right now",
        "nowadays",
    ];
    if update_patterns.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Update;
    }
    // Mutable-state patterns: questions about things that commonly change over time
    let mutable_state = [
        "where do i live",
        "where am i living",
        "where do i work",
        "what is my address",
        "what is my job",
        "what is my salary",
        "what is my mortgage",
        "how much is my mortgage",
        "how much is my rent",
        "what is my personal best",
        "what is my best time",
        "what is my record",
        "where am i going",
        "what am i planning",
        "what was my personal best",
        "what was my best time",
        "what was my record",
        "pre-approved",
        "pre-approval",
        "where did i get",
    ];
    if mutable_state.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Update;
    }

    // Computation patterns — detect before Preference to avoid misrouting
    let is_yes_no = q.starts_with("did ")
        || q.starts_with("is ")
        || q.starts_with("was ")
        || q.starts_with("do ")
        || q.starts_with("does ")
        || q.starts_with("are ")
        || q.starts_with("has ")
        || q.starts_with("have ")
        || q.starts_with("can ")
        || q.starts_with("could ")
        || q.starts_with("will ")
        || q.starts_with("would ");
    let computation_patterns = [
        "percentage",
        "percent",
        "discount",
        "older than",
        "younger than",
        "taller than",
        "shorter than",
        "difference between",
        "how much more",
        "how much less",
    ];
    if !is_yes_no && computation_patterns.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Enumeration;
    }

    // Preference patterns (includes advice-seeking)
    let pref_patterns = [
        "favorite",
        "prefer",
        "like best",
        "recommend",
        "any tips",
        "any suggestions",
        "any ideas",
        "do you think",
        "what do you think",
        "what should i",
        "what would you suggest",
        "how can i",
        "how should i",
        "advice",
        "can you suggest",
        "suggestions for",
        "help me decide",
        "help me choose",
        "what can i do",
        "ideas on how",
        "can you remind me of",
        "follow up on our previous",
    ];
    if pref_patterns.iter().any(|p| q.contains(p)) {
        return QuestionStrategy::Preference;
    }

    QuestionStrategy::Default
}

/// Check if a question is asking for a count or enumeration.
pub fn is_counting_question(question: &str) -> bool {
    let q = question.to_lowercase();
    let count_patterns = [
        "how many",
        "how much",
        "list all",
        "what are all",
        "name every",
        "enumerate",
        "tell me all",
        "total number",
        "count of",
    ];
    count_patterns.iter().any(|p| q.contains(p))
}

/// Detect if a question is asking for a sum/total (not a difference or comparison).
pub fn is_sum_question(question: &str) -> bool {
    let q = question.to_lowercase();
    let has_sum_intent = [
        "total cost",
        "total price",
        "total amount",
        "total spending",
        "total expense",
        "in total",
        "altogether",
        "combined cost",
        "how much did i spend",
        "how much have i spent",
    ]
    .iter()
    .any(|p| q.contains(p));
    let is_difference = [
        "more than",
        "less than",
        "compared to",
        "difference",
        "how much more",
        "how much less",
    ]
    .iter()
    .any(|p| q.contains(p));
    has_sum_intent && !is_difference
}
