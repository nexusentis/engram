//! Tests for the answerer module.

use super::*;
use engram::agent::{build_agent_system_prompt, strategy_guidance, is_counting_question, is_sum_question};
use super::reduction::extract_dollar_amounts;
use crate::QuestionCategory;
use engram::retrieval::AbstentionConfig;

fn create_test_question() -> BenchmarkQuestion {
    BenchmarkQuestion::new(
        "q1",
        "What is the user's name?",
        "John",
        QuestionCategory::Extraction,
    )
}

/// Detect if a temporal question asks for an interval between two events.
/// These require date_diff for correct computation.
/// Excludes "direct duration lookup" questions like "how long has he been working?"
/// which can be answered by extracting a stated duration from text.
fn is_interval_between_events(question: &str) -> bool {
    let q = question.to_lowercase();

    // Patterns that indicate interval computation between two events
    let interval_patterns = [
        "how many days",
        "how many weeks",
        "how many months",
        "how many years",
        "how long between",
        "how long from",
        "had passed",
        "has passed",
        "have passed",
        "elapsed",
    ];
    let has_interval = interval_patterns.iter().any(|p| q.contains(p));
    if !has_interval {
        return false;
    }

    // Check for "between" or dual-event markers (A when B, A before B, A after B, since A)
    let dual_event_markers = [
        "between", "when i", "when my", "before i", "before my",
        "after i", "after my", "since i", "since my", "ago did",
    ];
    if dual_event_markers.iter().any(|m| q.contains(m)) {
        return true;
    }

    // Note: "how many X ago" (single event to today) is NOT a dual-event interval.
    // The gate is now one-shot, so broad matching here would waste the single fire.
    // Only the dual_event_markers above should trigger the hard gate.

    false
}

/// Detect the expected temporal unit from a question
fn detect_temporal_unit(question: &str) -> &'static str {
    let q = question.to_lowercase();
    if q.contains("week") {
        "weeks"
    } else if q.contains("month") {
        "months"
    } else if q.contains("year") {
        "years"
    } else {
        "days"
    }
}

/// Extract a numeric value from a date_diff tool result string.
/// date_diff returns: "2023/04/10 is 81 days after 2023/01/19 (start: ..., end: ...)"
/// We need to extract the number AFTER "is " — not the date prefix.
fn extract_number_from_date_diff(result: &str) -> Option<i64> {
    // Find " is " marker — everything after it starts with the numeric result
    if let Some(is_pos) = result.find(" is ") {
        let after_is = &result[is_pos + 4..]; // skip " is "
        let mut num_str = String::new();
        let mut found_start = false;
        for ch in after_is.chars() {
            if ch.is_ascii_digit() || (ch == '-' && !found_start) {
                num_str.push(ch);
                found_start = true;
            } else if found_start {
                break;
            }
        }
        return num_str.parse().ok();
    }
    // Fallback: try to find "N days/weeks/months/years" pattern anywhere
    for word in result.split_whitespace() {
        if let Ok(n) = word.parse::<i64>() {
            // Check next word is a unit
            let rest = &result[result.find(word).unwrap_or(0) + word.len()..];
            let next = rest.trim_start().split_whitespace().next().unwrap_or("");
            if ["days", "day", "weeks", "week", "months", "month", "years", "year"]
                .contains(&next)
            {
                return Some(n);
            }
        }
    }
    None
}

/// Extract the first number from a free-text answer
fn extract_number_from_answer(answer: &str) -> Option<i64> {
    // Skip common prefixes like "approximately", "about", "around"
    let mut num_str = String::new();
    let mut found_start = false;
    for ch in answer.chars() {
        if ch.is_ascii_digit() {
            num_str.push(ch);
            found_start = true;
        } else if found_start {
            break;
        }
    }
    num_str.parse().ok()
}

