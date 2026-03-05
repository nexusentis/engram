//! Generic LLM agent loop with tool-calling, duplicate detection, and hooks.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{self, Value};
use tracing;

use engram_core::llm::types::{AgentResponse, CompletionResult};
use engram_core::llm::LlmClient;

use crate::config::AgentConfig;
use crate::error::AgentError;
use crate::hook::{AgentHook, NoOpHook};
use crate::tool::Tool;
use crate::types::{AgentResult, LoopBreakReason, LoopState, ToolEvent, ToolTraceEntry};

/// A reusable LLM agent that iteratively calls tools to answer a question.
///
/// The agent loop is generic — all domain-specific behavior (gates, truncation
/// direction, strategy) is injected via [`Tool`] and [`AgentHook`] implementations.
pub struct Agent {
    config: AgentConfig,
    tools: Vec<Box<dyn Tool>>,
    llm: Arc<dyn LlmClient>,
    hook: Box<dyn AgentHook>,
}

impl Agent {
    /// Create a new agent with the given config, tools, and LLM client.
    pub fn new(
        config: AgentConfig,
        tools: Vec<Box<dyn Tool>>,
        llm: Arc<dyn LlmClient>,
    ) -> Self {
        Self {
            config,
            tools,
            llm,
            hook: Box::new(NoOpHook),
        }
    }

    /// Attach a lifecycle hook for customizing agent behavior.
    pub fn with_hook(mut self, hook: Box<dyn AgentHook>) -> Self {
        self.hook = hook;
        self
    }

