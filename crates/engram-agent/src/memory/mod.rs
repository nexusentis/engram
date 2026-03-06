//! Memory agent domain: gates, tool adapter, and MemoryAgent runner.

pub mod gates;
pub mod runner;
pub mod tool_adapter;

pub use gates::MemoryAgentHook;
pub use runner::{MemoryAgent, MemoryAnswerResult};
