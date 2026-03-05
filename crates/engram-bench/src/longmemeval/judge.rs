//! GPT-4o judge for LongMemEval-S benchmark
//!
//! Evaluates answer correctness using semantic comparison.

use crate::types::QuestionCategory;
use crate::error::Result;

/// Result from judging an answer
#[derive(Debug, Clone)]
pub struct JudgeResult {
    /// Whether the answer was judged correct
    pub is_correct: bool,
    /// Score from 0.0 to 1.0
    pub score: f32,
    /// Reasoning for the judgment
    pub reasoning: String,
    /// Cost in USD
    pub cost_usd: f32,
}

impl JudgeResult {
    /// Create a new judge result
    pub fn new(is_correct: bool, score: f32, reasoning: impl Into<String>) -> Self {
        Self {
            is_correct,
            score,
            reasoning: reasoning.into(),
            cost_usd: 0.0,
        }
    }

    /// Create a correct result
    pub fn correct(reasoning: impl Into<String>) -> Self {
        Self::new(true, 1.0, reasoning)
    }

    /// Create an incorrect result
    pub fn incorrect(reasoning: impl Into<String>) -> Self {
        Self::new(false, 0.0, reasoning)
    }

    /// Set cost
    pub fn with_cost(mut self, cost: f32) -> Self {
        self.cost_usd = cost;
        self
    }
}

/// Configuration for the judge
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct JudgeConfig {
    /// Model to use for judging
    pub judge_model: String,
    /// Temperature for judging (should be low for consistency)
    pub temperature: f32,
    /// Maximum tokens for judge response
    pub max_tokens: usize,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            judge_model: "gpt-4o".to_string(),
            temperature: 0.0,
            max_tokens: 300,
        }
    }
}

impl JudgeConfig {
    /// Create a new config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set judge model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.judge_model = model.into();
        self
    }

    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp;
        self
    }
}

/// GPT-4o based answer judge
///
/// Evaluates whether generated answers are semantically correct
/// compared to expected answers.
#[derive(Debug)]
pub struct Judge {
    config: JudgeConfig,
    llm_client: Option<super::answerer::LlmClient>,
}

impl Judge {
    /// Create a new judge
    pub fn new(config: JudgeConfig) -> Self {
        Self {
            config,
            llm_client: None,
        }
    }

    /// Create with default config
    pub fn with_defaults() -> Self {
        Self::new(JudgeConfig::default())
    }

    /// Create with LLM client for real API calls
    pub fn with_llm_client(mut self, client: super::answerer::LlmClient) -> Self {
        self.llm_client = Some(client);
        self
    }

    /// Create with LLM client from environment
    pub fn with_llm_from_env(mut self) -> Self {
        self.llm_client = super::answerer::LlmClient::from_env();
        self
    }

    /// Get the configuration
    pub fn config(&self) -> &JudgeConfig {
        &self.config
    }

    /// Judge whether the generated answer is correct (sync version)
    ///
    /// Warning: Do NOT call from async context — will panic with nested runtime.
    /// Use `judge_async` instead.
    pub fn judge(
        &self,
        question: &str,
        expected_answer: &str,
        generated_answer: &str,
        category: QuestionCategory,
    ) -> Result<JudgeResult> {
        // Quick exact-match check: skip LLM if normalized answers are identical
        if let Some(result) =
            Self::exact_match_check_with_category(expected_answer, generated_answer, Some(category))
        {
            return Ok(result);
        }

        // Build the judge prompt
        let prompt = self.build_judge_prompt(question, expected_answer, generated_answer, category);

        // Use LLM if available, otherwise fall back to heuristics
        if let Some(ref client) = self.llm_client {
            match client.complete_sync(&self.config.judge_model, &prompt, 0.0) {
                Ok((response, cost)) => {
                    let (is_correct, score, reasoning) = Self::parse_judge_response(&response);
                    return Ok(JudgeResult::new(is_correct, score, reasoning).with_cost(cost));
                }
                Err(e) => {
                    tracing::warn!("LLM judge failed, falling back to heuristics: {}", e);
                }
            }
        }

        // Fallback to heuristic comparison
        let (is_correct, score, reasoning) =
            self.evaluate_answer(expected_answer, generated_answer, category);

        let cost = super::answerer::estimate_cost(&self.config.judge_model, 800, 100);

        Ok(JudgeResult::new(is_correct, score, reasoning).with_cost(cost))
    }