    /// Run the agent loop with the given initial messages.
    ///
    /// Returns an [`AgentResult`] with the final answer, cost, and trace data.
    pub async fn run(&self, mut messages: Vec<Value>) -> Result<AgentResult, AgentError> {
        // Collect tool schemas (including done, which is handled specially)
        let tool_schemas: Vec<Value> = self.tools.iter().map(|t| t.schema()).collect();

        let mut total_cost = 0.0f32;
        let mut total_prompt_tokens = 0u64;
        let mut total_completion_tokens = 0u64;
        let mut answer = String::new();
        let mut tool_trace: Vec<ToolTraceEntry> = Vec::new();
        let mut tool_events: Vec<ToolEvent> = Vec::new();
        let mut seen_tool_calls: HashSet<String> = HashSet::new();
        let mut consecutive_dupes = 0u32;
        let mut loop_break = false;
        let mut loop_break_reason: Option<LoopBreakReason> = None;
        let mut last_iteration = 0u32;

        for iteration in 0..self.config.max_iterations {
            last_iteration = (iteration + 1) as u32;
            let result: CompletionResult = self
                .llm
                .complete_with_tools(
                    &self.config.model,
                    &messages,
                    &tool_schemas,
                    self.config.temperature,
                )
                .await?;

            total_cost += result.cost;
            total_prompt_tokens += result.prompt_tokens;
            total_completion_tokens += result.completion_tokens;

            match result.response {
                AgentResponse::ToolCalls(calls) => {
                    // Append assistant message with raw tool_calls JSON
                    let tool_calls_json: Vec<Value> = calls
                        .iter()
                        .map(|tc| {
                            if let Some(ref raw) = tc.raw_json {
                                raw.clone()
                            } else {
                                serde_json::json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                                    }
                                })
                            }
                        })
                        .collect();
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "tool_calls": tool_calls_json
                    }));

                    let mut all_dupes_this_iteration = true;

                    for tc in &calls {
                        // --- done() handling: checked BEFORE duplicate detection ---
                        if tc.name == "done" {
                            let loop_state = LoopState {
                                iteration: iteration as u32,
                                total_cost,
                                prompt_tokens: total_prompt_tokens,
                                completion_tokens: total_completion_tokens,
                                tool_trace: &tool_trace,
                                tool_events: &tool_events,
                                messages: &messages,
                            };

                            // Extract answer text; treat empty/missing as auto-rejection
                            let proposed_answer = tc.arguments["answer"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let done_validation = if proposed_answer.is_empty() {
                                Err("REJECTED: done() called without an answer.".to_string())
                            } else {
                                self.hook.validate_done(&tc.arguments, &loop_state)
                            };

                            match done_validation {
                                Ok(()) => {
                                    // Accepted
                                    answer = proposed_answer;
                                    tracing::debug!(
                                        iteration = iteration + 1,
                                        answer = %answer,
                                        "Agent done"
                                    );

                                    // Record the done event
                                    tool_events.push(ToolEvent {
                                        tool_name: "done".to_string(),
                                        tool_call_id: tc.id.clone(),
                                        args: tc.arguments.clone(),
                                        result: answer.clone(),
                                        success: true,
                                        duplicate: false,
                                    });
                                }
                                Err(rejection) => {
                                    // Rejected — inject rejection as tool result
                                    tracing::debug!(
                                        iteration = iteration + 1,
                                        rejection = %rejection,
                                        "Done rejected by hook"
                                    );
                                    messages.push(serde_json::json!({
                                        "role": "tool",
                                        "tool_call_id": tc.id,
                                        "content": rejection
                                    }));

                                    tool_events.push(ToolEvent {
                                        tool_name: "done".to_string(),
                                        tool_call_id: tc.id.clone(),
                                        args: tc.arguments.clone(),
                                        result: rejection,
                                        success: false,
                                        duplicate: false,
                                    });

                                    all_dupes_this_iteration = false;
                                }
                            }

                            if !answer.is_empty() {
                                break; // inner for loop
                            }
                            continue;
                        }

                        // --- Duplicate detection ---
                        let call_key = format!(
                            "{}:{}",
                            tc.name,
                            serde_json::to_string(&tc.arguments).unwrap_or_default()
                        );
                        if !seen_tool_calls.insert(call_key) {
                            tracing::debug!(
                                iteration = iteration + 1,
                                tool = %tc.name,
                                "Duplicate tool call (skipped)"
                            );
                            tool_trace.push(ToolTraceEntry {
                                tool: tc.name.clone(),
                                iteration: (iteration + 1) as u32,
                                chars: 0,
                                duplicate: true,
                            });
                            tool_events.push(ToolEvent {
                                tool_name: tc.name.clone(),
                                tool_call_id: tc.id.clone(),
                                args: tc.arguments.clone(),
                                result: String::new(),
                                success: true,
                                duplicate: true,
                            });
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tc.id,
                                "content": "You already made this exact search. Try a different query, use a different tool, or call 'done' with your best answer (or 'I don't have enough information')."
                            }));
                            continue;
                        }
                        all_dupes_this_iteration = false;

                        // --- Pre-execution hook ---
                        let loop_state = LoopState {
                            iteration: iteration as u32,
                            total_cost,
                            prompt_tokens: total_prompt_tokens,
                            completion_tokens: total_completion_tokens,
                            tool_trace: &tool_trace,
                            tool_events: &tool_events,
                            messages: &messages,
                        };

                        if let Err(rejection) =
                            self.hook.pre_tool_execute(&tc.name, &tc.arguments, &loop_state)
                        {
                            tracing::debug!(
                                iteration = iteration + 1,
                                tool = %tc.name,
                                rejection = %rejection,
                                "Tool rejected by pre_tool_execute hook"
                            );
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tc.id,
                                "content": rejection
                            }));

                            tool_events.push(ToolEvent {
                                tool_name: tc.name.clone(),
                                tool_call_id: tc.id.clone(),
                                args: tc.arguments.clone(),
                                result: rejection,
                                success: false,
                                duplicate: false,
                            });
                            continue;
                        }

                        // --- Execute tool ---
                        let tool = self.tools.iter().find(|t| t.name() == tc.name);
                        let result_text = match tool {
                            Some(t) => t
                                .execute(tc.arguments.clone())
                                .await
                                .unwrap_or_else(|e| format!("Error: {}", e)),
                            None => format!("Error: unknown tool '{}'", tc.name),
                        };
                        let success = !result_text.starts_with("Error:");

                        // --- Post-execution hook ---
                        let loop_state = LoopState {
                            iteration: iteration as u32,
                            total_cost,
                            prompt_tokens: total_prompt_tokens,
                            completion_tokens: total_completion_tokens,
                            tool_trace: &tool_trace,
                            tool_events: &tool_events,
                            messages: &messages,
                        };
                        let result_text =
                            self.hook
                                .post_tool_execute(&tc.name, result_text, &loop_state);

                        // --- Default truncation (keep start, at line boundaries) ---
                        let truncated = if result_text.len() > self.config.tool_result_limit {
                            truncate_at_line_boundary(
                                &result_text,
                                self.config.tool_result_limit,
                                false,
                            )
                        } else {
                            result_text
                        };

                        tracing::debug!(
                            iteration = iteration + 1,
                            tool = %tc.name,
                            chars = truncated.len(),
                            "Tool executed"
                        );

                        tool_trace.push(ToolTraceEntry {
                            tool: tc.name.clone(),
                            iteration: (iteration + 1) as u32,
                            chars: truncated.len(),
                            duplicate: false,
                        });
                        tool_events.push(ToolEvent {
                            tool_name: tc.name.clone(),
                            tool_call_id: tc.id.clone(),
                            args: tc.arguments.clone(),
                            result: truncated.clone(),
                            success,
                            duplicate: false,
                        });

                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": truncated
                        }));
                    }

                    // Break outer loop if we got an answer
                    if !answer.is_empty() {
                        break;
                    }

                    // Consecutive duplicate detection
                    if all_dupes_this_iteration {
                        consecutive_dupes += 1;
                        if consecutive_dupes >= self.config.consecutive_dupe_limit {
                            tracing::debug!(
                                consecutive_dupes,
                                "Breaking: consecutive duplicate iterations"
                            );
                            answer = "I don't have enough information to answer this question."
                                .to_string();
                            loop_break = true;
                            loop_break_reason = Some(LoopBreakReason::DuplicateDetection);
                            break;
                        }
                    } else {
                        consecutive_dupes = 0;
                    }

                    // Cost circuit breaker
                    if total_cost > self.config.cost_limit {
                        tracing::debug!(
                            cost = total_cost,
                            limit = self.config.cost_limit,
                            "Breaking: cost limit exceeded"
                        );
                        answer =
                            "I don't have enough information to answer this question.".to_string();
                        loop_break = true;
                        loop_break_reason = Some(LoopBreakReason::CostLimit);
                        break;
                    }
                }

                AgentResponse::TextResponse(text) => {
                    tracing::debug!(
                        iteration = iteration + 1,
                        text_len = text.len(),
                        "Text response (no tool calls)"
                    );
                    answer = text;
                    break;
                }
            }
        }

        // Fallback if no answer after all iterations
        if answer.is_empty() {
            tracing::debug!(
                max_iterations = self.config.max_iterations,
                "No answer after max iterations"
            );
            answer = "I don't have enough information to answer this question.".to_string();
            loop_break = true;
            loop_break_reason = Some(LoopBreakReason::IterationExhaustion);
        }

        Ok(AgentResult {
            answer,
            cost: total_cost,
            prompt_tokens: total_prompt_tokens,
            completion_tokens: total_completion_tokens,
            iterations: last_iteration,
            tool_trace,
            tool_events,
            loop_break,
            loop_break_reason,
        })
    }
}