/// Scan agent message history for the last successful date_diff tool result
fn find_last_date_diff_result(messages: &[serde_json::Value]) -> Option<String> {
    // Walk backwards through messages to find tool results for date_diff calls
    let mut date_diff_call_ids: Vec<String> = Vec::new();

    // First pass: find date_diff tool call IDs
    for msg in messages {
        if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tool_calls {
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                if name == "date_diff" {
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                        date_diff_call_ids.push(id.to_string());
                    }
                }
            }
        }
    }

    // Second pass: find the tool result for the LAST date_diff call
    // Only return successful results (contain " is " marker), not rejections
    if let Some(last_id) = date_diff_call_ids.last() {
        for msg in messages.iter().rev() {
            if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
                if let Some(call_id) = msg.get("tool_call_id").and_then(|i| i.as_str()) {
                    if call_id == last_id {
                        if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                            // Successful date_diff output contains " is " and date patterns
                            // Rejection messages start with "REJECTED:" or "You already"
                            if content.contains(" is ") && !content.starts_with("REJECTED") {
                                return Some(content.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[test]
fn test_answerer_config_default() {
    let config = AnswererConfig::default();
    assert_eq!(config.answer_model, "gpt-4o");
    assert_eq!(config.max_tokens, 500);
    assert_eq!(config.temperature, 0.0);
    assert!(config.use_llm);
}

#[test]
fn test_answerer_config_builder() {
    let config = AnswererConfig::new()
        .with_model("gpt-4o-mini")
        .with_max_tokens(1000)
        .with_temperature(0.5)
        .with_use_llm(false);

    assert_eq!(config.answer_model, "gpt-4o-mini");
    assert_eq!(config.max_tokens, 1000);
    assert_eq!(config.temperature, 0.5);
    assert!(!config.use_llm);
}

#[test]
fn test_generator_reranking_always_disabled() {
    let config = AnswererConfig::default();
    let generator = AnswerGenerator::new(config);
    assert!(!generator.is_reranking_enabled());
}

#[test]
fn test_retrieved_memory_info_effective_score() {
    // Without reranker score
    let info = RetrievedMemoryInfo::new(uuid::Uuid::now_v7(), "test content", 0.8);
    assert_eq!(info.effective_score(), 0.8);
    assert!(info.reranker_score.is_none());

    // With reranker score
    let info = info.with_reranker_score(0.95);
    assert_eq!(info.effective_score(), 0.95);
    assert_eq!(info.reranker_score, Some(0.95));
}

#[test]
fn test_answer_result_builder() {
    let result = AnswerResult::new("Test answer")
        .with_retrieval_time(100)
        .with_answer_time(200)
        .with_total_time(300)
        .with_cost(0.05);

    assert_eq!(result.answer, "Test answer");
    assert_eq!(result.retrieval_time_ms, 100);
    assert_eq!(result.answer_time_ms, 200);
    assert_eq!(result.total_time_ms, 300);
    assert!((result.cost_usd - 0.05).abs() < 0.001);
    assert!(!result.abstained);
}

#[test]
fn test_answer_result_abstention() {
    let result = AnswerResult::abstention();
    assert!(result.abstained);
    assert!(result.abstention_reason.is_some());
    // Default abstention uses InsufficientResults reason
    assert_eq!(
        result.abstention_reason,
        Some(AbstentionReason::InsufficientResults)
    );
}

#[test]
fn test_answer_result_abstention_with_reason() {
    let result = AnswerResult::abstention_with_reason(AbstentionReason::NoRelevantMemories);
    assert!(result.abstained);
    assert_eq!(
        result.abstention_reason,
        Some(AbstentionReason::NoRelevantMemories)
    );
    assert!(!result.answer.is_empty());
}

#[test]
fn test_answerer_config_abstention() {
    let config = AnswererConfig::new()
        .with_abstention(true)
        .with_abstention_config(AbstentionConfig::default());

    assert!(config.enable_abstention);
}

#[test]
fn test_build_answer_prompt() {
    let prompt = AnswerGenerator::build_answer_prompt(
        "What is 2+2?",
        "Math facts: 2+2=4",
        &TemporalIntent::None,
        None,
        &QuestionStrategy::Default,
    );
    assert!(prompt.contains("What is 2+2?"));
    assert!(prompt.contains("Math facts: 2+2=4"));
    assert!(prompt.contains("Question:"));
    assert!(prompt.contains("Answer:"));
}

#[test]
fn test_answer_generator() {
    let generator = AnswerGenerator::with_defaults();
    let question = create_test_question();

    let result = generator.answer(&question, "user-1");
    assert!(result.is_ok());

    let answer = result.unwrap();
    assert!(!answer.answer.is_empty());
    assert!(!answer.abstained);
}

#[test]
fn test_estimate_cost() {
    let cost = estimate_cost("gpt-4o", 1000, 500);
    assert!(cost > 0.0);

    // GPT-4o: $2.50 per 1M prompt, $10 per 1M completion
    // 1000 prompt = $0.0025, 500 completion = $0.005
    // Total = $0.0075
    let expected = (1000.0 / 1_000_000.0) * 2.50 + (500.0 / 1_000_000.0) * 10.00;
    assert!((cost - expected).abs() < 0.0001);
}

#[test]
fn test_estimate_cost_different_models() {
    let gpt4o = estimate_cost("gpt-4o", 1000, 1000);
    let gpt4o_mini = estimate_cost("gpt-4o-mini", 1000, 1000);
    let gpt4_turbo = estimate_cost("gpt-4-turbo", 1000, 1000);

    // GPT-4o-mini should be cheapest
    assert!(gpt4o_mini < gpt4o);
    // GPT-4-turbo should be most expensive
    assert!(gpt4_turbo > gpt4o);
}

#[test]
fn test_llm_client_no_key() {
    let client = LlmClient::new("").unwrap();
    assert!(!client.has_api_key());

    // complete_sync should fail with no API key
    let result = client.complete_sync("gpt-4o", "test", 0.0);
    assert!(result.is_err());
}

#[test]
fn test_llm_client_with_key() {
    let client = LlmClient::new("test-key").unwrap();
    assert!(client.has_api_key());
    // Note: complete_sync with invalid key will fail when making actual API call
    // This just tests that the client is created with a key
}

#[test]
fn test_extract_search_keywords() {
    let kw =
        AnswerGenerator::extract_search_keywords("What is the user's favorite restaurant?");
    assert!(kw.contains(&"favorite".to_string()));
    assert!(kw.contains(&"restaurant".to_string()));
    // Stopwords should be excluded
    assert!(!kw.contains(&"what".to_string()));
    assert!(!kw.contains(&"the".to_string()));
    assert!(!kw.contains(&"is".to_string()));
}

#[test]
fn test_extract_search_keywords_names() {
    let kw =
        AnswerGenerator::extract_search_keywords("What did John say about his trip to Paris?");
    assert!(kw.contains(&"john".to_string()));
    assert!(kw.contains(&"trip".to_string()));
    assert!(kw.contains(&"paris".to_string()));
}

#[test]
fn test_extract_search_keywords_empty() {
    let kw = AnswerGenerator::extract_search_keywords("is the a");
    assert!(kw.is_empty());
}

#[test]
fn test_agent_system_prompt_includes_question() {
    let prompt = build_agent_system_prompt("What is Bob's job?", None);
    assert!(prompt.contains("What is Bob's job?"));
    assert!(prompt.contains("QUESTION:"));
    assert!(prompt.contains("personal memory assistant"));
    assert!(!prompt.contains("Today's date"));
}

#[test]
fn test_agent_system_prompt_with_date() {
    use chrono::TimeZone;
    let date = chrono::Utc.with_ymd_and_hms(2024, 6, 15, 0, 0, 0).unwrap();
    let prompt = build_agent_system_prompt("test?", Some(date));
    assert!(prompt.contains("Today's date: 2024/06/15"));
}

#[test]
fn test_answerer_config_agentic() {
    let config = AnswererConfig::new()
        .with_agentic(true)
        .with_max_iterations(15);
    assert!(config.agentic);
    assert_eq!(config.max_iterations, Some(15));
}

#[test]
fn test_answerer_config_agentic_default() {
    let config = AnswererConfig::default();
    assert!(!config.agentic);
    assert_eq!(config.max_iterations, None);
}

#[test]
fn test_detect_enumeration() {
    assert_eq!(
        detect_question_strategy("List all the restaurants I mentioned"),
        QuestionStrategy::Enumeration
    );
    assert_eq!(
        detect_question_strategy("How many books did I read?"),
        QuestionStrategy::Enumeration
    );
    assert_eq!(
        detect_question_strategy("What are all the places I visited?"),
        QuestionStrategy::Enumeration
    );
}

#[test]
fn test_detect_update() {
    assert_eq!(
        detect_question_strategy("What is my current job?"),
        QuestionStrategy::Update
    );
    assert_eq!(
        detect_question_strategy("What is my latest phone?"),
        QuestionStrategy::Update
    );
    assert_eq!(
        detect_question_strategy("Has my address changed?"),
        QuestionStrategy::Update
    );
    // Mutable-state patterns
    assert_eq!(
        detect_question_strategy("Where do I live?"),
        QuestionStrategy::Update
    );
    assert_eq!(
        detect_question_strategy("What is my mortgage?"),
        QuestionStrategy::Update
    );
    assert_eq!(
        detect_question_strategy("What is my personal best for the 5K?"),
        QuestionStrategy::Update
    );
    // P-NEW-C: "where did i get" is mutable (service locations change)
    assert_eq!(
        detect_question_strategy("Where did I get my guitar serviced?"),
        QuestionStrategy::Update
    );
}

#[test]
fn test_detect_temporal() {
    assert_eq!(
        detect_question_strategy("When did I visit Paris?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("What happened in 2023?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("What was the first time I went hiking?"),
        QuestionStrategy::Temporal
    );
    // "How many days/weeks" should be Temporal, NOT Enumeration
    assert_eq!(
        detect_question_strategy("How many days ago did I attend a baking class?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("How many weeks had passed since I recovered from the flu?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("How long have I been working before I started at NovaTech?"),
        QuestionStrategy::Temporal
    );
}

#[test]
fn test_detect_preference() {
    assert_eq!(
        detect_question_strategy("What is my favorite color?"),
        QuestionStrategy::Preference
    );
    assert_eq!(
        detect_question_strategy("What do I prefer for breakfast?"),
        QuestionStrategy::Preference
    );
}

#[test]
fn test_detect_default() {
    assert_eq!(
        detect_question_strategy("What is Bob's job?"),
        QuestionStrategy::Default
    );
    assert_eq!(
        detect_question_strategy("Where does Alice live?"),
        QuestionStrategy::Default
    );
}

#[test]
fn test_strategy_guidance_not_empty() {
    assert!(!strategy_guidance(&QuestionStrategy::Enumeration).is_empty());
    assert!(!strategy_guidance(&QuestionStrategy::Update).is_empty());
    assert!(!strategy_guidance(&QuestionStrategy::Temporal).is_empty());
    assert!(!strategy_guidance(&QuestionStrategy::Preference).is_empty());
    assert!(strategy_guidance(&QuestionStrategy::Default).is_empty());
}

#[test]
fn test_is_interval_between_events() {
    // Interval-between: should require date_diff
    assert!(is_interval_between_events(
        "How many weeks had passed since I recovered from the flu when I went on my 10th jog outdoors?"
    ));
    assert!(is_interval_between_events(
        "How many days ago did I attend a baking class when I made my friend's birthday cake?"
    ));
    assert!(is_interval_between_events(
        "How many days ago did I launch my website when I signed a contract with my first client?"
    ));
    assert!(is_interval_between_events(
        "How many months elapsed between my first and second trip to Japan?"
    ));
    assert!(is_interval_between_events(
        "How long between my first and second trip to Japan?"
    ));
    assert!(is_interval_between_events(
        "How many days had passed since I started my new job?"
    ));

    // Direct duration lookup: should NOT require date_diff
    assert!(!is_interval_between_events(
        "How long have I been working at my current company?"
    ));
    assert!(!is_interval_between_events(
        "What was the airline that I flew with on Valentine's day?"
    ));
    assert!(!is_interval_between_events(
        "When did I first visit Paris?"
    ));
    assert!(!is_interval_between_events(
        "What is my current job title?"
    ));
}

#[test]
fn test_detect_temporal_unit() {
    assert_eq!(detect_temporal_unit("How many weeks had passed?"), "weeks");
    assert_eq!(detect_temporal_unit("How many days ago?"), "days");
    assert_eq!(
        detect_temporal_unit("How many months between trips?"),
        "months"
    );
    assert_eq!(
        detect_temporal_unit("How many years have I worked?"),
        "years"
    );
    assert_eq!(
        detect_temporal_unit("How long had it been?"),
        "days" // default
    );
}

#[test]
fn test_extract_number_from_date_diff() {
    // Real date_diff output format: "END is N unit DIRECTION START (start: ..., end: ...)"
    assert_eq!(
        extract_number_from_date_diff(
            "2023/04/10 is 81 days after 2023/01/19 (start: 2023/01/19, end: 2023/04/10)"
        ),
        Some(81)
    );
    assert_eq!(
        extract_number_from_date_diff(
            "2023/04/10 is 11 weeks and 4 days (81 days total) after 2023/01/19 (start: 2023/01/19, end: 2023/04/10)"
        ),
        Some(11)
    );
    assert_eq!(
        extract_number_from_date_diff(
            "2023/06/20 is 3 months after 2023/03/20 (start: 2023/03/20, end: 2023/06/20)"
        ),
        Some(3)
    );
    // Fallback: simple format without date prefix
    assert_eq!(extract_number_from_date_diff("81 days"), Some(81));
    assert_eq!(extract_number_from_date_diff("no result"), None);
    // Should NOT extract 2023 from date prefix
    assert_ne!(
        extract_number_from_date_diff(
            "2023/04/10 is 81 days after 2023/01/19"
        ),
        Some(2023)
    );
}

#[test]
fn test_extract_number_from_answer() {
    assert_eq!(extract_number_from_answer("12 weeks"), Some(12));
    assert_eq!(
        extract_number_from_answer("approximately 15 weeks"),
        Some(15)
    );
    assert_eq!(extract_number_from_answer("5 days"), Some(5));
    assert_eq!(
        extract_number_from_answer("It was about 20 days."),
        Some(20)
    );
    assert_eq!(
        extract_number_from_answer("I don't have enough information"),
        None
    );
}

#[test]
fn test_find_last_date_diff_result() {
    let messages = vec![
        serde_json::json!({"role": "system", "content": "You are a helper"}),
        serde_json::json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "date_diff", "arguments": "{}"}
            }]
        }),
        serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "2023/07/15 is 81 days after 2023/04/25 (start: 2023/04/25, end: 2023/07/15)"
        }),
    ];
    assert_eq!(
        find_last_date_diff_result(&messages),
        Some("2023/07/15 is 81 days after 2023/04/25 (start: 2023/04/25, end: 2023/07/15)".to_string())
    );

    // No date_diff calls
    let no_dd = vec![serde_json::json!({"role": "system", "content": "test"})];
    assert_eq!(find_last_date_diff_result(&no_dd), None);

    // Rejected date_diff call should NOT be returned
    let rejected = vec![
        serde_json::json!({"role": "system", "content": "You are a helper"}),
        serde_json::json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_2",
                "type": "function",
                "function": {"name": "date_diff", "arguments": "{}"}
            }]
        }),
        serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_2",
            "content": "REJECTED: You must search for the exact dates first"
        }),
    ];
    assert_eq!(find_last_date_diff_result(&rejected), None);

    // Successful result with real format
    let real_format = vec![
        serde_json::json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_3",
                "type": "function",
                "function": {"name": "date_diff", "arguments": "{}"}
            }]
        }),
        serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_3",
            "content": "2023/04/10 is 81 days after 2023/01/19 (start: 2023/01/19, end: 2023/04/10)"
        }),
    ];
    assert_eq!(
        find_last_date_diff_result(&real_format),
        Some("2023/04/10 is 81 days after 2023/01/19 (start: 2023/01/19, end: 2023/04/10)".to_string())
    );
}

