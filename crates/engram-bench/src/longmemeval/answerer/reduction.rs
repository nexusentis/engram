//! Post-loop count and sum reduction logic.

use super::strategy::{is_counting_question, is_sum_question};

/// Extract items from a numbered/enumerated answer.
/// Handles formats like "1) item", "1. item", "1: item" (line-start)
/// and inline formats like "Items: 1) item one, 2) item two, 3) item three".
fn extract_enumerated_items(answer: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in answer.lines() {
        let trimmed = line.trim();
        // Match line-start patterns: "N) ...", "N. ...", "N: ..."
        if let Some(rest) = trimmed
            .strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|s| {
                // Consume additional digits
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

    // If no line-start items found, try inline format:
    // "Items: 1) item one, 2) item two" or "3 items: 1) foo 2) bar 3) baz"
    if items.is_empty() {
        items = extract_inline_enumerated_items(answer);
    }

    items
}

/// Extract items from inline enumeration format.
/// Handles "... 1) item one, 2) item two, 3) item three ..." on a single line
/// or across the text without line-start formatting.
fn extract_inline_enumerated_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    // Find all positions of "N)" patterns (1-3 digit number followed by ")")
    let bytes = text.as_bytes();
    let mut positions: Vec<usize> = Vec::new();

    let mut i = 0;
    while i < bytes.len() {
        // Look for digit followed by more optional digits then ')'
        if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b')' {
                // Check that this looks like an enumeration marker:
                // either at start of text, or preceded by whitespace/comma/semicolon
                let is_start = start == 0
                    || matches!(
                        bytes[start - 1],
                        b' ' | b'\t' | b'\n' | b',' | b';' | b':'
                    );
                if is_start {
                    positions.push(i + 1); // position right after the ')'
                }
            }
        }
        i += 1;
    }

    // Need at least 2 numbered items to consider this an inline list
    if positions.len() >= 2 {
        for (idx, &pos) in positions.iter().enumerate() {
            let end = if idx + 1 < positions.len() {
                // Find the start of next "N)" marker — backtrack from next position
                // to find where the digit(s) start
                let next_after_paren = positions[idx + 1];
                // next_after_paren points after ')' of next marker
                // Walk back to find start of the digit(s)
                let mut marker_start = next_after_paren - 2; // at least the ')' and one digit
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
/// Lowercases, strips parenthetical session citations, collapses whitespace.
fn normalize_item(s: &str) -> String {
    let lower = s.to_lowercase();
    // Remove session citations like "(from session on ...)" or "(session ...)"
    let without_citation = if let Some(pos) = lower.find("(from session") {
        &lower[..pos]
    } else if let Some(pos) = lower.find("(session") {
        &lower[..pos]
    } else {
        &lower
    };
    // Collapse whitespace
    without_citation
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Conservative duplicate check: exact match after normalization.
fn items_likely_duplicate(a: &str, b: &str) -> bool {
    let norm_a = normalize_item(a);
    let norm_b = normalize_item(b);
    norm_a == norm_b
}

/// Deduplicate enumerated items conservatively (exact normalized match only).
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
/// Handles "$1,234.56", "$1234", "$1,234" formats.
pub(super) fn extract_dollar_amounts(text: &str) -> Vec<f64> {
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
            // Remove commas and parse
            let clean: String = num_str.replace(',', "");
            if let Ok(val) = clean.parse::<f64>() {
                amounts.push(val);
            }
        }
    }
    amounts
}

/// Log entry for a post-loop reduction action.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ReductionLog {
    pub(super) reducer: &'static str,
    pub(super) original_answer: String,
    pub(super) reduced_answer: String,
    pub(super) claimed_count: Option<usize>,
    pub(super) listed_count: Option<usize>,
    pub(super) deduped_count: Option<usize>,
    pub(super) confidence: f32,
    pub(super) action: &'static str,
    pub(super) reason: String,
}

/// Extract the claimed count from an answer string.
/// Looks for patterns like "3 items", "found 3", leading digit, "there are 3", etc.
/// Only searches the PREAMBLE (text before the first enumerated item) to avoid
/// picking up numbers from inside list items.
/// Returns None if no clear count is found.
fn extract_claimed_count(answer: &str) -> Option<usize> {
    // Restrict search to text before the first enumerated item marker
    let preamble = extract_preamble(answer);
    let lower = preamble.to_lowercase();

    // P15b: Reject if preamble looks like a currency amount or measurement
    // These are never count claims
    let trimmed_lower = lower.trim();
    if trimmed_lower.starts_with('$') || trimmed_lower.ends_with("hours") {
        return None;
    }

    // Pattern 1: "N <noun>" — match any word after a number (not just hardcoded nouns)
    // Use the preamble's first "number + word" pair, with rejection filters
    // Strategy: find all candidate "number word" pairs, reject bad ones, return first good one
    let words: Vec<&str> = lower.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        // Try to parse this word as a number (digit or word-form)
        let num = if let Ok(n) = word.parse::<usize>() {
            Some(n)
        } else {
            word_to_number(word)
        };

        if let Some(n) = num {
            if n > 100 {
                continue;
            }
            // Rejection filters: skip if this number looks like a non-count context
            // R1: Currency — preceded by "$" or part of "$N"
            if word.starts_with('$') {
                continue;
            }
            if i > 0 && words[i - 1].ends_with('$') {
                continue;
            }
            // R2: Date-like — "april 17", "may 10", month names before/after
            let date_words = [
                "january", "february", "march", "april", "may", "june",
                "july", "august", "september", "october", "november", "december",
            ];
            if i > 0 && date_words.contains(&words[i - 1]) {
                continue;
            }
            if i + 1 < words.len() && date_words.contains(&words[i + 1]) {
                continue;
            }
            // R3: Measurement unit follows — "20-gallon", "1-gallon", etc.
            if i + 1 < words.len() {
                let next = words[i + 1];
                if next.contains("gallon") || next.contains("inch")
                    || next.contains("pound") || next.contains("mile")
                    || next.contains("hour") || next.contains("minute")
                    || next.contains("year") || next.contains("month")
                    || next.contains("week") || next.contains("percent")
                    || next.starts_with("am") || next.starts_with("pm")
                {
                    continue;
                }
            }
            // R4: Hyphenated compound (e.g., "20-gallon") — skip if word contains hyphen + letters
            if word.contains('-') && word.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            // R5: Decimal number (e.g., "0.5") — not a count
            if word.contains('.') && word.chars().filter(|c| *c == '.').count() == 1 {
                if let Ok(_f) = word.parse::<f64>() {
                    continue;
                }
            }
            // R6: Check context around the number
            if i + 1 < words.len() {
                let next_raw = words[i + 1];
                let next = next_raw.trim_end_matches(|c: char| c.is_ascii_punctuation());

                // Duration filter: "for N years/months" — skip only when preceded by duration preposition
                let duration_units = ["year", "month", "week", "hour", "minute"];
                if i > 0 {
                    let prev = words[i - 1];
                    let is_duration_prep = prev == "for" || prev == "about"
                        || prev == "approximately" || prev == "around"
                        || prev == "over" || prev == "under" || prev == "within";
                    if is_duration_prep && duration_units.iter().any(|u| next.contains(u)) {
                        continue;
                    }
                }

                // Accept: the next word (stripped of punctuation) is a plural noun
                if next.ends_with('s') || next.ends_with("ies") {
                    return Some(n);
                }
                // Accept: compound noun — check word after next (e.g., "2 dinner parties")
                if i + 2 < words.len() {
                    let next2 = words[i + 2].trim_end_matches(|c: char| c.is_ascii_punctuation());
                    if next2.ends_with('s') || next2.ends_with("ies") {
                        return Some(n);
                    }
                }
                // Accept: singular noun when count is 1
                if n <= 1 && !next.is_empty() && next.chars().all(|c| c.is_alphabetic()) {
                    return Some(n);
                }
            }
            // Accept: number at start of preamble followed by punctuation (e.g., "4.")
            if i == 0 {
                let raw_word = preamble.trim().split_whitespace().next().unwrap_or("");
                if raw_word.ends_with('.') || raw_word.ends_with(',') || raw_word.ends_with(':') {
                    return Some(n);
                }
            }
        }
    }

    // Pattern 2: "there are/were N", "found N", "identified N", "a total of N"
    let prefix_patterns = [
        "there are ",
        "there were ",
        "there have been ",
        "i found ",
        "i identified ",
        "i recall ",
        "i remember ",
        "a total of ",
        "total of ",
        "you have ",
        "you own ",
        "you currently have ",
        "you currently own ",
        "i have ",
    ];
    for pat in &prefix_patterns {
        if let Some(pos) = lower.find(pat) {
            let after = &lower[pos + pat.len()..];
            if let Some(num) = extract_leading_number(after) {
                return Some(num);
            }
        }
    }

    // Pattern 3: answer starts with a number (digit or word-form) on its own line
    let trimmed = preamble.trim();
    if let Some(first_line) = trimmed.lines().next() {
        let first_line = first_line.trim();
        if let Ok(n) = first_line.parse::<usize>() {
            if n > 0 && n <= 100 {
                return Some(n);
            }
        }
        // P15b: word-form on its own line (e.g., "Three")
        if let Some(n) = word_to_number(first_line) {
            if n > 0 && n <= 100 {
                return Some(n);
            }
        }
    }

    None
}

/// Extract the preamble of an answer — the text before the first enumerated item.
/// This is used to restrict claimed-count extraction to avoid grabbing numbers
/// from inside list items.
fn extract_preamble(answer: &str) -> &str {
    // P15b: Also detect inline "Items: 1)" or "Items:\n1)" patterns
    if let Some(items_pos) = answer.to_lowercase().find("items:") {
        // Check if "1)" follows shortly after "Items:"
        let after_items = &answer[items_pos + 6..];
        let after_trimmed = after_items.trim_start();
        if after_trimmed.starts_with("1)") || after_trimmed.starts_with("1.") {
            return &answer[..items_pos];
        }
    }

    // Find first line-start enumeration marker: "N) ", "N. ", "N: "
    for (line_start, line) in answer.match_indices('\n') {
        let trimmed = line.trim_start_matches('\n').trim();
        if trimmed
            .strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|s| {
                let s = s.trim_start_matches(|c: char| c.is_ascii_digit());
                s.strip_prefix(')')
                    .or_else(|| s.strip_prefix('.'))
                    .or_else(|| s.strip_prefix(':'))
            })
            .is_some()
        {
            return &answer[..line_start];
        }
    }
    // Check if the very first line is an enumeration marker
    let trimmed = answer.trim();
    if trimmed
        .strip_prefix(|c: char| c.is_ascii_digit())
        .and_then(|s| {
            let s = s.trim_start_matches(|c: char| c.is_ascii_digit());
            s.strip_prefix(')')
                .or_else(|| s.strip_prefix('.'))
                .or_else(|| s.strip_prefix(':'))
        })
        .is_some()
    {
        return ""; // entire answer is a list
    }
    // No enumeration found — preamble is the whole answer
    answer
}

