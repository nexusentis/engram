use rusqlite::Connection;
use std::path::Path;

use super::sqlite_config::SqliteConfig;
use crate::error::{Result, StorageError};

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create the database
    pub fn open(config: &SqliteConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(&config.path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&config.path).map_err(StorageError::Sqlite)?;

        // Configure connection
        conn.busy_timeout(std::time::Duration::from_millis(
            config.busy_timeout_ms as u64,
        ))
        .map_err(StorageError::Sqlite)?;

        if config.wal_mode {
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(StorageError::Sqlite)?;
        }

        // Enable foreign keys
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(StorageError::Sqlite)?;

        let db = Self { conn };
        db.migrate()?;

        Ok(db)
    }

    /// Run all pending migrations
    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(MIGRATION_V001)
            .map_err(StorageError::Sqlite)?;
        Ok(())
    }

    /// Get a reference to the connection
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

const MIGRATION_V001: &str = r#"
-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now')),
    description TEXT
);

-- Insert initial version if not exists
INSERT OR IGNORE INTO schema_version (version, description)
VALUES (1, 'Initial schema');

-- Audit log
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    operation TEXT NOT NULL CHECK (operation IN ('create', 'update', 'delete', 'search')),
    user_id TEXT NOT NULL,
    memory_id TEXT,
    session_id TEXT,
    collection TEXT,
    payload_before TEXT,
    payload_after TEXT,
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_user_id ON audit_log(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_memory_id ON audit_log(memory_id);
CREATE INDEX IF NOT EXISTS idx_audit_operation ON audit_log(operation);

-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    message_count INTEGER NOT NULL DEFAULT 0,
    memory_count INTEGER NOT NULL DEFAULT 0,
    timeline_summary TEXT,
    entity_mentions TEXT,
    topic_tags TEXT,
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_created_at ON sessions(created_at);

-- Entities
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    canonical_name TEXT NOT NULL,
    aliases TEXT,
    first_seen TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen TEXT NOT NULL DEFAULT (datetime('now')),
    mention_count INTEGER NOT NULL DEFAULT 1,
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_entities_user_id ON entities(user_id);
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
CREATE INDEX IF NOT EXISTS idx_entities_canonical_name ON entities(canonical_name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_user_canonical ON entities(user_id, canonical_name);

-- DAG Tags (for SwiftMem optimization)
CREATE TABLE IF NOT EXISTS dag_tags (
    id TEXT PRIMARY KEY,
    tag_name TEXT NOT NULL,
    parent_id TEXT REFERENCES dag_tags(id),
    centroid_vector BLOB,
    memory_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_dag_tags_name ON dag_tags(tag_name);
CREATE INDEX IF NOT EXISTS idx_dag_tags_parent ON dag_tags(parent_id);

-- Config overrides
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT
);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_database_open() {
        let temp_dir = TempDir::new().unwrap();
        let config = SqliteConfig {
            path: temp_dir
                .path()
                .join("test.db")
                .to_str()
                .unwrap()
                .to_string(),
            ..Default::default()
        };

        let db = Database::open(&config).unwrap();

        // Verify WAL mode
        let mode: String = db
            .conn()
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn test_schema_version_created() {
        let temp_dir = TempDir::new().unwrap();
        let config = SqliteConfig {
            path: temp_dir
                .path()
                .join("test.db")
                .to_str()
                .unwrap()
                .to_string(),
            ..Default::default()
        };

        let db = Database::open(&config).unwrap();

        let version: i32 = db
            .conn()
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_all_tables_created() {
        let temp_dir = TempDir::new().unwrap();
        let config = SqliteConfig {
            path: temp_dir
                .path()
                .join("test.db")
                .to_str()
                .unwrap()
                .to_string(),
            ..Default::default()
        };

        let db = Database::open(&config).unwrap();

        let tables: Vec<String> = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"audit_log".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"entities".to_string()));
        assert!(tables.contains(&"dag_tags".to_string()));
        assert!(tables.contains(&"config".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }
}