#[test]
fn test_detect_temporal_new_patterns() {
    // New patterns added in Phase 1
    assert_eq!(
        detect_question_strategy("How long had I been living there?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("How long was the trip?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("How long did the project take?"),
        QuestionStrategy::Temporal
    );
    assert_eq!(
        detect_question_strategy("How many years ago did I graduate?"),
        QuestionStrategy::Temporal
    );
    // Diagnostic Q36: should be Temporal, not Update
    assert_eq!(
        detect_question_strategy(
            "How long have I been working before I started my current job at NovaTech?"
        ),
        QuestionStrategy::Temporal
    );
}

// ── Phase 2: Evidence Aggregation Engine tests ──

#[test]
fn test_extract_enumerated_items() {
    let answer = "3 books. Items:\n1) The Great Gatsby (from session on 2024-01-01)\n2) 1984 (from session on 2024-02-01)\n3) Brave New World (from session on 2024-03-01)";
    let items = reduction::extract_enumerated_items_for_test(answer);
    assert_eq!(items.len(), 3);
    assert!(items[0].contains("The Great Gatsby"));
    assert!(items[1].contains("1984"));
    assert!(items[2].contains("Brave New World"));

    // "N. item" format
    let answer2 = "2 pets.\n1. Golden Retriever named Max\n2. Tabby cat named Luna";
    let items2 = reduction::extract_enumerated_items_for_test(answer2);
    assert_eq!(items2.len(), 2);
    assert!(items2[0].contains("Golden Retriever"));
    assert!(items2[1].contains("Tabby cat"));

    // Empty answer
    let items3 = reduction::extract_enumerated_items_for_test("I don't know");
    assert!(items3.is_empty());
}

#[test]
fn test_normalize_item() {
    assert_eq!(
        reduction::normalize_item_for_test("The Great Gatsby (from session on 2024-01-01)"),
        "the great gatsby"
    );
    assert_eq!(
        reduction::normalize_item_for_test("  some  ITEM  with   spaces  "),
        "some item with spaces"
    );
    assert_eq!(
        reduction::normalize_item_for_test("Running shoes (session abc123)"),
        "running shoes"
    );
    // No citation
    assert_eq!(reduction::normalize_item_for_test("plain item"), "plain item");
}

#[test]
fn test_items_likely_duplicate() {
    // Exact after normalization
    assert!(reduction::items_likely_duplicate_for_test(
        "The Great Gatsby (from session on 2024-01-01)",
        "the great gatsby (from session on 2024-03-15)"
    ));
    // Different items
    assert!(!reduction::items_likely_duplicate_for_test("The Great Gatsby", "1984"));
    // Same content, different whitespace
    assert!(reduction::items_likely_duplicate_for_test("  Running shoes ", "running shoes"));
}

#[test]
fn test_deduplicate_items() {
    let items = vec![
        "The Great Gatsby (from session on 2024-01-01)".to_string(),
        "1984 (from session on 2024-02-01)".to_string(),
        "The Great Gatsby (from session on 2024-03-15)".to_string(),
    ];
    let deduped = reduction::deduplicate_items_for_test(&items);
    assert_eq!(deduped.len(), 2);

    // No duplicates
    let unique = vec!["A".to_string(), "B".to_string(), "C".to_string()];
    assert_eq!(reduction::deduplicate_items_for_test(&unique).len(), 3);
}

#[test]
fn test_is_sum_question() {
    assert!(is_sum_question("How much did I spend in total?"));
    assert!(is_sum_question("What was the total cost of the trip?"));
    assert!(is_sum_question("How much have I spent altogether?"));
    assert!(is_sum_question(
        "What is the combined cost of all my purchases?"
    ));
    // Not sum questions
    assert!(!is_sum_question("How much more did A cost than B?"));
    assert!(!is_sum_question(
        "What is the difference between the two prices?"
    ));
    assert!(!is_sum_question("What is my favorite color?"));
}

#[test]
fn test_extract_dollar_amounts() {
    let amounts = extract_dollar_amounts("$100 and $250.50");
    assert_eq!(amounts.len(), 2);
    assert!((amounts[0] - 100.0).abs() < 0.01);
    assert!((amounts[1] - 250.50).abs() < 0.01);

    // With commas
    let amounts2 = extract_dollar_amounts("The total was $1,234.56");
    assert_eq!(amounts2.len(), 1);
    assert!((amounts2[0] - 1234.56).abs() < 0.01);

    // No amounts
    assert!(extract_dollar_amounts("no dollars here").is_empty());
}

#[tokio::test]
#[ignore] // Requires OPENAI_API_KEY
async fn test_llm_client_real_api() {
    let client = LlmClient::from_env().expect("OPENAI_API_KEY must be set");

    let result = client
        .complete("gpt-4o-mini", "Say 'hello' in one word", 0.0)
        .await;
    assert!(result.is_ok(), "API call should succeed");

    let (response, cost) = result.unwrap();
    assert!(!response.is_empty(), "Response should not be empty");
    assert!(cost > 0.0, "Cost should be positive");
    println!("Response: {}, Cost: ${:.6}", response, cost);
}

// === P10: Count reducer tests ===

#[test]
fn test_extract_inline_enumerated_items() {
    let text = "Items: 1) hiking 2) kayaking 3) rock climbing";
    let items = reduction::extract_inline_enumerated_items_for_test(text);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], "hiking");
    assert_eq!(items[1], "kayaking");
    assert_eq!(items[2], "rock climbing");
}

