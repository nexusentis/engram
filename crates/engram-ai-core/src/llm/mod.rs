//! Shared LLM HTTP client, configuration types, and protocol types.
//!
//! This module provides the core LLM interaction layer used by the benchmark
//! harness, extraction pipeline, and other components that need to call
//! LLM APIs.

mod client;
pub mod config;
mod traits;
pub mod types;

pub use client::{AuthStyle, HttpLlmClient};
pub use config::{LlmClientConfig, ModelProfile, ModelRegistry};
pub use traits::LlmClient;
pub use types::{
    estimate_cost, set_global_model_registry, AgentResponse, CompletionResult, ToolCall,
};
