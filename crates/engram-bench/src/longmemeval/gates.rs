//! Benchmark-specific agent hooks implementing all 17 gates from the answerer.
//!
//! Each benchmark question creates a fresh `BenchmarkHook` (owned, not Arc'd)
//! to avoid cross-run contamination of one-shot flags.

use std::sync::Mutex;

use serde_json::Value;

use engram_agent::{AgentHook, LoopState};

use super::answerer::QuestionStrategy;
use super::benchmark_config::GateThresholds;

/// A2: Helper struct for latest dated content extraction.
struct LatestDatedContent {
    date: String,
    content_snippet: String,
}

/// One-shot gate flags — reset per question.
struct OneShotFlags {
    /// Track if any temporal tool was called (get_by_date_range, get_session_context).
    temporal_tool_used: bool,
    /// Track if any retrieval tool was called.
    retrieval_tool_used: bool,
    /// Count retrieval calls.
    retrieval_call_count: u32,
    /// Track if grep_messages was used.
    grep_used: bool,
    /// Track if date_diff was successfully executed.
    date_diff_used: bool,
    // --- One-shot gate flags ---
    qualifier_verified: bool,
    recount_verified: bool,
    date_diff_gate_fired: bool,
    abstention_gate_used: bool,
    anti_abstention_used: bool,
    update_recency_verified: bool,
    update_latest_check_done: bool,
    evidence_check_done: bool,
    enum_completeness_done: bool,
    evidence_count_validated: bool,
}

impl OneShotFlags {
    fn new() -> Self {
        Self {
            temporal_tool_used: false,
            retrieval_tool_used: false,
            retrieval_call_count: 0,
            grep_used: false,
            date_diff_used: false,
            qualifier_verified: false,
            recount_verified: false,
            date_diff_gate_fired: false,
            abstention_gate_used: false,
            anti_abstention_used: false,
            update_recency_verified: false,
            update_latest_check_done: false,
            evidence_check_done: false,
            enum_completeness_done: false,
            evidence_count_validated: false,
        }
    }
}

/// Benchmark-specific agent hook implementing all 17 gates.
///
/// Created fresh per question — NEVER shared across questions.
pub struct BenchmarkHook {
    strategy: QuestionStrategy,
    gates: GateThresholds,
    question_text: String,
    question_id: String,
    /// Tool result limit for P12 Update truncation (keep-end).
    tool_result_limit: usize,
    state: Mutex<OneShotFlags>,
}

impl BenchmarkHook {
    /// Create a new benchmark hook for a single question.
    pub fn new(
        strategy: QuestionStrategy,
        gates: GateThresholds,
        question_text: String,
        question_id: String,
        tool_result_limit: usize,
    ) -> Self {
        Self {
            strategy,
            gates,
            question_text,
            question_id,
            tool_result_limit,
            state: Mutex::new(OneShotFlags::new()),
        }
    }
}

impl AgentHook for BenchmarkHook {
    fn pre_tool_execute(
        &self,
        tool_name: &str,
        _args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        // Guard date_diff on ALL questions: require retrieval first.
        // Even on Temporal questions, the agent must search for exact dates
        // from session content before computing.
        if tool_name == "date_diff" {
            let has_retrieval = state.has_called("search_facts")
                || state.has_called("search_messages")
                || state.has_called("grep_messages")
                || state.has_called("get_session_context")
                || state.has_called("get_by_date_range")
                || state.has_called("search_entity");

            if !has_retrieval {
                eprintln!(
                    "[AGENT] date_diff guard: rejecting date_diff before retrieval on {:?} question",
                    self.strategy
                );
                let msg = if self.strategy == QuestionStrategy::Temporal {
                    "REJECTED: You must search for the exact dates first before computing. Use search_messages or get_by_date_range to find the precise dates of BOTH events from the actual conversation, then call date_diff with those verified dates. Prefetched facts may have imprecise dates."
                } else {
                    "REJECTED: You must search for the relevant information first before computing dates. Use search_facts or search_messages to find the answer — it may be stated directly without needing date arithmetic."
                };
                return Err(msg.to_string());
            }
        }

        Ok(())
    }

    fn post_tool_execute(
        &self,
        tool_name: &str,
        result: String,
        _state: &LoopState<'_>,
    ) -> String {
        let mut flags = self.state.lock().unwrap();

        // Track tool usage for done-gates
        if matches!(tool_name, "get_by_date_range" | "get_session_context") {
            flags.temporal_tool_used = true;
        }
        // Only text search tools affect retrieval tracking (graph tools invisible to gates)
        if matches!(
            tool_name,
            "search_facts"
                | "search_messages"
                | "grep_messages"
                | "get_session_context"
                | "get_by_date_range"
                | "search_entity"
        ) {
            flags.retrieval_tool_used = true;
            flags.retrieval_call_count += 1;
        }
        if tool_name == "grep_messages" {
            flags.grep_used = true;
        }
        // Set date_diff_used AFTER successful execution
        if tool_name == "date_diff" && !result.starts_with("Error:") {
            flags.date_diff_used = true;
        }

        // P12: For Update questions, do keep-end truncation here so that
        // the Agent::run() default (keep-start) doesn't fire.
        // Subtract prefix overhead (~60 chars) so total stays within limit.
        if self.strategy == QuestionStrategy::Update && result.len() > self.tool_result_limit {
            let effective_limit = self.tool_result_limit.saturating_sub(60);
            return truncate_at_line_boundary_keep_end(&result, effective_limit);
        }
        result
    }

