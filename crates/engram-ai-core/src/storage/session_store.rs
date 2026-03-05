use rusqlite::params;

use super::database::Database;
use crate::error::Result;
use crate::types::Session;

pub struct SessionStore<'a> {
    db: &'a Database,
}

impl<'a> SessionStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Create or update a session
    pub fn upsert(&self, session: &Session) -> Result<()> {
        self.db.conn().execute(
            "INSERT INTO sessions (id, user_id, created_at, updated_at, message_count, memory_count, timeline_summary, entity_mentions, topic_tags, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                 updated_at = ?4,
                 message_count = ?5,
                 memory_count = ?6,
                 timeline_summary = ?7,
                 entity_mentions = ?8,
                 topic_tags = ?9,
                 metadata = ?10",
            params![
                session.id,
                session.user_id,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
                session.message_count,
                session.memory_count,
                session.timeline_summary.as_ref().map(|v| v.to_string()),
                serde_json::to_string(&session.entity_mentions).ok(),
                serde_json::to_string(&session.topic_tags).ok(),
                session.metadata.as_ref().map(|v| v.to_string()),
            ],
        ).map_err(crate::error::StorageError::Sqlite)?;

        Ok(())
    }

    /// Get a session by ID
    pub fn get(&self, user_id: &str, session_id: &str) -> Result<Option<Session>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, user_id, created_at, updated_at, message_count, memory_count, timeline_summary, entity_mentions, topic_tags, metadata
             FROM sessions
             WHERE id = ?1 AND user_id = ?2"
        ).map_err(crate::error::StorageError::Sqlite)?;

        let session = stmt.query_row(params![session_id, user_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                user_id: row.get(1)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                message_count: row.get(4)?,
                memory_count: row.get(5)?,
                timeline_summary: row
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                entity_mentions: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                topic_tags: row
                    .get::<_, Option<String>>(8)?
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                metadata: row
                    .get::<_, Option<String>>(9)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
            })
        });

        match session {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::error::StorageError::Sqlite(e).into()),
        }
    }

    /// List sessions for a user
    pub fn list(&self, user_id: &str, limit: u32) -> Result<Vec<Session>> {
        let mut stmt = self.db.conn().prepare(
            "SELECT id, user_id, created_at, updated_at, message_count, memory_count, timeline_summary, entity_mentions, topic_tags, metadata
             FROM sessions
             WHERE user_id = ?1
             ORDER BY updated_at DESC
             LIMIT ?2"
        ).map_err(crate::error::StorageError::Sqlite)?;

        let sessions = stmt
            .query_map(params![user_id, limit], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    message_count: row.get(4)?,
                    memory_count: row.get(5)?,
                    timeline_summary: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    entity_mentions: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    topic_tags: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    metadata: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                })
            })
            .map_err(crate::error::StorageError::Sqlite)?;

        sessions
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| crate::error::StorageError::Sqlite(e).into())
    }

    /// Delete a session
    pub fn delete(&self, user_id: &str, session_id: &str) -> Result<bool> {
        let rows = self
            .db
            .conn()
            .execute(
                "DELETE FROM sessions WHERE id = ?1 AND user_id = ?2",
                params![session_id, user_id],
            )
            .map_err(crate::error::StorageError::Sqlite)?;

        Ok(rows > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteConfig;
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
    fn test_session_upsert_and_get() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);
        let session = Session::new("user_123");

        store.upsert(&session).unwrap();

        let retrieved = store.get("user_123", &session.id).unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, session.id);
        assert_eq!(retrieved.user_id, session.user_id);
    }

    #[test]
    fn test_session_get_nonexistent() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);

        let result = store.get("user_123", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_session_user_isolation() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);
        let session = Session::new("user_123");

        store.upsert(&session).unwrap();

        // Different user should not see the session
        let result = store.get("different_user", &session.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_session_list() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);

        let session1 = Session::new("user_123");
        let session2 = Session::new("user_123");
        let session3 = Session::new("other_user");

        store.upsert(&session1).unwrap();
        store.upsert(&session2).unwrap();
        store.upsert(&session3).unwrap();

        let sessions = store.list("user_123", 10).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_session_update() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);
        let mut session = Session::new("user_123");

        store.upsert(&session).unwrap();

        session.record_message();
        session.record_message();
        store.upsert(&session).unwrap();

        let retrieved = store.get("user_123", &session.id).unwrap().unwrap();
        assert_eq!(retrieved.message_count, 2);
    }

    #[test]
    fn test_session_delete() {
        let (_dir, db) = setup_db();
        let store = SessionStore::new(&db);
        let session = Session::new("user_123");

        store.upsert(&session).unwrap();

        let deleted = store.delete("user_123", &session.id).unwrap();
        assert!(deleted);

        let result = store.get("user_123", &session.id).unwrap();
        assert!(result.is_none());
    }
}
