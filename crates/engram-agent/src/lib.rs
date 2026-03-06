//! engram-agent: Reusable LLM agent loop with tool-calling and lifecycle hooks.
//!
//! This crate provides a generic agent loop that:
//! - Calls an LLM with tool schemas
//! - Dispatches tool calls to [`Tool`] implementations
//! - Applies lifecycle hooks via [`AgentHook`]
//! - Handles duplicate detection, cost limits, and iteration limits
//!
//! # Architecture
//!
//! ```text
//! engram-core (owns LlmClient trait + HttpLlmClient impl)
//!     ↑
//! engram-agent (owns Agent loop + Tool/AgentHook traits)
//!     ↑
//! engram-bench (implements Tool for memory tools, AgentHook for gates)
//! engram-server (future: implements Tool for REST-backed tools)
//! ```

mod agent;
mod config;
mod error;
mod hook;
#[cfg(feature = "memory")]
pub mod memory;
mod tool;
mod types;

pub use agent::Agent;
pub use config::AgentConfig;
pub use error::AgentError;
pub use hook::AgentHook;
#[cfg(feature = "memory")]
pub use memory::{MemoryAgent, MemoryAgentHook, MemoryAnswerResult};
pub use tool::Tool;
pub use types::{AgentResult, LoopBreakReason, LoopState, ToolEvent, ToolTraceEntry};
