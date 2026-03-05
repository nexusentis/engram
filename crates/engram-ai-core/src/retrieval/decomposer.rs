//! Query decomposition for multi-hop queries
//!
//! Decomposes complex queries like "What project is Alice's manager working on?"
//! into sequential sub-queries that can be executed step-by-step.

use super::engine::RerankedResult;

/// A single step in a decomposed query
#[derive(Debug, Clone)]
pub struct QueryStep {
    /// The sub-query to execute
    pub query: String,
    /// What entity/fact this step is looking for
    pub target: String,
    /// Dependencies on previous steps (by index)
    pub depends_on: Vec<usize>,
    /// Whether this step uses results from previous steps
    pub uses_previous_result: bool,
}

/// A decomposed multi-hop query
#[derive(Debug, Clone)]
pub struct DecomposedQuery {
    /// Original query
    pub original: String,
    /// Ordered steps to execute
    pub steps: Vec<QueryStep>,
    /// Final aggregation strategy
    pub aggregation: AggregationStrategy,
}

#[derive(Debug, Clone, Copy)]
pub enum AggregationStrategy {
    /// Return result of last step
    LastStep,
    /// Combine all step results
    Union,
    /// Intersect results across steps
    Intersection,
}

/// Query decomposer for multi-hop queries
pub struct QueryDecomposer {
    patterns: Vec<DecompositionPattern>,
}

struct DecompositionPattern {
    /// Pattern to match (simplified regex-like)
    pattern: &'static str,
    /// Function to decompose
    decompose: fn(&str) -> Option<Vec<QueryStep>>,
}

impl QueryDecomposer {
    pub fn new() -> Self {
        Self {
            patterns: Self::build_patterns(),
        }
    }

    fn build_patterns() -> Vec<DecompositionPattern> {
        vec![
            // "X's Y's Z" pattern: Alice's manager's project
            DecompositionPattern {
                pattern: "'s.*'s",
                decompose: Self::decompose_possessive_chain,
            },
            // "Y of X" pattern: project of manager of Alice
            DecompositionPattern {
                pattern: " of .* of ",
                decompose: Self::decompose_of_chain,
            },
            // "What does X work on" pattern
            DecompositionPattern {
                pattern: "what does .* work on",
                decompose: Self::decompose_work_on,
            },
        ]
    }

    /// Decompose a query if it's multi-hop
    pub fn decompose(&self, query: &str) -> Option<DecomposedQuery> {
        let lower = query.to_lowercase();

        for pattern in &self.patterns {
            if lower.contains(pattern.pattern.split(".*").next().unwrap_or("")) {
                if let Some(steps) = (pattern.decompose)(query) {
                    return Some(DecomposedQuery {
                        original: query.to_string(),
                        steps,
                        aggregation: AggregationStrategy::LastStep,
                    });
                }
            }
        }

        None
    }

    /// "Alice's manager's project" -> ["Who is Alice's manager?", "What project is [result] working on?"]
    fn decompose_possessive_chain(query: &str) -> Option<Vec<QueryStep>> {
        let parts: Vec<&str> = query.split("'s").collect();
        if parts.len() < 3 {
            return None;
        }

        let entity = parts[0].trim();
        let relation = parts[1].trim();
        let target = parts[2..].join("'s").trim().to_string();

        Some(vec![
            QueryStep {
                query: format!("Who is {}'s {}?", entity, relation),
                target: format!("{}'s {}", entity, relation),
                depends_on: vec![],
                uses_previous_result: false,
            },
            QueryStep {
                query: format!("What {} does [RESULT] have?", target.trim_end_matches('?')),
                target: target.clone(),
                depends_on: vec![0],
                uses_previous_result: true,
            },
        ])
    }