/// P15b: Convert word-form numbers to digits (zero-twenty).
/// Excludes ambiguous words like "a", "an", "no" which are too common in prose.
fn word_to_number(word: &str) -> Option<usize> {
    match word.to_lowercase().as_str() {
        "zero" => Some(0),
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "thirteen" => Some(13),
        "fourteen" => Some(14),
        "fifteen" => Some(15),
        "sixteen" => Some(16),
        "seventeen" => Some(17),
        "eighteen" => Some(18),
        "nineteen" => Some(19),
        "twenty" => Some(20),
        _ => None,
    }
}

/// Extract a leading number (digit or word-form) from a string.
/// e.g., "3 items" -> 3, "two tanks" -> 2
fn extract_leading_number(s: &str) -> Option<usize> {
    let s = s.trim();
    // First try: digit at start
    let end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());
    if end > 0 {
        if let Some(n) = s[..end].parse::<usize>().ok().filter(|&n| n <= 100) {
            return Some(n);
        }
    }
    // P15b: Try word-form number as first word
    if let Some(first_word) = s.split_whitespace().next() {
        if let Some(n) = word_to_number(first_word) {
            if n <= 100 {
                return Some(n);
            }
        }
    }
    None
}

/// Post-loop count reducer.
///
/// Compares the agent's claimed count against its own itemized evidence.
/// Only corrects when high-confidence: both are clearly parseable and they disagree.
/// For sum questions, recomputes arithmetic from extracted dollar amounts.
///
/// Returns the (possibly corrected) answer and an optional log entry.
pub(super) fn reduce_count(answer: &str, question: &str) -> (String, Option<ReductionLog>) {
    // 1. Only apply to counting/enumeration/sum questions
    if !is_counting_question(question) && !is_sum_question(question) {
        return (answer.to_string(), None);
    }

    // 2. Extract itemized list from answer
    let items = extract_enumerated_items(answer);
    if items.is_empty() {
        return (
            answer.to_string(),
            Some(ReductionLog {
                reducer: "count",
                original_answer: answer.to_string(),
                reduced_answer: answer.to_string(),
                claimed_count: extract_claimed_count(answer),
                listed_count: Some(0),
                deduped_count: Some(0),
                confidence: 0.0,
                action: "no-op",
                reason: "no enumerated items found in answer".to_string(),
            }),
        );
    }

    // 3. Deduplicate items
    let deduped = deduplicate_items(&items);

    // 4. For sum questions: check arithmetic
    if is_sum_question(question) {
        return reduce_sum(answer, &items, &deduped);
    }

    // 5. For count questions: compare claimed vs listed
    let claimed = match extract_claimed_count(answer) {
        Some(c) => c,
        None => {
            return (
                answer.to_string(),
                Some(ReductionLog {
                    reducer: "count",
                    original_answer: answer.to_string(),
                    reduced_answer: answer.to_string(),
                    claimed_count: None,
                    listed_count: Some(items.len()),
                    deduped_count: Some(deduped.len()),
                    confidence: 0.0,
                    action: "no-op",
                    reason: "could not extract claimed count from answer".to_string(),
                }),
            );
        }
    };

    let listed = deduped.len();

    if claimed == listed {
        return (
            answer.to_string(),
            Some(ReductionLog {
                reducer: "count",
                original_answer: answer.to_string(),
                reduced_answer: answer.to_string(),
                claimed_count: Some(claimed),
                listed_count: Some(items.len()),
                deduped_count: Some(listed),
                confidence: 1.0,
                action: "consistent",
                reason: format!("claimed {} == listed {} (consistent)", claimed, listed),
            }),
        );
    }

    let delta = (claimed as isize - listed as isize).unsigned_abs();

    // Safety: large delta suggests parsing failure, not agent error
    if delta > 2 {
        return (
            answer.to_string(),
            Some(ReductionLog {
                reducer: "count",
                original_answer: answer.to_string(),
                reduced_answer: answer.to_string(),
                claimed_count: Some(claimed),
                listed_count: Some(items.len()),
                deduped_count: Some(listed),
                confidence: 0.3,
                action: "skipped",
                reason: format!(
                    "delta {} too large (claimed={}, listed={}), likely parsing error",
                    delta, claimed, listed
                ),
            }),
        );
    }

    // P15: Promote confidence for delta=1 corrections (proven robust in P10 observe mode)
    let confidence: f32 = if delta == 1 { 0.98 } else { 0.6 };

    // Correct the answer: replace claimed count with listed count
    let corrected = correct_count_in_answer(answer, claimed, listed);

    (
        corrected.clone(),
        Some(ReductionLog {
            reducer: "count",
            original_answer: answer.to_string(),
            reduced_answer: corrected,
            claimed_count: Some(claimed),
            listed_count: Some(items.len()),
            deduped_count: Some(listed),
            confidence,
            action: "corrected",
            reason: format!(
                "claimed {} but listed {} items (delta={})",
                claimed, listed, delta
            ),
        }),
    )
}

