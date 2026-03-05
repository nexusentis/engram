//! Configuration management
//!
//! Provides TOML-based configuration with defaults.

mod settings;

pub use settings::{Config, ExtractionConfig, RetrievalConfig, SecurityConfig, ServerConfig};
