//! Production quality gates for the memory answering agent.
//!
//! `MemoryAgentHook` implements [`AgentHook`] with 18 gates + 1 pre-tool guard.
//! All gate thresholds are driven by [`GateConfig`] (TOML-configurable).
//!
//! # Gate inventory
//!
//! **Pre-tool guard:** date_diff guard (require retrieval before computation)
//!
//! **Temporal (Phase 4a):** Gates 1, 9, 10, 11
//! **Update (Phase 4b):** Gates 5, 6, 7, 8 + P12 truncation
//! **Enumeration (Phase 4c):** Gates 3, 4, 12, 13, 14
//! **Abstention + evidence + preference (Phase 4d):** Gates 2, 15, 16, 16b, 17
//!
//! # Configurable gates (off by default in production)
//!
//! - Gate 4 (enumeration qualifier): fires only when `anti_abstention_keyword_threshold > 0`
//!   is implicitly always-on in benchmark configs
//! - Gate 16 (anti-abstention keyword overlap): fires when `anti_abstention_keyword_threshold > 0`
//! - Gate 16b (preference anti-abstention): fires when `preference_keyword_threshold > 0`

use std::sync::Mutex;

use serde_json::Value;

use engram_core::agent::{QuestionStrategy, is_sum_question};
use engram_core::config::GateConfig;

use crate::{AgentHook, LoopState};

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A2: Helper struct for latest dated content extraction.
struct LatestDatedContent {
    date: String,
    #[allow(dead_code)]
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

// ---------------------------------------------------------------------------
// MemoryAgentHook
// ---------------------------------------------------------------------------

/// Production memory agent hook implementing all quality gates.
///
/// Created fresh per question — NEVER shared across questions.
/// All gate thresholds come from [`GateConfig`] which is TOML-driven.
pub struct MemoryAgentHook {
    strategy: QuestionStrategy,
    gates: GateConfig,
    question_text: String,
    /// Tool result limit for P12 Update truncation (keep-end).
    tool_result_limit: usize,
    /// When true, skip anti-abstention gates (16, 16b).
    /// Benchmark sets this for known-abstention questions; production leaves false.
    skip_anti_abstention: bool,
    state: Mutex<OneShotFlags>,
}

impl MemoryAgentHook {
    /// Create a new memory agent hook for a single question.
    pub fn new(
        strategy: QuestionStrategy,
        gates: GateConfig,
        question_text: String,
        tool_result_limit: usize,
        skip_anti_abstention: bool,
    ) -> Self {
        Self {
            strategy,
            gates,
            question_text,
            tool_result_limit,
            skip_anti_abstention,
            state: Mutex::new(OneShotFlags::new()),
        }
    }
}

impl AgentHook for MemoryAgentHook {
    // -----------------------------------------------------------------------
    // Pre-tool guard
    // -----------------------------------------------------------------------

    fn pre_tool_execute(
        &self,
        tool_name: &str,
        _args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        // Guard date_diff on ALL questions: require retrieval first.
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

    // -----------------------------------------------------------------------
    // Post-tool transform
    // -----------------------------------------------------------------------

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
        if tool_name == "date_diff" && result.contains(" is ") && !result.starts_with("Error") {
            flags.date_diff_used = true;
        }

        // P12: For Update questions, truncate keeping the END (newest content)
        if self.strategy == QuestionStrategy::Update
            && matches!(
                tool_name,
                "search_facts"
                    | "search_messages"
                    | "grep_messages"
                    | "get_by_date_range"
                    | "search_entity"
            )
            && result.len() > self.tool_result_limit
        {
            return truncate_at_line_boundary_keep_end(&result, self.tool_result_limit);
        }

        result
    }

    // -----------------------------------------------------------------------
    // validate_done: All 18 gates
    // -----------------------------------------------------------------------