#[test]
fn test_extract_inline_items_with_commas() {
    let text = "3 events: 1) art gallery opening, 2) sculpture workshop, 3) painting class";
    let items = reduction::extract_inline_enumerated_items_for_test(text);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], "art gallery opening");
    assert_eq!(items[1], "sculpture workshop");
    assert_eq!(items[2], "painting class");
}

#[test]
fn test_extract_enumerated_items_falls_back_to_inline() {
    // No line-start items, should fall back to inline
    let text = "The 3 activities are: 1) hiking 2) swimming 3) biking";
    let items = reduction::extract_enumerated_items_for_test(text);
    assert_eq!(items.len(), 3);
}

#[test]
fn test_extract_enumerated_items_prefers_line_start() {
    let text = "I found 3 activities:\n1) hiking\n2) swimming\n3) biking";
    let items = reduction::extract_enumerated_items_for_test(text);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], "hiking");
}

#[test]
fn test_extract_claimed_count_noun_pattern() {
    assert_eq!(reduction::extract_claimed_count_for_test("I found 3 items"), Some(3));
    assert_eq!(reduction::extract_claimed_count_for_test("There are 5 events"), Some(5));
    assert_eq!(reduction::extract_claimed_count_for_test("4 trips were mentioned"), Some(4));
}

