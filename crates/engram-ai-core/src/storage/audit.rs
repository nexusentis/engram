use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::database::Database;
use crate::error::Result;
use crate::types::Memory;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Create,
    Update,
    Delete,
    Search,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::Search => write!(f, "search"),
        }
    }
}

#[derive(Debug)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: String,
    pub operation: Operation,
    pub user_id: String,
    pub memory_id: Option<String>,
    pub session_id: Option<String>,
    pub collection: Option<String>,
    pub payload_before: Option<String>,
    pub payload_after: Option<String>,
    pub metadata: Option<String>,
}

pub struct AuditLog<'a> {
    db: &'a Database,
}

impl<'a> AuditLog<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Log a memory creation
    pub fn log_create(&self, memory: &Memory) -> Result<i64> {
        let payload = serde_json::to_string(memory)?;

        self.db.conn().execute(
            "INSERT INTO audit_log (operation, user_id, memory_id, session_id, collection, payload_after)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Operation::Create.to_string(),
                memory.user_id,
                memory.id.to_string(),
                memory.session_id,
                memory.collection(),
                payload,
            ],
        ).map_err(crate::error::StorageError::Sqlite)?;

        Ok(self.db.conn().last_insert_rowid())
    }

    /// Log a memory update
    pub fn log_update(&self, before: &Memory, after: &Memory) -> Result<i64> {
        let before_json = serde_json::to_string(before)?;
        let after_json = serde_json::to_string(after)?;

        self.db.conn().execute(
            "INSERT INTO audit_log (operation, user_id, memory_id, session_id, collection, payload_before, payload_after)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                Operation::Update.to_string(),
                after.user_id,
                after.id.to_string(),
                after.session_id,
                after.collection(),
                before_json,
                after_json,
            ],
        ).map_err(crate::error::StorageError::Sqlite)?;

        Ok(self.db.conn().last_insert_rowid())
    }

    /// Log a memory deletion
    pub fn log_delete(&self, memory: &Memory) -> Result<i64> {
        let payload = serde_json::to_string(memory)?;

        self.db.conn().execute(
            "INSERT INTO audit_log (operation, user_id, memory_id, session_id, collection, payload_before)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Operation::Delete.to_string(),
                memory.user_id,
                memory.id.to_string(),
                memory.session_id,
                memory.collection(),
                payload,
            ],
        ).map_err(crate::error::StorageError::Sqlite)?;

        Ok(self.db.conn().last_insert_rowid())
    }

    /// Log a search operation
    pub fn log_search(&self, user_id: &str, query: &str, result_count: usize) -> Result<i64> {
        let metadata = serde_json::json!({
            "query": query,
            "result_count": result_count,
        });

        self.db
            .conn()
            .execute(
                "INSERT INTO audit_log (operation, user_id, metadata)
             VALUES (?1, ?2, ?3)",
                params![Operation::Search.to_string(), user_id, metadata.to_string(),],
            )
            .map_err(crate::error::StorageError::Sqlite)?;

        Ok(self.db.conn().last_insert_rowid())
    }

    /// Get audit history for a memory
    pub fn get_memory_history(&self, user_id: &str, memory_id: &str) -> Result<Vec<AuditEntry>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, timestamp, operation, user_id, memory_id, session_id, collection, payload_before, payload_after, metadata
             FROM audit_log
             WHERE user_id = ?1 AND memory_id = ?2
             ORDER BY timestamp DESC"
        ).map_err(crate::error::StorageError::Sqlite)?;

        let entries = stmt
            .query_map(params![user_id, memory_id], |row| {
                Ok(AuditEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    operation: match row.get::<_, String>(2)?.as_str() {
                        "create" => Operation::Create,
                        "update" => Operation::Update,
                        "delete" => Operation::Delete,
                        _ => Operation::Search,
                    },
                    user_id: row.get(3)?,
                    memory_id: row.get(4)?,
                    session_id: row.get(5)?,
                    collection: row.get(6)?,
                    payload_before: row.get(7)?,
                    payload_after: row.get(8)?,
                    metadata: row.get(9)?,
                })
            })
            .map_err(crate::error::StorageError::Sqlite)?;

        entries
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| crate::error::StorageError::Sqlite(e).into())
    }

    /// Get total operation counts
    pub fn get_stats(&self) -> Result<(i64, i64, i64, i64)> {
        let mut stmt = self
            .db
            .conn()
            .prepare(
                "SELECT
                SUM(CASE WHEN operation = 'create' THEN 1 ELSE 0 END),
                SUM(CASE WHEN operation = 'update' THEN 1 ELSE 0 END),
                SUM(CASE WHEN operation = 'delete' THEN 1 ELSE 0 END),
                SUM(CASE WHEN operation = 'search' THEN 1 ELSE 0 END)
             FROM audit_log",
            )
            .map_err(crate::error::StorageError::Sqlite)?;

        stmt.query_row([], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        })
        .map_err(|e| crate::error::StorageError::Sqlite(e).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteConfig;
    use crate::types::Memory;
    use tempfile::TempDir;

    fn setup_db() -> (TempDir, Database) {
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
        (temp_dir, db)
    }

    #[test]
    fn test_log_create() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);
        let memory = Memory::new("user_123", "Test content");

        let id = audit.log_create(&memory).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_log_update() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);
        let before = Memory::new("user_123", "Original content");
        let mut after = before.clone();
        after.content = "Updated content".to_string();

        let id = audit.log_update(&before, &after).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_log_delete() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);
        let memory = Memory::new("user_123", "Content to delete");

        let id = audit.log_delete(&memory).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_log_search() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);

        let id = audit.log_search("user_123", "test query", 5).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_get_stats() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);
        let memory = Memory::new("user_123", "Test");

        audit.log_create(&memory).unwrap();
        audit.log_create(&memory).unwrap();
        audit.log_search("user_123", "query", 3).unwrap();

        let (creates, updates, deletes, searches) = audit.get_stats().unwrap();
        assert_eq!(creates, 2);
        assert_eq!(updates, 0);
        assert_eq!(deletes, 0);
        assert_eq!(searches, 1);
    }

    #[test]
    fn test_get_memory_history() {
        let (_dir, db) = setup_db();
        let audit = AuditLog::new(&db);
        let memory = Memory::new("user_123", "Test");
        let memory_id = memory.id.to_string();

        audit.log_create(&memory).unwrap();

        let mut updated = memory.clone();
        updated.content = "Updated".to_string();
        audit.log_update(&memory, &updated).unwrap();

        let history = audit.get_memory_history("user_123", &memory_id).unwrap();
        assert_eq!(history.len(), 2);
    }
}