    /// "project of manager of Alice" -> decompose into steps
    fn decompose_of_chain(query: &str) -> Option<Vec<QueryStep>> {
        let lower = query.to_lowercase();
        let parts: Vec<&str> = lower.split(" of ").collect();
        if parts.len() < 3 {
            return None;
        }

        // Reverse to get entity first
        let reversed: Vec<_> = parts.iter().rev().collect();

        let mut steps = Vec::new();
        for (i, (current, next)) in reversed.iter().zip(reversed.iter().skip(1)).enumerate() {
            steps.push(QueryStep {
                query: if i == 0 {
                    format!("What is the {} of {}?", next, current)
                } else {
                    format!("What is the {} of [RESULT]?", next)
                },
                target: next.to_string(),
                depends_on: if i > 0 { vec![i - 1] } else { vec![] },
                uses_previous_result: i > 0,
            });
        }

        Some(steps)
    }

    /// "What does Alice work on" -> simple single-hop
    fn decompose_work_on(query: &str) -> Option<Vec<QueryStep>> {
        // Extract entity between "does" and "work"
        let lower = query.to_lowercase();
        let start = lower.find("does ")?.checked_add(5)?;
        let end = lower.find(" work")?;
        let entity = query[start..end].trim();

        Some(vec![QueryStep {
            query: format!("What projects is {} working on?", entity),
            target: "projects".to_string(),
            depends_on: vec![],
            uses_previous_result: false,
        }])
    }
}

impl Default for QueryDecomposer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of executing a decomposed query
#[derive(Debug)]
pub struct DecompositionResult {
    pub original_query: String,
    pub steps_executed: usize,
    pub step_results: Vec<StepResult>,
    pub final_results: Vec<RerankedResult>,
    pub confidence: f32,
}

#[derive(Debug)]
pub struct StepResult {
    pub step_index: usize,
    pub query_executed: String,
    pub results: Vec<RerankedResult>,
    pub extracted_answer: Option<String>,
}

/// Executor for decomposed queries
pub struct DecompositionExecutor<'a> {
    #[allow(clippy::type_complexity)]
    retrieval_fn: Box<dyn Fn(&str) -> Vec<RerankedResult> + 'a>,
}

impl<'a> DecompositionExecutor<'a> {
    pub fn new<F>(retrieval_fn: F) -> Self
    where
        F: Fn(&str) -> Vec<RerankedResult> + 'a,
    {
        Self {
            retrieval_fn: Box::new(retrieval_fn),
        }
    }

    /// Execute a decomposed query step by step
    pub fn execute(&self, decomposed: &DecomposedQuery) -> DecompositionResult {
        let mut step_results = Vec::new();
        let mut previous_answers: Vec<Option<String>> = Vec::new();

        for (i, step) in decomposed.steps.iter().enumerate() {
            // Substitute previous results into query
            let query = if step.uses_previous_result {
                let prev_answer = step
                    .depends_on
                    .iter()
                    .filter_map(|&idx| previous_answers.get(idx)?.as_ref())
                    .next()
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");

                step.query.replace("[RESULT]", prev_answer)
            } else {
                step.query.clone()
            };

            // Execute retrieval
            let results = (self.retrieval_fn)(&query);

            // Extract answer from top result
            let extracted_answer = results
                .first()
                .map(|r| Self::extract_answer(&r.memory.content, &step.target));

            step_results.push(StepResult {
                step_index: i,
                query_executed: query,
                results: results.clone(),
                extracted_answer: extracted_answer.clone(),
            });

            previous_answers.push(extracted_answer);
        }

        // Final results from last step
        let final_results = step_results
            .last()
            .map(|s| s.results.clone())
            .unwrap_or_default();

        // Calculate overall confidence
        let confidence = Self::calculate_chain_confidence(&step_results);

        DecompositionResult {
            original_query: decomposed.original.clone(),
            steps_executed: step_results.len(),
            step_results,
            final_results,
            confidence,
        }
    }

    fn extract_answer(content: &str, target: &str) -> String {
        // Simple extraction: look for the target in content
        // In production, this could use an LLM or NER
        let target_lower = target.to_lowercase();
        let content_lower = content.to_lowercase();

        if content_lower.contains(&target_lower) {
            // Find surrounding context
            if let Some(pos) = content_lower.find(&target_lower) {
                let start = content[..pos].rfind(' ').map(|p| p + 1).unwrap_or(0);
                let end = content[pos..]
                    .find(['.', ',', '!', '?'])
                    .map(|p| pos + p)
                    .unwrap_or(content.len());
                return content[start..end].trim().to_string();
            }
        }

        // Fallback: return first sentence
        content
            .split('.')
            .next()
            .unwrap_or(content)
            .trim()
            .to_string()
    }

