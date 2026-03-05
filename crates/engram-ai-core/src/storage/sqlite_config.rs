use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    /// Path to SQLite database file
    pub path: String,
    /// Enable WAL mode for better concurrency
    pub wal_mode: bool,
    /// Busy timeout in milliseconds
    pub busy_timeout_ms: u32,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: "./data/engram.db".to_string(),
            wal_mode: true,
            busy_timeout_ms: 5000,
        }
    }
}