#[test]
fn test_extract_claimed_count_prefix_pattern() {
    assert_eq!(
        reduction::extract_claimed_count_for_test("There are 7 things to consider"),
        Some(7)
    );
    assert_eq!(
        reduction::extract_claimed_count_for_test("I found 4 different activities"),
        Some(4)
    );
}

#[test]
fn test_extract_claimed_count_leading_digit() {
    assert_eq!(reduction::extract_claimed_count_for_test("3\n\n1) a\n2) b\n3) c"), Some(3));
}

#[test]
fn test_extract_claimed_count_none() {
    assert_eq!(reduction::extract_claimed_count_for_test("I don't know"), None);
    assert_eq!(reduction::extract_claimed_count_for_test("No information available"), None);
}

#[test]
fn test_is_counting_question() {
    assert!(is_counting_question("How many pets does the user have?"));
    assert!(is_counting_question("List all the books mentioned"));
    assert!(is_counting_question("How much did I spend in total?"));
    assert!(!is_counting_question("What is the user's favorite color?"));
    assert!(!is_counting_question("When did the user start working?"));
}

#[test]
fn test_reduce_count_mismatch() {
    let answer = "I found 3 events:\n1) art gallery\n2) sculpture class\n3) painting workshop\n4) pottery session";
    let question = "How many art events did I attend?";
    let (reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.reducer, "count");
    assert_eq!(log.claimed_count, Some(3));
    assert_eq!(log.listed_count, Some(4));
    assert_eq!(log.deduped_count, Some(4));
    assert_eq!(log.action, "corrected");
    // P15: delta=1 corrections get 0.98 confidence (above enforce threshold)
    assert!(log.confidence >= 0.95);
    assert!(reduced.contains("4"));
}