    fn calculate_chain_confidence(steps: &[StepResult]) -> f32 {
        if steps.is_empty() {
            return 0.0;
        }

        // Chain confidence: product of step confidences
        let mut confidence = 1.0;
        for step in steps {
            let step_conf = step.results.first().map(|r| r.final_score).unwrap_or(0.0);
            confidence *= step_conf;
        }

        confidence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Memory;

    fn make_memory(content: &str) -> Memory {
        Memory::new("test_user", content)
    }

    #[test]
    fn test_possessive_chain_decomposition() {
        let decomposer = QueryDecomposer::new();

        let query = "What is Alice's manager's project?";
        let decomposed = decomposer.decompose(query);

        assert!(decomposed.is_some());
        let d = decomposed.unwrap();
        assert_eq!(d.steps.len(), 2);
        assert!(d.steps[0].query.contains("manager"));
        assert!(d.steps[1].uses_previous_result);
    }

    #[test]
    fn test_simple_query_no_decomposition() {
        let decomposer = QueryDecomposer::new();

        let query = "What is Alice's job?";
        let decomposed = decomposer.decompose(query);

        // Single possessive shouldn't trigger multi-hop
        assert!(decomposed.is_none());
    }

    #[test]
    fn test_of_chain_decomposition() {
        let decomposer = QueryDecomposer::new();

        let query = "project of manager of Alice";
        let decomposed = decomposer.decompose(query);

        assert!(decomposed.is_some());
        let d = decomposed.unwrap();
        assert_eq!(d.steps.len(), 2);
    }

    #[test]
    fn test_work_on_decomposition() {
        let decomposer = QueryDecomposer::new();

        let query = "What does Alice work on?";
        let decomposed = decomposer.decompose(query);

        assert!(decomposed.is_some());
        let d = decomposed.unwrap();
        assert_eq!(d.steps.len(), 1);
        assert!(d.steps[0].query.contains("Alice"));
    }

    #[test]
    fn test_decomposition_executor() {
        let decomposer = QueryDecomposer::new();
        let decomposed = decomposer
            .decompose("What is Alice's manager's project?")
            .unwrap();

        // Mock retrieval function
        let executor = DecompositionExecutor::new(|_query: &str| {
            let mut memory = make_memory("Bob is the manager");
            memory.content = "Bob is the manager".to_string();
            vec![RerankedResult {
                memory,
                original_rrf_score: 0.8,
                rerank_score: Some(0.9),
                final_score: 0.85,
                contributing_channels: vec![],
            }]
        });

        let result = executor.execute(&decomposed);
        assert_eq!(result.steps_executed, 2);
        assert!(!result.final_results.is_empty());
    }

    #[test]
    fn test_extract_answer() {
        let content = "Bob is the manager of the engineering team.";
        let answer = DecompositionExecutor::<'_>::extract_answer(content, "manager");
        assert!(answer.contains("manager"));
    }

    #[test]
    fn test_chain_confidence_calculation() {
        let steps = vec![
            StepResult {
                step_index: 0,
                query_executed: "step 1".to_string(),
                results: vec![make_reranked_result(0.8)],
                extracted_answer: Some("answer1".to_string()),
            },
            StepResult {
                step_index: 1,
                query_executed: "step 2".to_string(),
                results: vec![make_reranked_result(0.5)],
                extracted_answer: Some("answer2".to_string()),
            },
        ];

        let confidence = DecompositionExecutor::<'_>::calculate_chain_confidence(&steps);
        // 0.8 * 0.5 = 0.4
        assert!((confidence - 0.4).abs() < 0.001);
    }

    fn make_reranked_result(score: f32) -> RerankedResult {
        RerankedResult {
            memory: make_memory("test content"),
            original_rrf_score: score,
            rerank_score: Some(score),
            final_score: score,
            contributing_channels: vec![],
        }
    }
}