/// Reduce sum questions: recompute total from itemized dollar amounts.
/// Uses raw (non-deduped) items because duplicate line items can be legitimate
/// separate expenses (e.g., two $50 meals on different days).
fn reduce_sum(answer: &str, items: &[String], deduped: &[String]) -> (String, Option<ReductionLog>) {
    // Extract dollar amounts from each RAW item (not deduped — duplicates may be real expenses)
    let mut item_amounts: Vec<f64> = Vec::new();
    for item in items {
        let amounts = extract_dollar_amounts(item);
        // Only take the last dollar amount per item to avoid grabbing intermediate values
        if let Some(&last) = amounts.last() {
            item_amounts.push(last);
        }
    }

    if item_amounts.is_empty() {
        return (
            answer.to_string(),
            Some(ReductionLog {
                reducer: "sum",
                original_answer: answer.to_string(),
                reduced_answer: answer.to_string(),
                claimed_count: None,
                listed_count: Some(items.len()),
                deduped_count: Some(deduped.len()),
                confidence: 0.0,
                action: "no-op",
                reason: "no dollar amounts found in items".to_string(),
            }),
        );
    }

    let computed_sum: f64 = item_amounts.iter().sum();

    // Extract the stated total from the answer
    // Look for "total" as a whole word (not "subtotal") followed by a dollar amount
    let lower = answer.to_lowercase();
    let stated_total = find_word_total_amount(&lower, answer).or_else(|| {
        // Fallback: try the last dollar amount in the answer as the stated total
        let all_amounts = extract_dollar_amounts(answer);
        all_amounts.last().copied()
    });

    let stated = match stated_total {
        Some(t) => t,
        None => {
            return (
                answer.to_string(),
                Some(ReductionLog {
                    reducer: "sum",
                    original_answer: answer.to_string(),
                    reduced_answer: answer.to_string(),
                    claimed_count: None,
                    listed_count: Some(items.len()),
                    deduped_count: Some(deduped.len()),
                    confidence: 0.0,
                    action: "no-op",
                    reason: "could not extract stated total from answer".to_string(),
                }),
            );
        }
    };

    if (stated - computed_sum).abs() < 0.01 {
        return (
            answer.to_string(),
            Some(ReductionLog {
                reducer: "sum",
                original_answer: answer.to_string(),
                reduced_answer: answer.to_string(),
                claimed_count: None,
                listed_count: Some(items.len()),
                deduped_count: Some(deduped.len()),
                confidence: 1.0,
                action: "consistent",
                reason: format!(
                    "stated ${:.2} == computed ${:.2} (consistent)",
                    stated, computed_sum
                ),
            }),
        );
    }

    // Correct: replace stated total with computed sum
    let corrected = correct_dollar_in_answer(answer, stated, computed_sum);

    (
        corrected.clone(),
        Some(ReductionLog {
            reducer: "sum",
            original_answer: answer.to_string(),
            reduced_answer: corrected,
            claimed_count: None,
            listed_count: Some(items.len()),
            deduped_count: Some(deduped.len()),
            confidence: 0.95, // P15: promote to enforcement threshold (proven in P10 observe mode)
            action: "corrected",
            reason: format!(
                "stated ${:.2} but items sum to ${:.2}",
                stated, computed_sum
            ),
        }),
    )
}