#[test]
fn test_reduce_count_consistent() {
    let answer = "I found 3 events:\n1) art gallery\n2) sculpture class\n3) painting workshop";
    let question = "How many art events did I attend?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.action, "consistent");
    assert_eq!(log.claimed_count, Some(3));
    assert_eq!(log.deduped_count, Some(3));
}

#[test]
fn test_reduce_count_no_items() {
    let answer = "I don't have enough information to answer this question.";
    let question = "How many pets does the user have?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.action, "no-op");
    assert_eq!(log.listed_count, Some(0));
}

#[test]
fn test_reduce_count_not_counting_question() {
    let answer = "The user's favorite color is blue.";
    let question = "What is the user's favorite color?";
    let (_reduced, log) = reduce_count(answer, question);
    assert!(log.is_none());
}

#[test]
fn test_reduce_count_large_delta_skipped() {
    let answer = "I found 10 items:\n1) a\n2) b\n3) c";
    let question = "How many items are there?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.action, "skipped");
    assert!(log.confidence < 0.5);
}

#[test]
fn test_reduce_count_no_claimed_count() {
    let answer = "Here are the items:\n1) apple\n2) banana\n3) cherry";
    let question = "How many fruits did I buy?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.action, "no-op");
    assert!(log.claimed_count.is_none());
}

