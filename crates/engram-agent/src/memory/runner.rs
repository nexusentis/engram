//! `MemoryAgent`: production memory answering agent with optional ensemble fallback.
//!
//! Wraps the generic `Agent` loop with memory-specific behavior:
//! - Strategy-aware prompting ([`QuestionStrategy`])
//! - Prefetch (parallel search of facts + messages)
//! - Quality gates via [`MemoryAgentHook`]
//! - Optional ensemble fallback (retry with second LLM on abstention/loop-break)

use std::sync::Arc;
use std::time::Instant;

use serde_json;

use engram_core::agent::{
    QuestionStrategy, build_agent_system_prompt, detect_question_strategy, strategy_guidance,
};
use engram_core::agent::tools::ToolExecutor;
use engram_core::config::AgentConfig as CoreAgentConfig;
use engram_core::embedding::EmbeddingProvider;
use engram_core::llm::LlmClient;
use engram_core::storage::QdrantStorage;

use crate::agent::Agent;
use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::types::LoopBreakReason;

use super::gates::MemoryAgentHook;
use super::tool_adapter::wrap_tools;

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Result from a `MemoryAgent::answer()` call.
#[derive(Debug)]
pub struct MemoryAnswerResult {
    /// The final answer text.
    pub answer: String,
    /// Whether the agent abstained ("I don't have enough information").
    pub abstained: bool,
    /// Total estimated cost in USD (primary + fallback if used).
    pub cost: f32,
    /// Total wall-clock time in milliseconds.
    pub total_time_ms: u64,
    /// Number of agentic iterations (primary run).
    pub iterations: u32,
    /// Whether the loop broke without a done() answer.
    pub loop_break: bool,
    /// Why the loop broke (if it did).
    pub loop_break_reason: Option<LoopBreakReason>,
    /// Strategy detected for this question.
    pub strategy: QuestionStrategy,
    /// Whether ensemble fallback was used.
    pub fallback_used: bool,
    /// Why fallback was triggered (if it was).
    pub fallback_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// MemoryAgent
// ---------------------------------------------------------------------------

/// Production memory answering agent.
///
/// Created once per server lifetime (or once per benchmark run), then
/// `answer()` is called per question. Thread-safe via interior `Arc`s.
pub struct MemoryAgent {
    config: CoreAgentConfig,
    storage: Arc<QdrantStorage>,
    embedding_provider: Arc<dyn EmbeddingProvider>,
    llm: Arc<dyn LlmClient>,
    fallback_llm: Option<Arc<dyn LlmClient>>,
}

impl MemoryAgent {
    /// Create a new memory agent.
    pub fn new(
        config: CoreAgentConfig,
        storage: Arc<QdrantStorage>,
        embedding_provider: Arc<dyn EmbeddingProvider>,
        llm: Arc<dyn LlmClient>,
    ) -> Self {
        Self {
            config,
            storage,
            embedding_provider,
            llm,
            fallback_llm: None,
        }
    }

    /// Configure a fallback LLM for ensemble routing.
    pub fn with_fallback_llm(mut self, fallback: Arc<dyn LlmClient>) -> Self {
        self.fallback_llm = Some(fallback);
        self
    }