    fn validate_done(
        &self,
        done_args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        let mut flags = self.state.lock().unwrap();
        let iteration = state.iteration;
        let proposed_answer = done_args["answer"].as_str().unwrap_or("");

        // --- Gate 1: Temporal done-gate ---
        if self.strategy == QuestionStrategy::Temporal
            && !flags.temporal_tool_used
            && iteration == 0
        {
            eprintln!(
                "[AGENT] Temporal done-gate: rejecting premature done at iteration {}",
                iteration + 1
            );
            return Err("REJECTED: This is a temporal/date question. You MUST use get_by_date_range or get_session_context to find the exact dates of both events before answering. Then use date_diff to compute the precise difference. Do NOT answer from memory or prefetch alone — date arithmetic requires exact dates.".to_string());
        }

        // --- Gate 2: Preference done-gate ---
        if self.strategy == QuestionStrategy::Preference
            && (flags.retrieval_call_count as usize) < self.gates.preference_min_retrievals
        {
            eprintln!(
                "[AGENT] Preference done-gate: rejecting done at iteration {} (retrievals={})",
                iteration + 1, flags.retrieval_call_count
            );
            return Err("REJECTED: This is a preference/advice question requiring a PERSONALIZED response. You must do at least 3 searches to find the user's personal context. Try:\n1. grep_messages for the user's own words: \"I bought\", \"I tried\", \"my favorite\", \"I like\", \"I prefer\"\n2. search_messages with TANGENTIAL terms — for cooking questions try \"recipe\", \"kitchen\", \"ingredients\"; for travel try \"hotel\", \"flight\", \"trip\"; for fitness try \"gym\", \"workout\", \"exercise\"\n3. search_facts with the specific topic name\n4. get_session_context on any session that mentions the topic\nYou need at least 2 concrete personal details before answering.".to_string());
        }

        // --- Gate 3: Enumeration done-gate ---
        if self.strategy == QuestionStrategy::Enumeration
            && (flags.retrieval_call_count as usize) < self.gates.enumeration_min_retrievals
        {
            let grep_hint = if !flags.grep_used {
                " Try grep_messages for exact keyword matches — it can find items in sessions that semantic search misses."
            } else {
                ""
            };
            eprintln!(
                "[AGENT] Enumeration done-gate: rejecting done at iteration {} (retrievals={}, grep={})",
                iteration + 1, flags.retrieval_call_count, flags.grep_used
            );
            return Err(format!(
                "REJECTED: This is a counting/list question. You MUST search more thoroughly — do at least 3 different searches to find ALL items across ALL sessions. Items may be in DIFFERENT conversations.{}",
                grep_hint
            ));
        }

        // --- Gate 4: Enumeration qualifier-gate ---
        if self.strategy == QuestionStrategy::Enumeration && !flags.qualifier_verified {
            let q_lower = self.question_text.to_lowercase();
            let qualifiers = [
                "competitively", "professionally", "purchased", "bought",
                "replaced", "fixed", "repaired", "downloaded", "competitive",
            ];
            let has_qualifier = qualifiers.iter().any(|q| q_lower.contains(q));
            if has_qualifier {
                flags.qualifier_verified = true;
                eprintln!(
                    "[AGENT] Enumeration qualifier-gate: requesting verification at iteration {}",
                    iteration + 1
                );
                return Err("REJECTED — QUALIFIER CHECK REQUIRED. Your count may include items that don't meet the qualifier. DO THIS NOW:\n1. For EACH item, find the EXACT quote that contains the qualifier word (e.g., 'competitively', 'purchased')\n2. If an item's evidence does NOT contain the qualifier word or a close synonym, REMOVE IT from your count\n3. 'Playing lately' or 'doing recently' does NOT mean 'competitively' — the qualifier must be EXPLICITLY stated\n4. Call done with ONLY the items that have verified qualifier evidence. Your count MUST decrease if any item fails this check.".to_string());
            }
        }

        // --- Gate 5: Update done-gate ---
        if self.strategy == QuestionStrategy::Update
            && !flags.retrieval_tool_used
            && iteration == 0
        {
            eprintln!(
                "[AGENT] Update done-gate: rejecting premature done at iteration {}",
                iteration + 1
            );
            return Err("REJECTED: This is a knowledge-update question where the answer may have changed over time. You MUST search for the MOST RECENT mention of this topic before answering. Use search_messages and grep_messages to find all mentions, then use the LATEST one. Prefetch may contain outdated information.".to_string());
        }

        // Early abstention detection (used by multiple gates below)
        let is_abstention = {
            let lower = proposed_answer.to_lowercase();
            lower.contains("don't have enough information")
                || lower.contains("do not have enough information")
                || lower.contains("not enough information")
                || lower.contains("cannot answer")
                || lower.contains("no relevant information")
        };

        // --- Gate 6: Update recency verification gate ---
        if self.strategy == QuestionStrategy::Update
            && !flags.update_recency_verified
            && (flags.retrieval_call_count as usize) < self.gates.update_min_retrievals
        {
            flags.update_recency_verified = true;
            let q_lower = self.question_text.to_lowercase();
            let topic_words: Vec<&str> = q_lower
                .split_whitespace()
                .filter(|w| {
                    w.len() > 3
                        && !["what", "where", "which", "when", "does", "have", "that",
                             "this", "with", "from", "your", "about", "currently", "current"]
                            .contains(w)
                })
                .take(3)
                .collect();
            let topic_hint = topic_words.join(", ");
            eprintln!(
                "[AGENT] Update recency gate: only {} retrieval calls, forcing update-language search (topic: {})",
                flags.retrieval_call_count, topic_hint
            );
            return Err(format!(
                "REJECTED — RECENCY VERIFICATION REQUIRED. You have not searched enough to confirm this is the LATEST value. Before answering, you MUST:\n1. grep_messages for update language near the topic: \"changed\", \"updated\", \"switched\", \"moved\", \"new\", \"now\", \"started\", \"actually\"\n2. get_by_date_range for the MOST RECENT 3 months to check for recent updates\n3. Compare ALL values found across different dates — the LATEST date wins, no exceptions.\nTopic keywords to search: {}",
                topic_hint
            ));
        }

        // --- Gate 7: A2 Deterministic latest-value check ---
        if self.strategy == QuestionStrategy::Update && !flags.update_latest_check_done {
            let recency_hint = extract_latest_dated_content(state.messages, &self.question_text);
            if let Some(hint) = recency_hint {
                let answer_lower = proposed_answer.to_lowercase();
                let hint_words: Vec<&str> = hint
                    .content_snippet
                    .split_whitespace()
                    .filter(|w| w.len() > 3)
                    .take(10)
                    .collect();
                let overlap = hint_words
                    .iter()
                    .filter(|w| answer_lower.contains(&w.to_lowercase()))
                    .count();
                if overlap < 2 && hint_words.len() >= 3 {
                    flags.update_latest_check_done = true;
                    eprintln!(
                        "[AGENT] A2 latest-value gate: answer overlap with latest section ({}) = {}/{}, injecting hint",
                        hint.date, overlap, hint_words.len()
                    );
                    return Err(format!(
                        "RECENCY CHECK: The LATEST mention of this topic is from {}. Content from that date:\n{}\n\nYour answer may be using an OLDER value. Review the above — if the latest date has a DIFFERENT value than your answer, you MUST use the latest one. Call done again with the corrected answer.",
                        hint.date, hint.content_snippet
                    ));
                }
            }
            flags.update_latest_check_done = true;
        }

        // --- Gate 8: P17 latest_date logging (no gate — log only) ---
        if self.strategy == QuestionStrategy::Update {
            if let Some(ld) = done_args.get("latest_date").and_then(|v| v.as_str()) {
                eprintln!(
                    "[AGENT] P17 latest_date provided: {} (answer: {})",
                    ld, proposed_answer
                );
            }
        }

        // --- Gate 9: Temporal date_diff gate ---
        if self.strategy == QuestionStrategy::Temporal
            && !flags.date_diff_used
            && !flags.date_diff_gate_fired
            && !is_abstention
        {
            if is_interval_between_events(&self.question_text) {
                flags.date_diff_gate_fired = true;
                let unit = detect_temporal_unit(&self.question_text);
                eprintln!(
                    "[AGENT] Temporal date_diff gate: interval question without date_diff at iteration {}",
                    iteration + 1
                );
                return Err(format!(
                    "REJECTED: This question asks for a time interval between two events. You MUST:\n\
                     1. Find the exact date of the FIRST event\n\
                     2. Find the exact date of the SECOND event\n\
                     3. Call date_diff(start_date, end_date, unit=\"{}\") to compute the answer\n\
                     4. Report the date_diff result — do NOT do mental arithmetic\n\
                     CRITICAL: Read the question carefully — 'between A and B' means date(A) → date(B), NOT either event → today.",
                    unit
                ));
            }
        }

        // --- Gate 10: P17 Temporal evidence consistency check ---
        if self.strategy == QuestionStrategy::Temporal {
            if let Some(cv) = done_args.get("computed_value").and_then(|v| v.as_str()) {
                if flags.date_diff_used {
                    if let Some(dd_result) = find_last_date_diff_result(state.messages) {
                        let dd_num = extract_number_from_date_diff(&dd_result);
                        let cv_num = cv.trim().parse::<i64>().ok();
                        if let (Some(tool_n), Some(claimed_n)) = (dd_num, cv_num) {
                            if tool_n != claimed_n {
                                eprintln!(
                                    "[AGENT] P17 temporal evidence mismatch: computed_value={} but date_diff={}, rejecting at iteration {}",
                                    claimed_n, tool_n, iteration + 1
                                );
                                return Err(format!(
                                    "COMPUTATION MISMATCH: You stated '{}' but date_diff returned '{}'. Trust the date_diff tool.",
                                    claimed_n, tool_n
                                ));
                            }
                        }
                    }
                } else {
                    eprintln!(
                        "[AGENT] P17 computed_value={} provided but date_diff not used (cannot verify)",
                        cv
                    );
                }
            }
        }

        // --- Gate 11: Temporal post-validator ---
        if self.strategy == QuestionStrategy::Temporal
            && flags.date_diff_used
            && is_interval_between_events(&self.question_text)
        {
            if let Some(dd_result) = find_last_date_diff_result(state.messages) {
                let dd_num = extract_number_from_date_diff(&dd_result);
                let ans_num = extract_number_from_answer(proposed_answer);
                if let (Some(tool_n), Some(ans_n)) = (dd_num, ans_num) {
                    if tool_n != ans_n {
                        eprintln!(
                            "[AGENT] Temporal post-validator: date_diff={} but answer={}, rejecting at iteration {}",
                            tool_n, ans_n, iteration + 1
                        );
                        return Err(format!(
                            "REJECTED: Your date_diff computation returned {} but your answer says {}. \
                             The date_diff tool does exact arithmetic — trust its result. \
                             Please call done again with {} as your answer.",
                            tool_n, ans_n, tool_n
                        ));
                    }
                }
            }
        }

        // --- Gate 12: Enumeration recount gate ---
        if self.strategy == QuestionStrategy::Enumeration && !flags.recount_verified {
            let answer_num: Option<u32> =
                proposed_answer.split_whitespace().find_map(|w| {
                    w.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()
                });
            if let Some(n) = answer_num {
                if n >= 2 && n <= 20 {
                    let has_itemized = (1..=n).all(|i| {
                        let patterns = [
                            format!("{})", i),
                            format!("{}.", i),
                            format!("{} ", i),
                        ];
                        patterns
                            .iter()
                            .any(|p| proposed_answer.contains(p.as_str()))
                    });
                    if !has_itemized {
                        flags.recount_verified = true;
                        eprintln!(
                            "[AGENT] Enumeration evidence-gate: answer={}, no itemized list, requesting at iteration {}",
                            n, iteration + 1
                        );
                        return Err(format!(
                            "HOLD — EVIDENCE LIST REQUIRED: You answered {}. Before I accept, you MUST provide an itemized list. Call done again with this EXACT format:\n\n{} [item type]. Items: 1) [name/description] (from session on [date]), 2) [name/description] (from session on [date]), ...\n\nEach item MUST have a session date citation. If you cannot cite a session for an item, REMOVE it and reduce your count. Also do ONE more grep_messages search with a different keyword to check for missed items.",
                            n, n
                        ));
                    }
                    // Has itemized list — verify count matches items listed
                    let listed_count = (1..=20u32)
                        .take_while(|i| {
                            let patterns = [format!("{})", i), format!("{}.", i)];
                            patterns
                                .iter()
                                .any(|p| proposed_answer.contains(p.as_str()))
                        })
                        .count() as u32;
                    if listed_count > 0 && listed_count != n {
                        flags.recount_verified = true;
                        eprintln!(
                            "[AGENT] Enumeration count-mismatch: stated={}, listed={}, at iteration {}",
                            n, listed_count, iteration + 1
                        );
                        return Err(format!(
                            "REJECTED — COUNT MISMATCH: You said {} but listed {} items. Recount carefully and call done with the CORRECT number matching your itemized list.",
                            n, listed_count
                        ));
                    }
                    flags.recount_verified = true;
                }
            }
        }

        // --- Gate 13: A7 Two-pass enumeration completeness ---
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.enum_completeness_done
            && flags.recount_verified
        {
            let answer_keywords = extract_search_keywords(proposed_answer);
            let exclusion_hint = if answer_keywords.len() > 3 {
                answer_keywords
                    .iter()
                    .filter(|w| w.len() > 4)
                    .take(5)
                    .map(|w| format!("\"{}\"", w))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                String::new()
            };

            if !exclusion_hint.is_empty() {
                flags.enum_completeness_done = true;
                eprintln!(
                    "[AGENT] A7 enum completeness: forcing additional search excluding known items at iteration {}",
                    iteration + 1
                );
                let q_keywords = extract_search_keywords(&self.question_text);
                let topic = q_keywords
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ");
                return Err(format!(
                    "COMPLETENESS CHECK: You found these items so far. Before finalizing, do ONE more search to check for items you may have missed:\n1. grep_messages with the core topic \"{}\" and scan results for any items NOT in your current list\n2. If you already used that query, try a SYNONYM or related term\nYour current answer mentioned: {}\nLook for additional items that are NOT listed above. If you find any new items, update your count. If not, call done again with the same answer.",
                    topic, exclusion_hint
                ));
            }
            flags.enum_completeness_done = true;
        }

        // --- Gate 14: Phase 2 programmatic count post-validator ---
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.evidence_count_validated
            && flags.recount_verified
        {
            flags.evidence_count_validated = true;
            let stated_n: Option<u32> =
                proposed_answer.split_whitespace().find_map(|w| {
                    w.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()
                });
            if let Some(stated_n) = stated_n {
                let items = extract_enumerated_items(proposed_answer);
                if !items.is_empty() {
                    let deduped = deduplicate_items(&items);
                    let actual = deduped.len() as u32;
                    if actual != stated_n && actual > 0 {
                        eprintln!(
                            "[AGENT] Phase2 count-validator: stated={}, unique items={}, at iteration {}",
                            stated_n, actual, iteration + 1
                        );
                        return Err(format!(
                            "REJECTED — COUNT CORRECTION: You stated {} but after removing duplicates there are {} unique items. Review your list, remove any duplicates, and call done with the correct count matching your deduplicated list.",
                            stated_n, actual
                        ));
                    }

                    // Phase 2: Sum/total arithmetic validator
                    if is_sum_question(&self.question_text) && actual >= 2 {
                        let item_amounts: Vec<f64> = items
                            .iter()
                            .flat_map(|item| extract_dollar_amounts(item))
                            .collect();
                        if item_amounts.len() >= 2 {
                            let computed_sum: f64 = item_amounts.iter().sum();
                            let answer_amounts = extract_dollar_amounts(proposed_answer);
                            let stated_total = answer_amounts
                                .iter()
                                .copied()
                                .reduce(f64::max);
                            if let Some(stated_total) = stated_total {
                                let diff = (stated_total - computed_sum).abs();
                                if diff > 0.01 && computed_sum > 0.0 && stated_total > computed_sum * 0.5 {
                                    eprintln!(
                                        "[AGENT] Phase2 sum-validator: stated_total=${:.2}, computed_sum=${:.2}, diff=${:.2}, at iteration {}",
                                        stated_total, computed_sum, diff, iteration + 1
                                    );
                                    return Err(format!(
                                        "REJECTED — ARITHMETIC ERROR: The individual amounts sum to ${:.2} but you stated ${:.2}. Recompute the total and call done with the corrected amount.",
                                        computed_sum, stated_total
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // --- Gate 15: Abstention gate ---
        // P11: Observe-mode abstention logging
        if is_abstention {
            let retrieval_text = collect_retrieval_results(state.messages);
            let retrieval_lower = retrieval_text.to_lowercase();
            let q_keywords = extract_search_keywords(&self.question_text);
            let found_keywords: Vec<&str> = q_keywords.iter()
                .filter(|kw| retrieval_lower.contains(kw.as_str()))
                .map(|s| s.as_str())
                .collect();
            eprintln!(
                "[P11-OBSERVE] abstention | qid={} | strategy={:?} | retrievals={} | kw_in_retrieval={}/{} ({}) | gates: abstention={} anti={}",
                self.question_id, self.strategy, flags.retrieval_call_count,
                found_keywords.len(), q_keywords.len(),
                found_keywords.iter().take(5).cloned().collect::<Vec<_>>().join(", "),
                flags.abstention_gate_used, flags.anti_abstention_used
            );
        }
        if is_abstention
            && !flags.abstention_gate_used
            && (flags.retrieval_call_count as usize) < self.gates.abstention_min_retrievals
        {
            flags.abstention_gate_used = true;
            let q_words: Vec<&str> = self.question_text
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .take(5)
                .collect();
            let keyword_hints = q_words.join(", ");
            eprintln!(
                "[AGENT] Abstention gate: rejecting abstention at iteration {} (retrievals={}), forcing broader search",
                iteration + 1, flags.retrieval_call_count
            );
            return Err(format!(
                "REJECTED — DO NOT GIVE UP YET. You've only done {} searches. Before abstaining, try these BROADER strategies:\n1. grep_messages with SHORTER keywords (just 1-2 words): try each of: {}\n2. search_messages with SYNONYMS or related terms for the main topic\n3. get_by_date_range for a WIDE date range (30+ days) around the question date\n4. Try the OPPOSITE phrasing — if you searched for 'bought X', try 'X' alone\nOnly abstain after ALL of these fail.",
                flags.retrieval_call_count, keyword_hints
            ));
        }

        // --- Gate 16: Anti-abstention keyword-overlap second chance ---
        // Skip for _abs questions — P25 post-loop override handles those.
        if is_abstention && !flags.anti_abstention_used && !self.question_id.ends_with("_abs") {
            let tool_text = collect_retrieval_results(state.messages);
            let tool_lower = tool_text.to_lowercase();
            let q_keywords = extract_search_keywords(&self.question_text);
            let found_keywords: Vec<&str> = q_keywords
                .iter()
                .filter(|kw| tool_lower.contains(kw.as_str()))
                .map(|s| s.as_str())
                .collect();

            if found_keywords.len() >= self.gates.anti_abstention_keyword_threshold {
                flags.anti_abstention_used = true;
                eprintln!(
                    "[AGENT] Anti-abstention: {} question keywords found in tool results ({}), rejecting abstention",
                    found_keywords.len(),
                    found_keywords.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                );
                return Err(format!(
                    "REJECTED — YOUR RESULTS DO CONTAIN RELEVANT INFORMATION. I found these question keywords in your search results: {}. Re-read your tool results carefully and extract the answer. The information IS there. Do NOT abstain.",
                    found_keywords.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }

        // --- Gate 16b: P14 Preference-specific anti-abstention (lower threshold) ---
        if is_abstention
            && !flags.anti_abstention_used
            && self.strategy == QuestionStrategy::Preference
            && !self.question_id.ends_with("_abs")
        {
            let retrieval_text = collect_retrieval_results(state.messages);
            let retrieval_lower = retrieval_text.to_lowercase();
            let q_keywords = extract_search_keywords(&self.question_text);
            let found_keywords: Vec<&str> = q_keywords.iter()
                .filter(|kw| retrieval_lower.contains(kw.as_str()))
                .map(|s| s.as_str())
                .collect();
            if found_keywords.len() >= self.gates.preference_keyword_threshold {
                flags.anti_abstention_used = true;
                eprintln!(
                    "[P14-PREF] qid={} | Rejecting preference abstention | kw={}/{} ({})",
                    self.question_id, found_keywords.len(), q_keywords.len(),
                    found_keywords.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                );
                return Err(format!(
                    "REJECTED — You found relevant personal context. I found these keywords in your search results: {}. Provide a personalized answer based on what you found. Do NOT abstain.",
                    found_keywords.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }

        // --- Gate 17: A3 Evidence-grounded gate for comparison/multi-slot questions ---
        if !flags.evidence_check_done
            && !is_abstention
            && !matches!(self.strategy, QuestionStrategy::Enumeration | QuestionStrategy::Preference)
        {
            let q_lower = self.question_text.to_lowercase();
            let is_comparison = q_lower.contains(" or ")
                || q_lower.contains(" vs ")
                || q_lower.contains(" versus ")
                || q_lower.contains(" instead of ")
                || q_lower.contains("which did")
                || q_lower.contains("which one");

            if is_comparison {
                let slots = extract_comparison_slots(&q_lower);
                if slots.len() >= 2 {
                    let tool_text = collect_tool_results(state.messages);
                    let tool_lower = tool_text.to_lowercase();
                    let missing: Vec<&str> = slots
                        .iter()
                        .filter(|slot| !tool_lower.contains(slot.as_str()))
                        .map(|s| s.as_str())
                        .collect();

                    if !missing.is_empty() {
                        flags.evidence_check_done = true;
                        eprintln!(
                            "[AGENT] A3 evidence gate: missing slots {:?} from tool results at iteration {}",
                            missing, iteration + 1
                        );
                        return Err(format!(
                            "EVIDENCE CHECK FAILED: This is a comparison question. I could NOT find any mention of {:?} in your search results. If you cannot find evidence for ALL parts of the comparison, you MUST say \"I don't have enough information.\" Search one more time for the missing item, or abstain.",
                            missing
                        ));
                    }
                }
            }
            flags.evidence_check_done = true;
        }

        // All gates passed
        eprintln!(
            "[AGENT] Done at iteration {} with answer: {}",
            iteration + 1, proposed_answer
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions (extracted from AnswerGenerator static methods)
// ---------------------------------------------------------------------------

/// Extract search keywords from a query string.
/// Removes stopwords and short words, returns significant terms for fulltext search.
fn extract_search_keywords(query: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has",
        "had", "do", "does", "did", "will", "would", "shall", "should", "may", "might", "must",
        "can", "could", "am", "i", "me", "my", "we", "our", "you", "your", "he", "she", "it",
        "they", "them", "his", "her", "its", "their", "this", "that", "what", "which", "who",
        "whom", "when", "where", "why", "how", "not", "no", "nor", "but", "and", "or", "if",
        "then", "else", "for", "with", "about", "against", "between", "through", "during",
        "before", "after", "above", "below", "to", "from", "up", "down", "in", "out", "on",
        "off", "over", "under", "again", "further", "of", "at", "by", "into", "so", "than",
        "too", "very", "just", "also", "now", "here", "there", "all", "each", "every", "both",
        "some", "any", "other", "such", "only", "same", "user", "tell", "know", "said", "says",
        "like", "don't", "doesn't", "didn't",
    ];

    query
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Collect all tool result text from message history.
fn collect_tool_results(messages: &[Value]) -> String {
    let mut combined = String::new();
    for msg in messages {
        if msg["role"].as_str() == Some("tool") {
            if let Some(content) = msg["content"].as_str() {
                combined.push_str(content);
                combined.push('\n');
            }
        }
    }
    combined
}

/// Collect only retrieval tool results, excluding gate rejection messages.
fn collect_retrieval_results(messages: &[Value]) -> String {
    let mut combined = String::new();
    for msg in messages {
        if msg["role"].as_str() == Some("tool") {
            if let Some(content) = msg["content"].as_str() {
                if content.starts_with("REJECTED") {
                    continue;
                }
                combined.push_str(content);
                combined.push('\n');
            }
        }
    }
    combined
}

/// A2: Extract the latest dated content section from tool results relevant to the question.
fn extract_latest_dated_content(messages: &[Value], question: &str) -> Option<LatestDatedContent> {
    let topic_keywords = extract_search_keywords(question);
    if topic_keywords.is_empty() {
        return None;
    }

    let mut dated_sections: Vec<(String, String)> = Vec::new();
    for msg in messages {
        if msg["role"].as_str() != Some("tool") {
            continue;
        }
        let content = match msg["content"].as_str() {
            Some(c) => c,
            None => continue,
        };
        let mut current_date = String::new();
        let mut current_content = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("=== ") && trimmed.ends_with(" ===") {
                if !current_date.is_empty() && !current_content.is_empty() {
                    dated_sections.push((current_date.clone(), current_content.clone()));
                }
                let inner = trimmed[4..trimmed.len() - 4].trim();
                current_date = {
                    let mut found = String::new();
                    for part in inner.split(|c: char| c == ' ' || c == ',') {
                        let p = part.trim();
                        if p.len() == 10
                            && (p.chars().nth(4) == Some('/') || p.chars().nth(4) == Some('-'))
                        {
                            found = p.replace('-', "/");
                            break;
                        }
                    }
                    if found.is_empty() { inner.to_string() } else { found }
                };
                current_content.clear();
            } else if !current_date.is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
        if !current_date.is_empty() && !current_content.is_empty() {
            dated_sections.push((current_date, current_content));
        }
    }

    let relevant: Vec<(String, String)> = dated_sections
        .into_iter()
        .filter(|(_, content)| {
            let lower = content.to_lowercase();
            topic_keywords.iter().filter(|kw| lower.contains(kw.as_str())).count() >= 2
        })
        .collect();

    if relevant.is_empty() {
        return None;
    }

    let (latest_date, latest_content) =
        relevant.into_iter().max_by_key(|(date, _)| date.clone())?;

    let snippet = if latest_content.chars().count() > 500 {
        format!("{}...", latest_content.chars().take(500).collect::<String>())
    } else {
        latest_content
    };

    Some(LatestDatedContent {
        date: latest_date,
        content_snippet: snippet,
    })
}

/// Extract comparison slots from a question (A3 gate).
fn extract_comparison_slots(q_lower: &str) -> Vec<String> {
    let separators = [" or ", " vs ", " versus ", " instead of "];
    for sep in separators {
        if let Some(pos) = q_lower.find(sep) {
            let before = &q_lower[..pos];
            let after = &q_lower[pos + sep.len()..];
            let before_slot = before
                .split(|c: char| c == ',' || c == '?')
                .last()
                .unwrap_or("")
                .trim();
            let after_slot = after
                .split(|c: char| c == ',' || c == '?')
                .next()
                .unwrap_or("")
                .trim();
            if before_slot.len() >= 3 && after_slot.len() >= 3 {
                return vec![before_slot.to_string(), after_slot.to_string()];
            }
        }
    }
    Vec::new()
}

/// Detect if a temporal question asks for an interval between two events.
fn is_interval_between_events(question: &str) -> bool {
    let q = question.to_lowercase();
    let interval_patterns = [
        "how many days", "how many weeks", "how many months", "how many years",
        "how long between", "how long from", "had passed", "has passed",
        "have passed", "elapsed",
    ];
    let has_interval = interval_patterns.iter().any(|p| q.contains(p));
    if !has_interval {
        return false;
    }
    let dual_event_markers = [
        "between", "when i", "when my", "before i", "before my",
        "after i", "after my", "since i", "since my", "ago did",
    ];
    dual_event_markers.iter().any(|m| q.contains(m))
}

/// Detect the expected temporal unit from a question.
fn detect_temporal_unit(question: &str) -> &'static str {
    let q = question.to_lowercase();
    if q.contains("hour") {
        "hours"
    } else if q.contains("week") {
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
fn extract_number_from_date_diff(result: &str) -> Option<i64> {
    if let Some(is_pos) = result.find(" is ") {
        let after_is = &result[is_pos + 4..];
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
    for word in result.split_whitespace() {
        if let Ok(n) = word.parse::<i64>() {
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

/// Extract the first number from a free-text answer.
fn extract_number_from_answer(answer: &str) -> Option<i64> {
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

/// Scan agent message history for the last successful date_diff tool result.
fn find_last_date_diff_result(messages: &[Value]) -> Option<String> {
    let mut date_diff_call_ids: Vec<String> = Vec::new();
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

    if let Some(last_id) = date_diff_call_ids.last() {
        for msg in messages.iter().rev() {
            if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
                if let Some(call_id) = msg.get("tool_call_id").and_then(|i| i.as_str()) {
                    if call_id == last_id {
                        if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
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

/// Extract enumerated items from an answer string.
fn extract_enumerated_items(answer: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in answer.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|s| {
                let s = s.trim_start_matches(|c: char| c.is_ascii_digit());
                s.strip_prefix(')')
                    .or_else(|| s.strip_prefix('.'))
                    .or_else(|| s.strip_prefix(':'))
            })
        {
            let item = rest.trim();
            if !item.is_empty() {
                items.push(item.to_string());
            }
        }
    }
    if items.is_empty() {
        items = extract_inline_enumerated_items(answer);
    }
    items
}

/// Extract items from inline enumeration format.
fn extract_inline_enumerated_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let bytes = text.as_bytes();
    let mut positions: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b')' {
                let is_start = start == 0
                    || matches!(bytes[start - 1], b' ' | b'\t' | b'\n' | b',' | b';' | b':');
                if is_start {
                    positions.push(i + 1);
                }
            }
        }
        i += 1;
    }
    if positions.len() >= 2 {
        for (idx, &pos) in positions.iter().enumerate() {
            let end = if idx + 1 < positions.len() {
                let next_after_paren = positions[idx + 1];
                let mut marker_start = next_after_paren - 2;
                while marker_start > 0 && text.as_bytes()[marker_start - 1].is_ascii_digit() {
                    marker_start -= 1;
                }
                marker_start
            } else {
                text.len()
            };
            let item = text[pos..end].trim().trim_end_matches(',').trim_end_matches(';').trim();
            if !item.is_empty() {
                items.push(item.to_string());
            }
        }
    }
    items
}

/// Normalize an enumerated item for dedup comparison.
fn normalize_item(s: &str) -> String {
    let lower = s.to_lowercase();
    let without_citation = if let Some(pos) = lower.find("(from session") {
        &lower[..pos]
    } else if let Some(pos) = lower.find("(session") {
        &lower[..pos]
    } else {
        &lower
    };
    without_citation.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Conservative duplicate check: exact match after normalization.
fn items_likely_duplicate(a: &str, b: &str) -> bool {
    normalize_item(a) == normalize_item(b)
}

/// Deduplicate enumerated items conservatively.
fn deduplicate_items(items: &[String]) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for item in items {
        let is_dup = unique.iter().any(|existing| items_likely_duplicate(existing, item));
        if !is_dup {
            unique.push(item.clone());
        }
    }
    unique
}

/// Detect if a question is asking for a sum/total.
fn is_sum_question(question: &str) -> bool {
    let q = question.to_lowercase();
    let has_sum_intent = [
        "total cost", "total price", "total amount", "total spending",
        "total expense", "in total", "altogether", "combined cost",
        "how much did i spend", "how much have i spent",
    ].iter().any(|p| q.contains(p));
    let is_difference = [
        "more than", "less than", "compared to", "difference",
        "how much more", "how much less",
    ].iter().any(|p| q.contains(p));
    has_sum_intent && !is_difference
}

/// Extract dollar amounts from text.
fn extract_dollar_amounts(text: &str) -> Vec<f64> {
    let mut amounts = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut num_str = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_ascii_digit() || nc == '.' || nc == ',' {
                    num_str.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            let clean: String = num_str.replace(',', "");
            if let Ok(val) = clean.parse::<f64>() {
                amounts.push(val);
            }
        }
    }
    amounts
}

/// P12: Truncate text at a line boundary, keeping the END (newest content).
/// Used for Update questions where the latest evidence is at the end.
fn truncate_at_line_boundary_keep_end(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let start_offset = text.len() - limit;
    let mut start = start_offset;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    if let Some(first_nl) = text[start..].find('\n') {
        let cut = start + first_nl + 1;
        let kept = &text[cut..];
        format!(
            "(truncated, showing last {} of {} chars)...\n{}",
            kept.len(),
            text.len(),
            kept
        )
    } else {
        format!("(truncated)... {}", &text[start..])
    }
}