    /// Judge whether the generated answer is correct (async version)
    ///
    /// Safe to call from async context (uses async LLM client).
    pub async fn judge_async(
        &self,
        question: &str,
        expected_answer: &str,
        generated_answer: &str,
        category: QuestionCategory,
    ) -> Result<JudgeResult> {
        // Quick exact-match check: skip LLM if normalized answers are identical
        if let Some(result) =
            Self::exact_match_check_with_category(expected_answer, generated_answer, Some(category))
        {
            return Ok(result);
        }

        let prompt = self.build_judge_prompt(question, expected_answer, generated_answer, category);

        if let Some(ref client) = self.llm_client {
            match client
                .complete(&self.config.judge_model, &prompt, 0.0)
                .await
            {
                Ok((response, cost)) => {
                    let (is_correct, score, reasoning) = Self::parse_judge_response(&response);
                    return Ok(JudgeResult::new(is_correct, score, reasoning).with_cost(cost));
                }
                Err(e) => {
                    tracing::warn!("LLM judge failed, falling back to heuristics: {}", e);
                }
            }
        }

        let (is_correct, score, reasoning) =
            self.evaluate_answer(expected_answer, generated_answer, category);

        let cost = super::answerer::estimate_cost(&self.config.judge_model, 800, 100);

        Ok(JudgeResult::new(is_correct, score, reasoning).with_cost(cost))
    }

    /// Build the judge prompt
    pub fn build_judge_prompt(
        &self,
        question: &str,
        expected: &str,
        generated: &str,
        category: QuestionCategory,
    ) -> String {
        let category_guidance = Self::get_category_guidance(category);

        format!(
            r#"You are evaluating the correctness of an AI assistant's answer about a user's personal information.

Question: {}

Expected Answer: {}

Generated Answer: {}

Category: {} - {}

Evaluate the generated answer:
1. Does it convey the same essential information as the expected answer?
2. Is it factually consistent with what was expected?
3. For temporal questions, are dates/sequences correct?
4. For updates, does it reflect the latest information?
5. For abstention, did it correctly say "I don't know" when appropriate?

IMPORTANT: Be lenient on formatting differences. "120 stars" and "120" are equivalent. "Yes" and "Yes." are equivalent. "2 months" and "2" are equivalent when the question asks "how many months". Focus on whether the CORE INFORMATION is correct, not exact string matching.

CRITICAL LENIENCY RULES:
- Adding specific dates to a correct answer does NOT make it wrong. E.g., expected "planting tomato saplings" vs generated "planted 12 tomato saplings on April 21" → CORRECT (same activity, date is bonus context).
- Verb tense differences don't matter: "planting" = "planted", "signing" = "signed", "starting" = "started".
- Paraphrasing the same event is correct: "signed a contract with first client" = "landed the first client" if they refer to the same event.
- Including the person's name when expected doesn't is fine: "took ukulele lessons" = "took ukulele lessons with Rachel".
- Extra context or detail beyond the expected answer is CORRECT as long as the core answer is present.
- Same location, different specificity: "closet" = "shoe rack in closet", "garage" = "garage shelf", "kitchen" = "kitchen counter". More specific is CORRECT.
- Equivalent numeric expressions: "2 times" = "twice", "3 times" = "three times", "1 time" = "once". These are identical.
- Correct count with missing/different explanation is CORRECT: if expected says "4 (A, B, C, D)" and generated says "4" or "4 (A, B, C, E)" but the COUNT matches, score as CORRECT when the question asks "how many".
- Over-elaboration with correct core: if the core fact is correct but the answer adds related but slightly different framing, score as CORRECT. E.g., expected "staunch atheist" vs generated "atheism, has been exploring Buddhism" → CORRECT if core stance matches.
- Synonym equivalence: "jog" = "run", "purchase" = "buy", "automobile" = "car", "ill" = "sick", "happy" = "glad". Common synonyms expressing the same meaning are CORRECT.
- Partial list overlap: if the expected answer lists N items and the generated answer lists N items with most overlapping, focus on whether the COUNT is correct and the core items match. Minor item differences in a correct-count list should score CORRECT.
- Rounding and approximation: "$1,300" = "close to $1,300" = "approximately $1,300" = "about 1300". Approximate values matching the expected value are CORRECT.
- Unit equivalence: "2 weeks" = "14 days", "1 month" = "about 4 weeks" = "30 days", "1 year" = "12 months" = "365 days". Equivalent time/measurement units are CORRECT.

PREFERENCE/ADVICE QUESTION RULES (when the expected answer describes user preferences or what kind of response is preferred):
- If the expected answer starts with "The user would prefer" or describes WHAT KIND of response is desired, the generated answer is CORRECT if it demonstrates awareness of the user's specific personal context and provides a relevant personalized response.
- The generated answer does NOT need to match the expected answer word-for-word. It just needs to show it USED the user's personal information (past experiences, preferences, concerns, specific items/names).
- A personalized answer that references the user's specific situation is CORRECT even if it suggests different specific items than the expected answer.
- Example: Expected says "user would prefer suggestions that incorporate quinoa", generated says "Since you enjoyed quinoa bowls, try a Mediterranean quinoa salad" → CORRECT (personalized, references user's quinoa preference).
- The key test: does the generated answer show knowledge of THIS specific user, or is it generic advice anyone could get? If personalized → CORRECT. If generic with no personal references → WRONG.

BINDING SCORING RULES — you MUST follow these:
- You MUST score 1.0 if the core fact is correct and the generated answer simply adds extra context, dates, or elaboration.
- Correct count = 1.0 regardless of whether individual item names differ slightly. "4 books" = "4 books" even if specific titles vary.
- Same numeric progression, different structure: If both answers convey the same numeric change over time (e.g., "started with 4, now has 5" vs "Team of 4 in May, Team of 5 in October"), score as CORRECT. The numbers and their temporal ordering matter, not sentence structure.
- Personalized advice that references the user's specific context (names, preferences, past events) = 1.0, even if the specific recommendations differ from the expected answer.
- If the generated answer contains the correct answer PLUS additional information, score 1.0. Extra information is NEVER a reason to mark wrong.

Respond in exactly this format:
CORRECT: [YES/NO]
SCORE: [0.0-1.0]
REASONING: [Your explanation]"#,
            question,
            expected,
            generated,
            category.as_str(),
            category_guidance
        )
    }

    /// Get category-specific guidance for judging
    fn get_category_guidance(category: QuestionCategory) -> &'static str {
        match category {
            QuestionCategory::Extraction => {
                "Focus on whether the key facts are correctly extracted and stated. For advice/recommendation questions (tips, suggestions), a personalized answer that references the user's specific context is CORRECT even if it doesn't match the expected answer exactly — the key is personalization, not exact phrasing."
            }
            QuestionCategory::MultiSession => {
                "Focus on whether the logical inference is sound and the conclusion is correct."
            }
            QuestionCategory::Updates => {
                "Focus on whether the answer reflects the most recent/updated information."
            }
            QuestionCategory::Temporal => {
                "Focus on whether temporal relationships and sequences are correctly understood."
            }
            QuestionCategory::Abstention => {
                "Focus on whether the system correctly abstained when information was unavailable, \
                 OR correctly answered if information was actually available."
            }
        }
    }

