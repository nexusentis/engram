//! Memory agent domain logic.
//!
//! This module provides the core building blocks for the memory answering agent:
//! - Tool execution logic (search, grep, context retrieval, date arithmetic)
//! - Tool JSON schema definitions
//! - Date expression parsing and formatting
//! - Question strategy detection (temporal, enumeration, update, preference)
//! - Strategy-aware prompt templates
//!
//! The `engram-agent` crate wraps these into `Tool` and `AgentHook` trait
//! implementations and provides the `MemoryAgent` orchestrator.

pub mod date_parsing;
pub mod prompting;
pub mod strategy;
pub mod tool_schemas;
pub mod tool_types;
pub mod tools;

pub use date_parsing::format_date_header;
pub use prompting::{build_agent_system_prompt, strategy_guidance};
pub use strategy::{
    QuestionStrategy, detect_question_strategy, is_counting_question, is_sum_question,
};
pub use tool_schemas::{done_schema, tool_schemas};
pub use tool_types::{ToolExecutionResult, get_int_payload, get_string_payload};
pub use tools::ToolExecutor;
