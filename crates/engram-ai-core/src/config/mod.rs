//! Configuration management
//!
//! Provides TOML-based configuration with defaults.

mod settings;

pub use settings::{
    AgentConfig, Config, EnsembleConfig, ExtractionConfig, GateConfig, RetrievalConfig,
    SecurityConfig, ServerConfig,
};