/// P15b: Convert a number to its word-form (for replacement in prose).
fn number_to_word(n: usize) -> Option<&'static str> {
    match n {
        0 => Some("zero"),
        1 => Some("one"),
        2 => Some("two"),
        3 => Some("three"),
        4 => Some("four"),
        5 => Some("five"),
        6 => Some("six"),
        7 => Some("seven"),
        8 => Some("eight"),
        9 => Some("nine"),
        10 => Some("ten"),
        11 => Some("eleven"),
        12 => Some("twelve"),
        13 => Some("thirteen"),
        14 => Some("fourteen"),
        15 => Some("fifteen"),
        16 => Some("sixteen"),
        17 => Some("seventeen"),
        18 => Some("eighteen"),
        19 => Some("nineteen"),
        20 => Some("twenty"),
        _ => None,
    }
}

/// Replace a claimed count number with the correct count in the answer text.
/// Only replaces within the preamble (before enumerated items) to avoid
/// corrupting list numbering or other data.
/// P15b: Also handles word-form numbers (e.g., "two" -> "three").
fn correct_count_in_answer(answer: &str, old_count: usize, new_count: usize) -> String {
    let new_str = new_count.to_string();
    let preamble_len = extract_preamble(answer).len();

    // Try digit replacement first
    let old_str = old_count.to_string();
    let bytes = answer.as_bytes();
    for (i, _) in answer.match_indices(&old_str) {
        if i + old_str.len() > preamble_len {
            break; // past preamble, stop
        }
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
        let after_ok = i + old_str.len() >= bytes.len()
            || !bytes[i + old_str.len()].is_ascii_digit();
        if before_ok && after_ok {
            let mut result = String::with_capacity(answer.len());
            result.push_str(&answer[..i]);
            result.push_str(&new_str);
            result.push_str(&answer[i + old_str.len()..]);
            return result;
        }
    }

    // P15b: Try word-form replacement (e.g., "two" -> "3", "Three" -> "4")
    if let Some(old_word) = number_to_word(old_count) {
        let preamble = &answer[..preamble_len];
        let preamble_lower = preamble.to_lowercase();
        if let Some(pos) = preamble_lower.find(old_word) {
            // Verify word boundary
            let before_ok = pos == 0
                || !preamble.as_bytes()[pos - 1].is_ascii_alphabetic();
            let end = pos + old_word.len();
            let after_ok = end >= preamble.len()
                || !preamble.as_bytes()[end].is_ascii_alphabetic();
            if before_ok && after_ok {
                let mut result = String::with_capacity(answer.len());
                result.push_str(&answer[..pos]);
                result.push_str(&new_str);
                result.push_str(&answer[end..]);
                return result;
            }
        }
    }

    // Fallback: no replacement possible
    answer.to_string()
}