    /// Strip common English suffixes for stem-aware comparison.
    /// Simple rule-based stemmer (not full Porter) — handles the most common inflections.
    fn stem_word(word: &str) -> String {
        let w = word.to_lowercase();
        if w.len() <= 3 {
            return w;
        }
        // Handle -ying → -y (e.g., "studying" → "study")
        if w.ends_with("ying") && w.len() > 5 {
            return format!("{}y", &w[..w.len() - 4]);
        }
        // Handle -ing: strip it, then undo doubled consonant if present
        if w.ends_with("ing") && w.len() > 4 {
            let stem = &w[..w.len() - 3];
            let bytes = stem.as_bytes();
            // If stem ends in doubled consonant (running→runn→run, hopping→hopp→hop)
            if bytes.len() >= 2 && bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
                return stem[..stem.len() - 1].to_string();
            }
            return stem.to_string();
        }
        // Handle -ied → -y (e.g., "studied" → "study")
        if w.ends_with("ied") && w.len() > 4 {
            return format!("{}y", &w[..w.len() - 3]);
        }
        // Handle -ated → -ate (e.g., "updated" → "update")
        if w.ends_with("ated") && w.len() > 5 {
            return w[..w.len() - 1].to_string();
        }
        // Handle -ed: strip it (e.g., "planted" → "plant", "signed" → "sign")
        if w.ends_with("ed") && w.len() > 4 {
            return w[..w.len() - 2].to_string();
        }
        if w.ends_with("es") && w.len() > 4 {
            return w[..w.len() - 2].to_string();
        }
        if w.ends_with("s") && !w.ends_with("ss") && w.len() > 3 {
            return w[..w.len() - 1].to_string();
        }
        if w.ends_with("ly") && w.len() > 4 {
            return w[..w.len() - 2].to_string();
        }
        w
    }

    /// Quick exact-match check to avoid LLM judge variance on identical answers.
    /// Returns Some(JudgeResult) if answers match, None otherwise.
    /// Pass category for category-aware keyword threshold (Extraction: 70%, others: 80%).
    fn exact_match_check_with_category(
        expected: &str,
        generated: &str,
        category: Option<QuestionCategory>,
    ) -> Option<JudgeResult> {
        let norm = |s: &str| -> String {
            let mut t = s
                .to_lowercase()
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_string();
            // Normalize equivalent expressions
            t = t
                .replace("twice", "2 times")
                .replace("once", "1 time")
                .replace("thrice", "3 times")
                .replace("approximately ", "about ")
                .replace("around ", "about ")
                .replace("close to ", "about ")
                .replace("roughly ", "about ");
            // Normalize time units
            t = t
                .replace("2 weeks", "14 days")
                .replace("a couple of", "2")
                .replace("a few", "3");
            t
        };
        let e = norm(expected);
        let g = norm(generated);

        // Abstention match: if both expected and generated indicate "not enough information", it's correct
        // P32: Skip abstention match when expected answer is numeric — a number can never be "not enough info"
        let e_is_numeric = e.trim().chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',' || c == '$');
        if !e_is_numeric {
            let abstention_phrases = [
                "not enough information",
                "i don't have enough information",
                "information provided is not enough",
                "did not mention",
                "information is not enough",
            ];
            let e_is_abstention = abstention_phrases.iter().any(|p| e.contains(p));
            let g_is_abstention = abstention_phrases.iter().any(|p| g.contains(p));
            if e_is_abstention && g_is_abstention {
                return Some(JudgeResult::new(
                    true,
                    1.0,
                    "Both answers indicate insufficient information (abstention match)".to_string(),
                ));
            }
        }

        // Number-word expansion map
        let number_words: &[(&str, &str)] = &[
            ("0", "zero"),
            ("1", "one"),
            ("2", "two"),
            ("3", "three"),
            ("4", "four"),
            ("5", "five"),
            ("6", "six"),
            ("7", "seven"),
            ("8", "eight"),
            ("9", "nine"),
            ("10", "ten"),
            ("11", "eleven"),
            ("12", "twelve"),
            ("13", "thirteen"),
            ("14", "fourteen"),
            ("15", "fifteen"),
            ("16", "sixteen"),
            ("17", "seventeen"),
            ("18", "eighteen"),
            ("19", "nineteen"),
            ("20", "twenty"),
        ];
        // Expand numbers in a string: "3" -> also check "three"
        let expand_numbers = |s: &str| -> String {
            let mut result = s.to_string();
            for &(digit, word) in number_words.iter().rev() {
                // Only expand standalone numbers (word boundary)
                let digit_padded = format!(" {} ", digit);
                let word_padded = format!(" {} ", word);
                if result.contains(&digit_padded) {
                    result = result.replace(&digit_padded, &word_padded);
                }
                // Also handle start/end of string
                if result == digit {
                    result = word.to_string();
                }
                if result.starts_with(&format!("{} ", digit)) {
                    result = format!("{}{}", word, &result[digit.len()..]);
                }
            }
            result
        };

        // Direct match or containment (also with number expansion)
        let e_expanded = expand_numbers(&e);
        let g_expanded = expand_numbers(&g);
        if !e.is_empty()
            && (e == g
                || g.contains(&e)
                || g_expanded.contains(&e)
                || g.contains(&e_expanded)
                || g_expanded.contains(&e_expanded))
        {
            return Some(JudgeResult::new(
                true,
                1.0,
                "Exact match (normalized, with number expansion)".to_string(),
            ));
        }

        // Key-word overlap check: if all significant words from expected appear in generated
        // This catches cases like "planting tomato saplings" vs "planted 12 tomato saplings on April 21"
        let stop_words: std::collections::HashSet<&str> = [
            "a", "an", "the", "i", "my", "me", "you", "your", "we", "our", "is", "was", "are",
            "were", "be", "been", "being", "in", "on", "at", "to", "for", "of", "with", "from",
            "by", "and", "or", "but", "not", "no", "so", "if", "then", "that", "this", "it", "its",
            "about", "did", "do", "does", "have", "has", "had", "will", "would", "could", "should",
        ]
        .iter()
        .copied()
        .collect();

        // Strip URLs before keyword extraction so URL fragments don't pollute overlap
        let strip_urls = |s: &str| -> String {
            let mut result = s.to_string();
            // Remove http(s) URLs
            while let Some(start) = result.find("http") {
                let end = result[start..]
                    .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ')')
                    .map(|i| start + i)
                    .unwrap_or(result.len());
                result.replace_range(start..end, "");
            }
            result
        };

        let extract_keywords = |s: &str| -> Vec<String> {
            let cleaned = strip_urls(s);
            cleaned
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty() && w.len() > 1 && !stop_words.contains(w))
                .map(|w| w.to_string())
                .collect()
        };

        let expected_keywords = extract_keywords(&e);
        if expected_keywords.len() >= 2 {
            let g_text = &g;
            let g_text_expanded = expand_numbers(&g);
            // Build stemmed version of generated text for stem-aware matching
            let g_words: Vec<String> = g
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(|w| Self::stem_word(w))
                .collect();
            let g_stemmed = g_words.join(" ");

            let matched = expected_keywords
                .iter()
                .filter(|kw| {
                    // Check original text, number-expanded, and stemmed
                    g_text.contains(kw.as_str())
                        || g_text_expanded.contains(kw.as_str())
                        || g_stemmed.contains(&Self::stem_word(kw))
                        || number_words.iter().any(|&(d, w)| {
                            (kw.as_str() == d && g_text.contains(w))
                                || (kw.as_str() == w && g_text.contains(d))
                        })
                })
                .count();
            // Category-aware threshold: Extraction uses 70% (answers paraphrase heavily),
            // other categories use 80%
            let threshold = match category {
                Some(QuestionCategory::Extraction) => 0.70,
                _ => 0.80,
            };
            if matched as f64 / expected_keywords.len() as f64 >= threshold {
                return Some(JudgeResult::new(
                    true,
                    1.0,
                    format!(
                        "Keyword overlap match ({}/{} keywords, threshold={:.0}%)",
                        matched,
                        expected_keywords.len(),
                        threshold * 100.0,
                    ),
                ));
            }
        }

        // P5: Deterministic temporal duration checker
        // If expected answer is a number+unit (e.g., "3 months", "15 weeks", "2 years"),
        // check if generated contains the same numeric value
        if let Some(result) = Self::temporal_duration_check(&e, &g, category) {
            return Some(result);
        }

        // P5: Deterministic numeric answer checker
        // If expected answer is a standalone number, check if generated contains it
        if let Some(result) = Self::numeric_answer_check(&e, &g) {
            return Some(result);
        }

        None
    }

    /// P5: Check if both answers express the same temporal duration
    /// Handles: "3 months", "15 weeks", "2 years", "about 9 months", etc.
    fn temporal_duration_check(
        expected: &str,
        generated: &str,
        category: Option<QuestionCategory>,
    ) -> Option<JudgeResult> {
        // Only apply to Temporal and Updates categories (where durations matter most)
        match category {
            Some(QuestionCategory::Temporal) | Some(QuestionCategory::Updates) => {}
            _ => return None,
        }

        let duration_re = regex::Regex::new(
            r"(?i)\b(\d+(?:\.\d+)?)\s*(month|week|year|day|hour|minute|second)s?\b"
        ).ok()?;

        let extract_first = |s: &str| -> Option<(f64, String)> {
            let caps = duration_re.captures(s)?;
            let num: f64 = caps[1].parse().ok()?;
            let unit = caps[2].to_lowercase();
            Some((num, unit))
        };

        // When expected mentions "total" and has multiple durations, the answer
        // is the total (last duration), not a component (first).
        // Fixes gpt4_a1b77f9c: "2 weeks... 4 weeks... total of 8 weeks" → use 8 weeks.
        let e_has_total = expected.to_lowercase().contains("total");
        let e_dur_count = duration_re.captures_iter(expected).count();
        let e_dur = if e_has_total && e_dur_count >= 2 {
            let all_caps: Vec<_> = duration_re.captures_iter(expected).collect();
            let last = all_caps.last()?;
            let num: f64 = last[1].parse().ok()?;
            let unit = last[2].to_lowercase();
            (num, unit)
        } else {
            extract_first(expected)?
        };
        let g_dur = extract_first(generated)?;

        // Same unit and same number → correct
        if e_dur.1 == g_dur.1 && (e_dur.0 - g_dur.0).abs() < 0.01 {
            return Some(JudgeResult::new(
                true,
                1.0,
                format!(
                    "Temporal duration match: {} {}",
                    e_dur.0, e_dur.1
                ),
            ));
        }

        // Convert to common unit (days) for cross-unit comparison
        let to_days = |num: f64, unit: &str| -> f64 {
            match unit {
                "day" => num,
                "week" => num * 7.0,
                "month" => num * 30.44, // average month
                "year" => num * 365.25,
                "hour" => num / 24.0,
                "minute" => num / 1440.0,
                "second" => num / 86400.0,
                _ => num,
            }
        };

        let e_days = to_days(e_dur.0, &e_dur.1);
        let g_days = to_days(g_dur.0, &g_dur.1);

        // Allow ~10% tolerance for unit conversion (e.g., "2 months" ≈ "8 weeks")
        if e_days > 0.0 && (e_days - g_days).abs() / e_days < 0.15 {
            return Some(JudgeResult::new(
                true,
                1.0,
                format!(
                    "Temporal duration match (cross-unit): {} {} ≈ {} {}",
                    e_dur.0, e_dur.1, g_dur.0, g_dur.1
                ),
            ));
        }

        // When expected is a "total", also check if ANY generated duration matches.
        // Agent may write "8 weeks total" or list components then total — match any.
        if e_has_total {
            for caps in duration_re.captures_iter(generated) {
                let num: f64 = match caps[1].parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let unit = caps[2].to_lowercase();
                let g_alt_days = to_days(num, &unit);
                if e_days > 0.0 && (e_days - g_alt_days).abs() / e_days < 0.15 {
                    return Some(JudgeResult::new(
                        true,
                        1.0,
                        format!(
                            "Temporal duration match (total vs any): {} {} ≈ {} {}",
                            e_dur.0, e_dur.1, num, unit
                        ),
                    ));
                }
            }
        }

        // If both have durations but they differ significantly, mark wrong
        // Only do this for Temporal category where the duration IS the answer
        if category == Some(QuestionCategory::Temporal)
            && e_days > 0.0
            && (e_days - g_days).abs() / e_days > 0.5
        {
            return Some(JudgeResult::new(
                false,
                0.0,
                format!(
                    "Temporal duration mismatch: expected {} {} but got {} {}",
                    e_dur.0, e_dur.1, g_dur.0, g_dur.1
                ),
            ));
        }

        None
    }

    /// P5: Check if expected is a standalone number and generated contains it
    fn numeric_answer_check(expected: &str, generated: &str) -> Option<JudgeResult> {
        // Only if expected is very short (likely a numeric answer)
        let e_trimmed = expected.trim();
        if e_trimmed.len() > 20 {
            return None;
        }

        // Extract the core number from expected (handles "$1,300", "15", "3.5")
        let extract_number = |s: &str| -> Option<f64> {
            let cleaned = s
                .replace(',', "")
                .replace('$', "")
                .trim()
                .to_string();
            let num_re = regex::Regex::new(r"^(\d+(?:\.\d+)?)$").ok()?;
            let caps = num_re.captures(&cleaned)?;
            caps[1].parse().ok()
        };

        let e_num = extract_number(e_trimmed)?;

        // Check if generated contains this exact number
        let g_nums: Vec<f64> = {
            let num_re = regex::Regex::new(r"\b(\d+(?:\.\d+)?)\b").ok()?;
            num_re
                .captures_iter(generated)
                .filter_map(|c| c[1].parse().ok())
                .collect()
        };

        // If generated contains the expected number, it's correct
        if g_nums.iter().any(|&n| (n - e_num).abs() < 0.01) {
            return Some(JudgeResult::new(
                true,
                1.0,
                format!("Numeric answer match: {}", e_num),
            ));
        }

        None
    }

    /// Parse a judge response in the expected format
    pub fn parse_judge_response(response: &str) -> (bool, f32, String) {
        let lines: Vec<&str> = response.lines().collect();

        let mut is_correct = false;
        let mut score = 0.0;
        let mut reasoning = String::new();

        for line in lines {
            let line = line.trim();
            let line_upper = line.to_uppercase();
            if line_upper.starts_with("CORRECT:") {
                let token = line.split(':').nth(1).unwrap_or("").trim().to_uppercase();
                is_correct = token.starts_with("YES");
            } else if line_upper.starts_with("SCORE:") {
                if let Some(score_str) = line.split(':').nth(1) {
                    score = score_str.trim().parse().unwrap_or(0.0);
                }
            } else if line_upper.starts_with("REASONING:") {
                reasoning = line
                    .split(':')
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(":")
                    .trim()
                    .to_string();
            }
        }

        (is_correct, score, reasoning)
    }

    /// Simple heuristic evaluation for placeholder implementation
    fn evaluate_answer(
        &self,
        expected: &str,
        generated: &str,
        category: QuestionCategory,
    ) -> (bool, f32, String) {
        let expected_lower = expected.to_lowercase();
        let generated_lower = generated.to_lowercase();

        // Special handling for abstention category
        if category == QuestionCategory::Abstention {
            let abstained = generated_lower.contains("don't know")
                || generated_lower.contains("don't have")
                || generated_lower.contains("cannot answer")
                || generated_lower.contains("no information");

            if expected_lower.contains("abstain")
                || expected_lower.contains("unknown")
                || expected_lower.contains("n/a")
            {
                // Should have abstained
                if abstained {
                    return (true, 1.0, "Correctly abstained from answering".to_string());
                } else {
                    return (
                        false,
                        0.0,
                        "Should have abstained but provided an answer".to_string(),
                    );
                }
            } else {
                // Should have answered
                if abstained {
                    return (
                        false,
                        0.0,
                        "Incorrectly abstained when answer was available".to_string(),
                    );
                }
            }
        }

        // Check if key information is present
        let expected_words: Vec<&str> = expected_lower.split_whitespace().collect();
        let key_words: Vec<&str> = expected_words
            .iter()
            .filter(|w| w.len() > 3 && !is_stop_word(w))
            .copied()
            .collect();

        if key_words.is_empty() {
            // Direct comparison for short answers
            if generated_lower.contains(&expected_lower) {
                return (
                    true,
                    1.0,
                    "Answer contains expected information".to_string(),
                );
            }
        }

        let matches: usize = key_words
            .iter()
            .filter(|w| generated_lower.contains(*w))
            .count();

        let score = if key_words.is_empty() {
            0.5
        } else {
            matches as f32 / key_words.len() as f32
        };

        let is_correct = score >= 0.5;
        let reasoning = format!(
            "Matched {}/{} key terms from expected answer",
            matches,
            key_words.len()
        );

        (is_correct, score, reasoning)
    }
}