    fn validate_done(
        &self,
        done_args: &Value,
        state: &LoopState<'_>,
    ) -> Result<(), String> {
        let mut flags = self.state.lock().unwrap();
        let iteration = state.iteration;
        let proposed_answer = done_args
            .get("answer")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let is_abstention = is_prompt_abstention(proposed_answer);

        // ===================================================================
        // Phase 4a: Temporal gates
        // ===================================================================

        // --- Gate 1: Temporal done-gate ---
        if self.strategy == QuestionStrategy::Temporal
            && !flags.temporal_tool_used
            && !is_abstention
        {
            eprintln!(
                "[AGENT] Temporal gate: rejecting done without temporal tool at iteration {}",
                iteration + 1
            );
            return Err(
                "REJECTED: For temporal questions, you MUST use get_by_date_range or get_session_context to verify exact dates before answering. Search for the specific dates mentioned in the question."
                    .to_string(),
            );
        }

        // --- Gate 9: date_diff requirement ---
        if self.strategy == QuestionStrategy::Temporal
            && is_interval_between_events(&self.question_text)
            && !flags.date_diff_used
            && !is_abstention
        {
            let unit = detect_temporal_unit(&self.question_text);
            eprintln!(
                "[AGENT] date_diff requirement: question asks about {} between events, but no date_diff called at iteration {}",
                unit, iteration + 1
            );
            return Err(format!(
                "REJECTED: This question asks about the number of {} between two events. You MUST use the date_diff tool to compute this precisely. Find the exact dates of BOTH events from session headers, then call date_diff(start_date, end_date, unit=\"{}\"). NEVER do date arithmetic in your head.",
                unit, unit
            ));
        }

        // --- Gate 10: date_diff consistency ---
        if self.strategy == QuestionStrategy::Temporal
            && flags.date_diff_used
            && !flags.date_diff_gate_fired
            && !is_abstention
        {
            if let Some(last_result) = find_last_date_diff_result(state.messages) {
                if let Some(computed) = extract_number_from_date_diff(&last_result) {
                    if let Some(stated) = extract_number_from_answer(proposed_answer) {
                        if computed != stated {
                            flags.date_diff_gate_fired = true;
                            let unit = detect_temporal_unit(&self.question_text);
                            eprintln!(
                                "[AGENT] date_diff consistency: date_diff returned {} but answer says {} at iteration {}",
                                computed, stated, iteration + 1
                            );
                            return Err(format!(
                                "REJECTED — ARITHMETIC MISMATCH: Your date_diff tool returned {} {} but your answer states {}. Use the EXACT number from date_diff. Do not round or adjust the tool's result.",
                                computed, unit, stated
                            ));
                        }
                    }
                }
            }
        }

        // --- Gate 11: Temporal post-validator ---
        if self.strategy == QuestionStrategy::Temporal
            && !flags.date_diff_gate_fired
            && !is_abstention
        {
            if is_interval_between_events(&self.question_text) && flags.date_diff_used {
                if let Some(last_result) = find_last_date_diff_result(state.messages) {
                    if let Some(computed) = extract_number_from_date_diff(&last_result) {
                        if let Some(stated) = extract_number_from_answer(proposed_answer) {
                            if computed != stated {
                                flags.date_diff_gate_fired = true;
                                let unit = detect_temporal_unit(&self.question_text);
                                eprintln!(
                                    "[AGENT] Temporal post-validator: computed={}, stated={}, at iteration {}",
                                    computed, stated, iteration + 1
                                );
                                return Err(format!(
                                    "REJECTED — Your answer ({}) doesn't match the date_diff result ({} {}). Use the EXACT computed value.",
                                    stated, computed, unit
                                ));
                            }
                        }
                    }
                }
            }
        }

        // ===================================================================
        // Phase 4b: Update gates
        // ===================================================================

        // --- Gate 5: Update done-gate ---
        if self.strategy == QuestionStrategy::Update
            && !flags.retrieval_tool_used
            && !is_abstention
        {
            let min = self.gates.update_min_retrievals;
            if (flags.retrieval_call_count as usize) < min {
                eprintln!(
                    "[AGENT] Update gate: only {} retrievals (min {}), rejecting at iteration {}",
                    flags.retrieval_call_count, min, iteration + 1
                );
                return Err(format!(
                    "REJECTED: For update questions, you must search thoroughly (at least {} retrieval calls) to find the MOST RECENT value. Search with different phrasings and check for updates.",
                    min
                ));
            }
        }

        // --- Gate 6: Update recency verification (A2) ---
        if self.strategy == QuestionStrategy::Update
            && !flags.update_recency_verified
            && !is_abstention
        {
            if let Some(latest) = extract_latest_dated_content(state.messages, &self.question_text)
            {
                let answer_lower = proposed_answer.to_lowercase();
                let latest_lower = latest.content_snippet.to_lowercase();
                let answer_words: Vec<&str> = answer_lower
                    .split_whitespace()
                    .filter(|w| w.len() > 3)
                    .collect();
                let answer_in_latest = answer_words
                    .iter()
                    .any(|w| latest_lower.contains(w));

                if !answer_in_latest {
                    flags.update_recency_verified = true;
                    eprintln!(
                        "[AGENT] A2 recency: answer '{}' not found in latest content (date {}), rejecting at iteration {}",
                        proposed_answer, latest.date, iteration + 1
                    );
                    return Err(format!(
                        "REJECTED — STALE VALUE: The most recent information I found is from {}. Your answer doesn't appear in that latest content. Please check the most recent evidence and use the LATEST value, not an older one. Look for the content from {} specifically.",
                        latest.date, latest.date
                    ));
                }
            }
        }

        // --- Gate 7: A2 latest-value check ---
        if self.strategy == QuestionStrategy::Update
            && !flags.update_latest_check_done
            && !is_abstention
        {
            let done_latest = done_args
                .get("latest_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !done_latest.is_empty() {
                if let Some(latest) =
                    extract_latest_dated_content(state.messages, &self.question_text)
                {
                    if done_latest < latest.date.as_str() {
                        flags.update_latest_check_done = true;
                        eprintln!(
                            "[AGENT] A2 latest-value: done says {} but latest content is from {}, rejecting at iteration {}",
                            done_latest, latest.date, iteration + 1
                        );
                        return Err(format!(
                            "REJECTED — You reported latest_date={} but I found more recent content from {}. Re-examine the evidence from {} and update your answer to reflect the most recent value.",
                            done_latest, latest.date, latest.date
                        ));
                    }
                }
            }
        }

        // --- Gate 8: latest_date logging ---
        if self.strategy == QuestionStrategy::Update {
            let done_latest = done_args
                .get("latest_date")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            eprintln!(
                "[AGENT] Update answer: '{}', latest_date='{}' at iteration {}",
                proposed_answer, done_latest, iteration + 1
            );
        }

        // ===================================================================
        // Phase 4c: Enumeration gates
        // ===================================================================

        // --- Gate 3: Enumeration done-gate ---
        if self.strategy == QuestionStrategy::Enumeration
            && !is_abstention
        {
            let min = self.gates.enumeration_min_retrievals;
            if (flags.retrieval_call_count as usize) < min {
                eprintln!(
                    "[AGENT] Enumeration gate: only {} retrievals (min {}), rejecting at iteration {}",
                    flags.retrieval_call_count, min, iteration + 1
                );
                return Err(format!(
                    "REJECTED: For list/count questions, you must search thoroughly (at least {} retrieval calls). Try different search queries, synonyms, and check multiple sessions.",
                    min
                ));
            }

            if !flags.grep_used {
                eprintln!(
                    "[AGENT] Enumeration gate: grep_messages not used, rejecting at iteration {}",
                    iteration + 1
                );
                return Err(
                    "REJECTED: For list/count questions, you MUST use grep_messages at least once with a key term from the question. Exact text search often finds items that semantic search misses."
                        .to_string(),
                );
            }
        }

        // --- Gate 4: Enumeration qualifier check ---
        // Always on — verifies enumerated items match question constraints.
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.qualifier_verified
            && !is_abstention
        {
            let q_lower = self.question_text.to_lowercase();
            let has_qualifier = q_lower.contains(" that ")
                || q_lower.contains(" which ")
                || q_lower.contains(" where ")
                || q_lower.contains(" who ");

            if has_qualifier {
                let items = extract_enumerated_items(proposed_answer);
                if items.len() >= 2 {
                    flags.qualifier_verified = true;
                    eprintln!(
                        "[AGENT] Gate 4 qualifier: found {} items with qualifier in question, requesting verification at iteration {}",
                        items.len(), iteration + 1
                    );
                    return Err(
                        "REJECTED — QUALIFIER CHECK: This question has a specific qualifier (that/which/where/who). Before finalizing, verify that EACH item in your list meets the qualifier. Remove any items that don't match the specific constraint. Then call done with the verified list."
                            .to_string(),
                    );
                }
            }
        }

        // --- Gate 12: Enumeration recount ---
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.recount_verified
            && !is_abstention
        {
            let stated_n = extract_number_from_answer(proposed_answer).map(|n| n as u32);
            let items = extract_enumerated_items(proposed_answer);

            if let Some(n) = stated_n {
                if !items.is_empty() && items.len() as u32 != n {
                    flags.recount_verified = true;
                    eprintln!(
                        "[AGENT] Recount: stated {} but listed {} items at iteration {}",
                        n, items.len(), iteration + 1
                    );
                    return Err(format!(
                        "REJECTED — COUNT MISMATCH: You said {} but listed {} items. Recount your list carefully and call done with the correct number matching your actual list.",
                        n, items.len()
                    ));
                }
            }
        }

        // --- Gate 13: Two-pass completeness ---
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.enum_completeness_done
            && !is_abstention
        {
            let items = extract_enumerated_items(proposed_answer);
            if items.len() >= 2 {
                flags.enum_completeness_done = true;
                eprintln!(
                    "[AGENT] Completeness gate: {} items found, requesting final verification at iteration {}",
                    items.len(), iteration + 1
                );
                return Err(
                    "REJECTED — COMPLETENESS CHECK: Before finalizing your list, do ONE more search with a COMPLETELY DIFFERENT keyword than any you've tried. This is mandatory to catch items you may have missed. Then call done with your final, verified list."
                        .to_string(),
                );
            }
        }

        // --- Gate 14: Count/sum validator ---
        if self.strategy == QuestionStrategy::Enumeration
            && !flags.evidence_count_validated
            && !is_abstention
        {
            flags.evidence_count_validated = true;

            let stated_n = extract_number_from_answer(proposed_answer).map(|n| n as u32);
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
                                if diff > 0.01
                                    && computed_sum > 0.0
                                    && stated_total > computed_sum * 0.5
                                {
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

        // ===================================================================
        // Phase 4d: Abstention + evidence + preference gates
        // ===================================================================

        // --- Gate 2: Preference done-gate ---
        if self.strategy == QuestionStrategy::Preference
            && !is_abstention
        {
            let min = self.gates.preference_min_retrievals;
            if (flags.retrieval_call_count as usize) < min {
                eprintln!(
                    "[AGENT] Preference gate: only {} retrievals (min {}), rejecting at iteration {}",
                    flags.retrieval_call_count, min, iteration + 1
                );
                return Err(format!(
                    "REJECTED: For preference/advice questions, you must search thoroughly (at least {} retrieval calls) to find personal context. Search for the user's specific experiences, preferences, and past activities.",
                    min
                ));
            }
        }

        // --- Gate 15: Abstention gate ---
        // P11: Observe-mode abstention logging
        if is_abstention {
            let retrieval_text = collect_retrieval_results(state.messages);
            let retrieval_lower = retrieval_text.to_lowercase();
            let q_keywords = extract_search_keywords(&self.question_text);
            let found_keywords: Vec<&str> = q_keywords
                .iter()
                .filter(|kw| retrieval_lower.contains(kw.as_str()))
                .map(|s| s.as_str())
                .collect();
            eprintln!(
                "[P11-OBSERVE] abstention | strategy={:?} | retrievals={} | kw_in_retrieval={}/{} ({}) | gates: abstention={} anti={}",
                self.strategy, flags.retrieval_call_count,
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
            let q_words: Vec<&str> = self
                .question_text
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
        if is_abstention
            && !flags.anti_abstention_used
            && !self.skip_anti_abstention
            && self.gates.anti_abstention_keyword_threshold > 0
        {
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

        // --- Gate 16b: Preference-specific anti-abstention (lower threshold) ---
        if is_abstention
            && !flags.anti_abstention_used
            && self.strategy == QuestionStrategy::Preference
            && !self.skip_anti_abstention
            && self.gates.preference_keyword_threshold > 0
        {
            let retrieval_text = collect_retrieval_results(state.messages);
            let retrieval_lower = retrieval_text.to_lowercase();
            let q_keywords = extract_search_keywords(&self.question_text);
            let found_keywords: Vec<&str> = q_keywords
                .iter()
                .filter(|kw| retrieval_lower.contains(kw.as_str()))
                .map(|s| s.as_str())
                .collect();
            if found_keywords.len() >= self.gates.preference_keyword_threshold {
                flags.anti_abstention_used = true;
                eprintln!(
                    "[P14-PREF] Rejecting preference abstention | kw={}/{} ({})",
                    found_keywords.len(),
                    q_keywords.len(),
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
            && !matches!(
                self.strategy,
                QuestionStrategy::Enumeration | QuestionStrategy::Preference
            )
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
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a proposed answer is an abstention (agent says "I don't know").
fn is_prompt_abstention(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    // Normalize Unicode curly apostrophes (GPT-5.2 outputs U+2019 ~56% of the time)
    let normalized = lower.replace('\u{2019}', "'");
    normalized.contains("don't have enough information")
        || normalized.contains("i don't have")
        || normalized.contains("i couldn't find")
        || normalized.contains("not enough information")
        || normalized.contains("no information found")
        || normalized.contains("i was unable to find")
        || normalized.contains("i cannot find")
        || normalized.contains("i could not find")
}

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
                    if found.is_empty() {
                        inner.to_string()
                    } else {
                        found
                    }
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
            topic_keywords
                .iter()
                .filter(|kw| lower.contains(kw.as_str()))
                .count()
                >= 2
        })
        .collect();

    if relevant.is_empty() {
        return None;
    }

    let (latest_date, latest_content) =
        relevant.into_iter().max_by_key(|(date, _)| date.clone())?;

    let snippet = if latest_content.chars().count() > 500 {
        format!(
            "{}...",
            latest_content.chars().take(500).collect::<String>()
        )
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
    let dual_event_markers = [
        "between",
        "when i",
        "when my",
        "before i",
        "before my",
        "after i",
        "after my",
        "since i",
        "since my",
        "ago did",
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
            let item = text[pos..end]
                .trim()
                .trim_end_matches(',')
                .trim_end_matches(';')
                .trim();
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
    without_citation
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Conservative duplicate check: exact match after normalization.
fn items_likely_duplicate(a: &str, b: &str) -> bool {
    normalize_item(a) == normalize_item(b)
}

/// Deduplicate enumerated items conservatively.
fn deduplicate_items(items: &[String]) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for item in items {
        let is_dup = unique
            .iter()
            .any(|existing| items_likely_duplicate(existing, item));
        if !is_dup {
            unique.push(item.clone());
        }
    }
    unique
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