/// Find a dollar amount near the word "total" (as a whole word, not "subtotal").
/// Returns the first dollar amount found after a whole-word "total" match.
fn find_word_total_amount(lower: &str, original: &str) -> Option<f64> {
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("total") {
        let abs_pos = search_from + pos;
        // Check word boundary before "total" — must not be preceded by a letter
        let before_ok = abs_pos == 0
            || !lower.as_bytes()[abs_pos - 1].is_ascii_alphabetic();
        if before_ok {
            let after_total = &original[abs_pos..];
            let totals = extract_dollar_amounts(after_total);
            if let Some(&first) = totals.first() {
                return Some(first);
            }
        }
        search_from = abs_pos + 5; // skip past "total"
        if search_from >= lower.len() {
            break;
        }
    }
    None
}

/// Replace a dollar amount in the answer text.
/// Anchors replacement near the "total" keyword to avoid replacing item amounts.
fn correct_dollar_in_answer(answer: &str, old_amount: f64, new_amount: f64) -> String {
    let old_with_comma = format_dollar_with_commas(old_amount);
    let new_with_comma = format_dollar_with_commas(new_amount);
    let old_formatted = if old_amount.fract().abs() < 0.001 {
        format!("${}", old_amount as i64)
    } else {
        format!("${:.2}", old_amount)
    };

    // Try to find and replace near "total" keyword first
    let lower = answer.to_lowercase();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("total") {
        let abs_pos = search_from + pos;
        let before_ok = abs_pos == 0
            || !lower.as_bytes()[abs_pos - 1].is_ascii_alphabetic();
        if before_ok {
            // Look for the old amount in the region after "total"
            let region = &answer[abs_pos..];
            if let Some(dollar_offset) = region.find(&old_with_comma) {
                let global_pos = abs_pos + dollar_offset;
                let mut result = String::with_capacity(answer.len());
                result.push_str(&answer[..global_pos]);
                result.push_str(&new_with_comma);
                result.push_str(&answer[global_pos + old_with_comma.len()..]);
                return result;
            }
            if let Some(dollar_offset) = region.find(&old_formatted) {
                let global_pos = abs_pos + dollar_offset;
                let mut result = String::with_capacity(answer.len());
                result.push_str(&answer[..global_pos]);
                result.push_str(&new_with_comma);
                result.push_str(&answer[global_pos + old_formatted.len()..]);
                return result;
            }
        }
        search_from = abs_pos + 5;
        if search_from >= lower.len() {
            break;
        }
    }

    // Fallback: replace last occurrence of old amount (most likely the total)
    let mut result = answer.to_string();
    if let Some(rpos) = result.rfind(&old_with_comma) {
        result = format!(
            "{}{}{}",
            &answer[..rpos],
            new_with_comma,
            &answer[rpos + old_with_comma.len()..]
        );
    } else if let Some(rpos) = result.rfind(&old_formatted) {
        result = format!(
            "{}{}{}",
            &answer[..rpos],
            new_with_comma,
            &answer[rpos + old_formatted.len()..]
        );
    }
    result
}