/// Truncate text at a line boundary to stay within a character limit.
///
/// If `keep_end` is true, keeps the end of the text (newest content);
/// otherwise keeps the start (oldest content).
fn truncate_at_line_boundary(text: &str, limit: usize, keep_end: bool) -> String {
    if text.len() <= limit {
        return text.to_string();
    }

    if keep_end {
        // Keep the END of the text (for Update questions: newest dates)
        let mut skip = text.len() - limit;
        // Normalize to a char boundary before slicing
        while skip < text.len() && !text.is_char_boundary(skip) {
            skip += 1;
        }
        // Find the next newline after `skip` to get a clean line boundary
        if let Some(pos) = text[skip..].find('\n') {
            let start = skip + pos + 1;
            format!("...(truncated, showing newest)\n{}", &text[start..])
        } else {
            format!("...(truncated)\n{}", &text[skip..])
        }
    } else {
        // Keep the START of the text (default: oldest content)
        // Normalize search_end to a char boundary before slicing
        let mut search_end = limit.min(text.len());
        while search_end > 0 && !text.is_char_boundary(search_end) {
            search_end -= 1;
        }
        if let Some(pos) = text[..search_end].rfind('\n') {
            format!("{}\n...(truncated)", &text[..pos])
        } else {
            format!("{}...(truncated)", &text[..search_end])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_at_line_boundary_no_truncation() {
        let text = "line 1\nline 2\nline 3";
        assert_eq!(truncate_at_line_boundary(text, 100, false), text);
    }

    #[test]
    fn test_truncate_at_line_boundary_keep_start() {
        let text = "line 1\nline 2\nline 3\nline 4\nline 5";
        let result = truncate_at_line_boundary(text, 20, false);
        assert!(result.len() <= 40); // original truncation point + suffix
        assert!(result.ends_with("...(truncated)"));
        assert!(result.contains("line 1"));
    }

    #[test]
    fn test_truncate_at_line_boundary_keep_end() {
        let text = "line 1\nline 2\nline 3\nline 4\nline 5";
        let result = truncate_at_line_boundary(text, 20, true);
        assert!(result.starts_with("...(truncated"));
        assert!(result.contains("line 5"));
    }
}