    /// Answer a question using the full agentic pipeline.
    ///
    /// # Arguments
    /// - `question`: the user's question text
    /// - `user_id`: the user whose memories to search
    /// - `question_date`: optional reference time (defaults to now)
    pub async fn answer(
        &self,
        question: &str,
        user_id: &str,
        question_date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<MemoryAnswerResult, AgentError> {
        let start = Instant::now();

        // 1. Detect strategy from question text (no oracle)
        let strategy = detect_question_strategy(question);

        // 2. Build ToolExecutor
        let tool_executor = ToolExecutor::new(
            Arc::clone(&self.storage),
            Arc::clone(&self.embedding_provider),
        )
        .with_user_id(user_id)
        .with_reference_date(question_date);

        let tool_executor = Arc::new(tool_executor);

        // 3. Prefetch initial results
        let prefetch_results = self.prefetch(question, &tool_executor).await;

        // 4. Build system prompt with strategy guidance
        let guidance = if self.config.use_strategy {
            strategy_guidance(&strategy)
        } else {
            ""
        };
        let base_prompt = build_agent_system_prompt(question, question_date);
        let system_prompt = if guidance.is_empty() {
            base_prompt
        } else {
            format!("{}\n{}", base_prompt, guidance)
        };

        eprintln!("[AGENT] Strategy: {:?} (guidance={})", strategy, !guidance.is_empty());

        // 5. Build initial messages
        let messages = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": prefetch_results}),
        ];

        // 6. Run the agent
        let primary_result = self
            .run_agent(
                question,
                &strategy,
                Arc::clone(&tool_executor),
                Arc::clone(&self.llm),
                messages.clone(),
            )
            .await?;

        let mut answer = primary_result.answer.clone();
        let mut abstained = is_prompt_abstention(&answer);
        let mut cost = primary_result.cost;
        let mut iterations = primary_result.iterations;
        let mut loop_break = primary_result.loop_break;
        let mut loop_break_reason = primary_result.loop_break_reason.clone();
        let mut fallback_used = false;
        let mut fallback_reason = None;

        // 7. Ensemble fallback routing
        if let Some(ref ensemble) = self.config.ensemble {
            if ensemble.enabled {
                if let Some(ref fallback_llm) = self.fallback_llm {
                    let should_fb = self.should_fallback(
                        abstained,
                        loop_break,
                        &strategy,
                        iterations,
                        ensemble,
                    );
                    if should_fb {
                        let trigger = if loop_break {
                            format!("loop-broke ({:?})", loop_break_reason)
                        } else if strategy == QuestionStrategy::Enumeration
                            && iterations as usize >= ensemble.enum_uncertainty_min_iterations
                            && !abstained
                        {
                            format!("enum-uncertainty ({}iter)", iterations)
                        } else {
                            "abstained".to_string()
                        };
                        eprintln!("[ENSEMBLE] Primary {}. Falling back.", trigger);

                        // Re-run with fallback LLM (fresh ToolExecutor + hook)
                        let fb_executor = ToolExecutor::new(
                            Arc::clone(&self.storage),
                            Arc::clone(&self.embedding_provider),
                        )
                        .with_user_id(user_id)
                        .with_reference_date(question_date);

                        let fb_executor = Arc::new(fb_executor);
                        let fb_prefetch = self.prefetch(question, &fb_executor).await;
                        let fb_messages = vec![
                            serde_json::json!({"role": "system", "content": system_prompt}),
                            serde_json::json!({"role": "user", "content": fb_prefetch}),
                        ];

                        let fb_result = self
                            .run_agent(
                                question,
                                &strategy,
                                fb_executor,
                                Arc::clone(fallback_llm),
                                fb_messages,
                            )
                            .await?;

                        let fb_reason = if loop_break {
                            format!("loop_break:{:?}", loop_break_reason)
                        } else if strategy == QuestionStrategy::Enumeration
                            && iterations as usize >= ensemble.enum_uncertainty_min_iterations
                            && !abstained
                        {
                            format!("enum_uncertainty:{}iter", iterations)
                        } else {
                            "abstention".to_string()
                        };

                        answer = fb_result.answer.clone();
                        abstained = is_prompt_abstention(&answer);
                        cost += fb_result.cost;
                        iterations = fb_result.iterations;
                        loop_break = fb_result.loop_break;
                        loop_break_reason = fb_result.loop_break_reason.clone();
                        fallback_used = true;
                        fallback_reason = Some(fb_reason);
                    }
                }
            }
        }

        let total_time = start.elapsed().as_millis() as u64;
        eprintln!(
            "[AGENT] Total: {}ms, ${:.4}, {} iterations",
            total_time, cost, iterations,
        );

        Ok(MemoryAnswerResult {
            answer,
            abstained,
            cost,
            total_time_ms: total_time,
            iterations,
            loop_break,
            loop_break_reason,
            strategy,
            fallback_used,
            fallback_reason,
        })
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// Run the agent loop with given tools, LLM, and messages.
    async fn run_agent(
        &self,
        question_text: &str,
        strategy: &QuestionStrategy,
        tool_executor: Arc<ToolExecutor>,
        llm: Arc<dyn LlmClient>,
        messages: Vec<serde_json::Value>,
    ) -> Result<crate::types::AgentResult, AgentError> {
        let agent_config = AgentConfig {
            model: llm
                .model_name()
                .unwrap_or(&self.config.model)
                .to_string(),
            temperature: self.config.temperature,
            max_iterations: self.config.max_iterations,
            cost_limit: self.config.cost_limit,
            consecutive_dupe_limit: self.config.gates.loop_break_consecutive_dupes as u32,
            tool_result_limit: self.config.tool_result_limit,
        };

        let tools = wrap_tools(tool_executor);

        let hook = MemoryAgentHook::new(
            strategy.clone(),
            self.config.gates.clone(),
            question_text.to_string(),
            self.config.tool_result_limit,
            false, // skip_anti_abstention: production default
        );

        let agent = Agent::new(agent_config, tools, llm).with_hook(Box::new(hook));
        agent.run(messages).await
    }

    /// Prefetch initial results (parallel search of facts + messages).
    async fn prefetch(&self, question: &str, executor: &ToolExecutor) -> String {
        let explicit_k = self.config.prefetch_explicit;
        let deductive_k = self.config.prefetch_deductive;
        let messages_k = self.config.prefetch_messages;

        let explicit_args =
            serde_json::json!({"query": question, "top_k": explicit_k, "level": "explicit"});
        let deductive_args =
            serde_json::json!({"query": question, "top_k": deductive_k, "level": "deductive"});
        let messages_args = serde_json::json!({"query": question, "top_k": messages_k});

        let (explicit_result, deductive_result, messages_result) = tokio::join!(
            executor.execute("search_facts", &explicit_args),
            executor.execute("search_facts", &deductive_args),
            executor.execute("search_messages", &messages_args),
        );

        let explicit_text = explicit_result.unwrap_or_else(|e| format!("Error: {}", e));
        let deductive_text = deductive_result.unwrap_or_else(|e| format!("Error: {}", e));
        let messages_text = messages_result.unwrap_or_else(|e| format!("Error: {}", e));

        format!(
            "=== Prefetched Explicit Facts ===\n{}\n\n=== Prefetched Deductive Facts ===\n{}\n\n=== Prefetched Messages ===\n{}",
            explicit_text, deductive_text, messages_text
        )
    }

    /// Check if ensemble fallback should be triggered.
    fn should_fallback(
        &self,
        abstained: bool,
        loop_break: bool,
        strategy: &QuestionStrategy,
        iterations: u32,
        ensemble: &engram_core::config::EnsembleConfig,
    ) -> bool {
        (ensemble.fallback_on_abstention && abstained && !loop_break)
            || (ensemble.fallback_on_loop_break && loop_break)
            || (ensemble.fallback_on_enum_uncertainty
                && *strategy == QuestionStrategy::Enumeration
                && iterations as usize >= ensemble.enum_uncertainty_min_iterations
                && !abstained)
    }
}

/// Check if a proposed answer is an abstention.
fn is_prompt_abstention(answer: &str) -> bool {
    let lower = answer.to_lowercase();
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