#[test]
fn test_reduce_sum_arithmetic() {
    let answer = "The expenses were:\n1) Hotel: $500\n2) Flights: $300\n3) Food: $200\n\nThe total is $1,100";
    let question = "How much did I spend in total on the trip?";
    let (reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.reducer, "sum");
    assert_eq!(log.action, "corrected");
    // Items sum to $1000 but stated total is $1100
    assert!(reduced.contains("$1,000"));
}

#[test]
fn test_reduce_sum_consistent() {
    let answer = "The expenses were:\n1) Hotel: $500\n2) Flights: $300\n3) Food: $200\n\nThe total is $1,000";
    let question = "How much did I spend in total on the trip?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.reducer, "sum");
    assert_eq!(log.action, "consistent");
}

#[test]
fn test_correct_count_in_answer() {
    assert_eq!(
        reduction::correct_count_in_answer_for_test("I found 3 items", 3, 4),
        "I found 4 items"
    );
    // Should not replace digits inside larger numbers
    assert_eq!(
        reduction::correct_count_in_answer_for_test("I found 3 items out of 30", 3, 4),
        "I found 4 items out of 30"
    );
}

#[test]
fn test_format_dollar_with_commas() {
    assert_eq!(reduction::format_dollar_with_commas_for_test(1000.0), "$1,000");
    assert_eq!(reduction::format_dollar_with_commas_for_test(8750.0), "$8,750");
    assert_eq!(reduction::format_dollar_with_commas_for_test(500.0), "$500");
    assert_eq!(reduction::format_dollar_with_commas_for_test(1234567.0), "$1,234,567");
}

#[test]
fn test_inline_items_no_false_positive() {
    // Should NOT extract inline items from plain text with parenthetical numbers
    let text = "The user has been to Paris (2 times) and London (3 times)";
    let items = reduction::extract_inline_enumerated_items_for_test(text);
    // '2' and '3' are preceded by '(' not whitespace, so no match
    assert!(items.is_empty());
}

#[test]
fn test_extract_claimed_count_ignores_item_numbers() {
    // Claimed count should come from preamble, not from inside list items
    let answer = "Here are the 2 trips:\n1) Trip to Paris with 5 friends\n2) Trip to London with 3 friends";
    assert_eq!(reduction::extract_claimed_count_for_test(answer), Some(2));
}

#[test]
fn test_correct_count_only_in_preamble() {
    // Should replace "3" in preamble, not the "3)" enumeration marker
    let answer = "I found 3 events:\n1) gallery\n2) museum\n3) concert\n4) opera";
    let corrected = reduction::correct_count_in_answer_for_test(answer, 3, 4);
    assert!(corrected.starts_with("I found 4 events:"));
    // Enumeration "3)" should be unchanged
    assert!(corrected.contains("3) concert"));
}