/// Format a dollar amount with commas (e.g., 8750.0 -> "$8,750").
pub(super) fn format_dollar_with_commas(amount: f64) -> String {
    let is_whole = amount.fract().abs() < 0.001;
    if is_whole {
        let n = amount as i64;
        let s = n.to_string();
        let mut result = String::new();
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 && c != '-' {
                result.push(',');
            }
            result.push(c);
        }
        format!("${}", result.chars().rev().collect::<String>())
    } else {
        format!("${:.2}", amount)
    }
}

// ── Test helpers: expose private functions for tests.rs ──

#[cfg(test)]
pub(super) fn extract_enumerated_items_for_test(answer: &str) -> Vec<String> {
    extract_enumerated_items(answer)
}

#[cfg(test)]
pub(super) fn extract_inline_enumerated_items_for_test(text: &str) -> Vec<String> {
    extract_inline_enumerated_items(text)
}

#[cfg(test)]
pub(super) fn normalize_item_for_test(s: &str) -> String {
    normalize_item(s)
}

#[cfg(test)]
pub(super) fn items_likely_duplicate_for_test(a: &str, b: &str) -> bool {
    items_likely_duplicate(a, b)
}

#[cfg(test)]
pub(super) fn deduplicate_items_for_test(items: &[String]) -> Vec<String> {
    deduplicate_items(items)
}

#[cfg(test)]
pub(super) fn extract_claimed_count_for_test(answer: &str) -> Option<usize> {
    extract_claimed_count(answer)
}

#[cfg(test)]
pub(super) fn extract_preamble_for_test(answer: &str) -> &str {
    extract_preamble(answer)
}

#[cfg(test)]
pub(super) fn word_to_number_for_test(word: &str) -> Option<usize> {
    word_to_number(word)
}

#[cfg(test)]
pub(super) fn correct_count_in_answer_for_test(answer: &str, old_count: usize, new_count: usize) -> String {
    correct_count_in_answer(answer, old_count, new_count)
}

#[cfg(test)]
pub(super) fn find_word_total_amount_for_test(lower: &str, original: &str) -> Option<f64> {
    find_word_total_amount(lower, original)
}

#[cfg(test)]
pub(super) fn format_dollar_with_commas_for_test(amount: f64) -> String {
    format_dollar_with_commas(amount)
}