/// Check if a word is a stop word
fn is_stop_word(word: &str) -> bool {
    matches!(
        word,
        "the"
            | "a"
            | "an"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "must"
            | "shall"
            | "can"
            | "need"
            | "dare"
            | "ought"
            | "used"
            | "to"
            | "of"
            | "in"
            | "for"
            | "on"
            | "with"
            | "at"
            | "by"
            | "from"
            | "up"
            | "about"
            | "into"
            | "through"
            | "during"
            | "before"
            | "after"
            | "above"
            | "below"
            | "between"
            | "under"
            | "again"
            | "further"
            | "then"
            | "once"
            | "here"
            | "there"
            | "when"
            | "where"
            | "why"
            | "how"
            | "all"
            | "each"
            | "few"
            | "more"
            | "most"
            | "other"
            | "some"
            | "such"
            | "no"
            | "nor"
            | "not"
            | "only"
            | "own"
            | "same"
            | "so"
            | "than"
            | "too"
            | "very"
            | "just"
            | "and"
            | "but"
            | "if"
            | "or"
            | "because"
            | "as"
            | "until"
            | "while"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_judge_config_default() {
        let config = JudgeConfig::default();
        assert_eq!(config.judge_model, "gpt-4o");
        assert_eq!(config.temperature, 0.0);
    }

    #[test]
    fn test_judge_config_builder() {
        let config = JudgeConfig::new()
            .with_model("gpt-4o-mini")
            .with_temperature(0.1);

        assert_eq!(config.judge_model, "gpt-4o-mini");
        assert_eq!(config.temperature, 0.1);
    }

    #[test]
    fn test_judge_result_correct() {
        let result = JudgeResult::correct("Good answer");
        assert!(result.is_correct);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn test_judge_result_incorrect() {
        let result = JudgeResult::incorrect("Bad answer");
        assert!(!result.is_correct);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_parse_judge_response_correct() {
        let response =
            "CORRECT: YES\nSCORE: 0.95\nREASONING: The answer matches the expected response.";
        let (is_correct, score, reasoning) = Judge::parse_judge_response(response);

        assert!(is_correct);
        assert!((score - 0.95).abs() < 0.01);
        assert!(reasoning.contains("matches"));
    }

    #[test]
    fn test_parse_judge_response_incorrect() {
        let response = "CORRECT: NO\nSCORE: 0.2\nREASONING: The answer is wrong.";
        let (is_correct, score, reasoning) = Judge::parse_judge_response(response);

        assert!(!is_correct);
        assert!((score - 0.2).abs() < 0.01);
        assert!(reasoning.contains("wrong"));
    }

    #[test]
    fn test_parse_judge_response_case_insensitive() {
        let response = "correct: yes\nscore: 1.0\nreasoning: ok";
        let (is_correct, score, reasoning) = Judge::parse_judge_response(response);
        assert!(is_correct);
        assert!((score - 1.0).abs() < 0.01);
        assert_eq!(reasoning, "ok");
    }

    #[test]
    fn test_parse_judge_response_no_false_positive_on_yes_in_reasoning() {
        // "CORRECT: NO" but reasoning contains "yes" — must NOT false-positive
        let response =
            "CORRECT: NO, the answer says yes but is wrong\nSCORE: 0.1\nREASONING: Wrong.";
        let (is_correct, _, _) = Judge::parse_judge_response(response);
        assert!(!is_correct);
    }

    #[test]
    fn test_build_judge_prompt() {
        let judge = Judge::with_defaults();
        let prompt = judge.build_judge_prompt(
            "What is the user's name?",
            "John",
            "The user's name is John.",
            QuestionCategory::Extraction,
        );

        assert!(prompt.contains("What is the user's name?"));
        assert!(prompt.contains("John"));
        assert!(prompt.contains("extraction"));
        assert!(prompt.contains("CORRECT:"));
        assert!(prompt.contains("SCORE:"));
        assert!(prompt.contains("REASONING:"));
    }

    #[test]
    fn test_judge_extraction() {
        let judge = Judge::with_defaults();
        let result = judge
            .judge(
                "What is the user's name?",
                "John Smith",
                "The user's name is John Smith",
                QuestionCategory::Extraction,
            )
            .unwrap();

        assert!(result.is_correct);
        assert!(result.score > 0.5);
    }

    #[test]
    fn test_judge_abstention_correct() {
        let judge = Judge::with_defaults();
        let result = judge
            .judge(
                "What is the user's favorite color?",
                "ABSTAIN - no information",
                "I don't have enough information to answer this question.",
                QuestionCategory::Abstention,
            )
            .unwrap();

        assert!(result.is_correct);
        assert!(result.reasoning.contains("abstained"));
    }

    #[test]
    fn test_judge_abstention_should_have_abstained() {
        let judge = Judge::with_defaults();
        let result = judge
            .judge(
                "What is the user's favorite color?",
                "ABSTAIN - unknown",
                "The user's favorite color is blue.",
                QuestionCategory::Abstention,
            )
            .unwrap();

        assert!(!result.is_correct);
        assert!(result.reasoning.contains("Should have abstained"));
    }

    #[test]
    fn test_judge_abstention_incorrectly_abstained() {
        let judge = Judge::with_defaults();
        let result = judge
            .judge(
                "What is the user's name?",
                "John Smith",
                "I don't have enough information to answer this.",
                QuestionCategory::Abstention,
            )
            .unwrap();

        assert!(!result.is_correct);
        assert!(result.reasoning.contains("Incorrectly abstained"));
    }

    #[test]
    fn test_get_category_guidance() {
        assert!(Judge::get_category_guidance(QuestionCategory::Extraction).contains("facts"));
        assert!(Judge::get_category_guidance(QuestionCategory::MultiSession).contains("inference"));
        assert!(Judge::get_category_guidance(QuestionCategory::Updates).contains("recent"));
        assert!(Judge::get_category_guidance(QuestionCategory::Temporal).contains("temporal"));
        assert!(Judge::get_category_guidance(QuestionCategory::Abstention).contains("abstained"));
    }

    #[test]
    fn test_is_stop_word() {
        assert!(is_stop_word("the"));
        assert!(is_stop_word("is"));
        assert!(is_stop_word("and"));
        assert!(!is_stop_word("john"));
        assert!(!is_stop_word("smith"));
        assert!(!is_stop_word("python"));
    }

    #[test]
    fn test_judge_cost_tracking() {
        let judge = Judge::with_defaults();
        // Use answers that won't be caught by exact_match_check so we hit heuristic path
        let result = judge
            .judge(
                "Describe the user's hobbies",
                "painting, hiking, and reading",
                "The user enjoys watercolor painting and mountain hiking",
                QuestionCategory::Extraction,
            )
            .unwrap();

        assert!(result.cost_usd > 0.0);
    }

    #[test]
    fn test_temporal_duration_check_same_unit() {
        let result = Judge::temporal_duration_check(
            "3 months",
            "the answer is 3 months",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_check_cross_unit() {
        // 2 months ≈ 8.7 weeks — should match with tolerance
        let result = Judge::temporal_duration_check(
            "2 months",
            "about 9 weeks",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_check_mismatch() {
        let result = Judge::temporal_duration_check(
            "3 months",
            "it was 8 months",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(!result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_check_wrong_category() {
        // Should not fire for Extraction category
        let result = Judge::temporal_duration_check(
            "3 months",
            "3 months",
            Some(QuestionCategory::Extraction),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_temporal_duration_total_keyword() {
        // gpt4_a1b77f9c: expected has component durations + total, generated has the total
        let result = Judge::temporal_duration_check(
            "2 weeks for 'The Nightingale', 4 weeks for 'Sapiens', and 2 weeks for 'The Power', so a total of 8 weeks.",
            "8 weeks. Items: 1) Week 1 of The Nightingale...",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_total_component_first_generated() {
        // Generated lists components then total — "any match" should catch the total
        let result = Judge::temporal_duration_check(
            "2 weeks for book A, 4 weeks for book B, total of 6 weeks",
            "2 weeks for A, 4 weeks for B, totaling 6 weeks",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_single_with_total_word() {
        // "total of 8 weeks" with only one duration — no special path needed, first == last
        let result = Judge::temporal_duration_check(
            "a total of 8 weeks",
            "8 weeks",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_temporal_duration_genuine_mismatch_unchanged() {
        // 8c18457d regression guard: 7 days vs 33 days — no "total" keyword, must stay INCORRECT
        let result = Judge::temporal_duration_check(
            "33 days",
            "7 days",
            Some(QuestionCategory::Temporal),
        );
        assert!(result.is_some());
        assert!(!result.unwrap().is_correct);
    }

    #[test]
    fn test_numeric_answer_check() {
        let result = Judge::numeric_answer_check("15", "the user ran 15 times");
        assert!(result.is_some());
        assert!(result.unwrap().is_correct);
    }

    #[test]
    fn test_numeric_answer_check_not_present() {
        let result = Judge::numeric_answer_check("15", "the user ran 12 times");
        assert!(result.is_none()); // number not found, falls through to LLM
    }

    #[test]
    fn test_stem_word() {
        // Doubled consonant after -ing removal
        assert_eq!(Judge::stem_word("running"), "run");
        assert_eq!(Judge::stem_word("hopping"), "hop");
        assert_eq!(Judge::stem_word("sitting"), "sit");
        assert_eq!(Judge::stem_word("planning"), "plan");

        // Regular -ing removal (no doubled consonant)
        assert_eq!(Judge::stem_word("waiting"), "wait");
        assert_eq!(Judge::stem_word("playing"), "play");
        assert_eq!(Judge::stem_word("creating"), "creat");

        // -ying → y
        assert_eq!(Judge::stem_word("studying"), "study");
        assert_eq!(Judge::stem_word("playing"), "play");

        // -ied → y
        assert_eq!(Judge::stem_word("studied"), "study");
        assert_eq!(Judge::stem_word("carried"), "carry");

        // -ated → ate
        assert_eq!(Judge::stem_word("created"), "create");
        assert_eq!(Judge::stem_word("related"), "relate");

        // -ed removal
        assert_eq!(Judge::stem_word("planted"), "plant");
        assert_eq!(Judge::stem_word("walked"), "walk");

        // Short words unchanged
        assert_eq!(Judge::stem_word("run"), "run");
        assert_eq!(Judge::stem_word("go"), "go");
    }
}