#[test]
fn test_reduce_sum_uses_raw_items() {
    // Two identical expenses should both be counted (not deduped)
    let answer = "The meals were:\n1) Dinner: $50\n2) Dinner: $50\n\nThe total is $80";
    let question = "How much did I spend in total on dinners?";
    let (reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.reducer, "sum");
    assert_eq!(log.action, "corrected");
    // Raw sum is $100 (2 x $50), not $50 (deduped)
    assert!(reduced.contains("$100"));
}

#[test]
fn test_find_word_total_ignores_subtotal() {
    let lower = "subtotal is $500, total is $750";
    let original = "Subtotal is $500, Total is $750";
    let result = reduction::find_word_total_amount_for_test(lower, original);
    // Should find $750 (near "total"), not $500 (near "subtotal")
    assert_eq!(result, Some(750.0));
}

#[test]
fn test_confidence_above_enforce_threshold_for_delta_1() {
    // P15: delta=1 corrections should have confidence >= 0.95 (enforced)
    let answer = "I found 3 events:\n1) a\n2) b\n3) c\n4) d";
    let question = "How many events did I attend?";
    let (_reduced, log) = reduce_count(answer, question);
    let log = log.unwrap();
    assert_eq!(log.action, "corrected");
    assert!(
        log.confidence >= 0.95,
        "confidence {} should be >= 0.95 enforce threshold for delta=1",
        log.confidence
    );
}

// P15b: Tests for improved claim parser

#[test]
fn test_extract_claimed_count_word_form() {
    // "two tanks" — word-form number + noun
    assert_eq!(
        reduction::extract_claimed_count_for_test("You currently have two tanks: a 20-gallon tank and a 1-gallon tank."),
        Some(2)
    );
}

#[test]
fn test_extract_claimed_count_inline_items() {
    // "4 weddings. Items: 1) ..."
    assert_eq!(
        reduction::extract_claimed_count_for_test("4 weddings. Items: 1) Emily's wedding, 2) roommate's wedding"),
        Some(4)
    );
}

#[test]
fn test_extract_claimed_count_dinner_parties() {
    assert_eq!(
        reduction::extract_claimed_count_for_test("2 dinner parties. Items: 1) Potluck, 2) Italian feast"),
        Some(2)
    );
}

#[test]
fn test_extract_claimed_count_days() {
    assert_eq!(
        reduction::extract_claimed_count_for_test("2 days. Items: 1) Workshop on April 17-18, 2) Lecture on April 10"),
        Some(2)
    );
}

#[test]
fn test_extract_claimed_count_three_instruments() {
    assert_eq!(
        reduction::extract_claimed_count_for_test("You currently own three musical instruments: a Fender, a Yamaha, and a Korg."),
        Some(3)
    );
}

#[test]
fn test_extract_claimed_count_rejects_currency() {
    // "$65" and "$8,750" should NOT be treated as counts
    assert_eq!(reduction::extract_claimed_count_for_test("$65"), None);
    assert_eq!(reduction::extract_claimed_count_for_test("In total, you raised $8,750 for charity."), None);
}

#[test]
fn test_extract_claimed_count_rejects_decimal() {
    // "0.5 hours" should NOT be treated as a count
    assert_eq!(reduction::extract_claimed_count_for_test("0.5 hours"), None);
}

#[test]
fn test_extract_claimed_count_rejects_measurement() {
    // "20-gallon" should not be extracted as count 20
    assert_eq!(
        reduction::extract_claimed_count_for_test("a 20-gallon community tank"),
        None
    );
}

#[test]
fn test_extract_claimed_count_prefix_you_have() {
    assert_eq!(
        reduction::extract_claimed_count_for_test("You have 4 tanks in total."),
        Some(4)
    );
    assert_eq!(
        reduction::extract_claimed_count_for_test("You currently own 3 instruments."),
        Some(3)
    );
}

#[test]
fn test_extract_preamble_inline_items() {
    let answer = "4 weddings. Items: 1) Emily's, 2) roommate's, 3) friend's, 4) sister's";
    let preamble = reduction::extract_preamble_for_test(answer);
    assert_eq!(preamble, "4 weddings. ");
}

#[test]
fn test_word_to_number() {
    assert_eq!(reduction::word_to_number_for_test("two"), Some(2));
    assert_eq!(reduction::word_to_number_for_test("Three"), Some(3));
    assert_eq!(reduction::word_to_number_for_test("FIVE"), Some(5));
    assert_eq!(reduction::word_to_number_for_test("twenty"), Some(20));
    assert_eq!(reduction::word_to_number_for_test("cat"), None);
}

#[test]
fn test_correct_count_word_form() {
    let answer = "You currently have two tanks: a 20-gallon tank and a 1-gallon tank.";
    let corrected = reduction::correct_count_in_answer_for_test(answer, 2, 3);
    assert!(corrected.contains("3 tanks") || corrected.contains("3tanks"),
        "Expected '3' to replace 'two', got: {}", corrected);
}
