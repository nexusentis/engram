//! SurrealDB-backed persistent knowledge graph
//!
//! Stores entities, typed relationships, and fact mentions extracted during ingestion.
//! Provides graph traversal and entity disambiguation for the answering agent.
//!
//! # Architecture
//!
//! - **Entities**: Keyed by auto-generated SurrealDB record ID (NOT by name, to avoid conflation)
//! - **Relationships**: Typed graph edges (relates_to) between entities
//! - **Mentions**: Links entities to Qdrant fact IDs and sessions
//! - **User-scoped**: All data is partitioned by user_id
//!
//! # Storage Backends
//!
//! - `new_rocksdb(path)`: Persistent storage for production/benchmarks
//! - `new_memory()`: In-memory for tests

use std::collections::HashMap;

use anyhow::{Context, Result};
use surrealdb::sql::Datetime as SurrealDatetime;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem, RocksDb};
use surrealdb::sql::Thing;
use surrealdb::Surreal;

/// Persistent knowledge graph backed by SurrealDB
pub struct GraphStore {
    db: Surreal<Db>,
}

/// Entity node stored in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEntity {
    pub id: Option<Thing>,
    pub name: String,
    pub entity_type: String,
    pub user_id: String,
    pub aliases: Vec<String>,
    pub first_seen: SurrealDatetime,
    pub last_seen: SurrealDatetime,
    pub mention_count: i64,
}

/// Relationship edge stored in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRelationship {
    pub id: Option<Thing>,
    #[serde(rename = "in")]
    pub in_node: Option<Thing>,
    #[serde(rename = "out")]
    pub out_node: Option<Thing>,
    pub relation_type: String,
    pub user_id: String,
    pub confidence: f64,
    pub first_seen: SurrealDatetime,
    pub last_seen: SurrealDatetime,
}

/// Mention record linking entity to a fact in Qdrant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMention {
    pub id: Option<Thing>,
    pub entity_id: Thing,
    pub fact_id: String,
    pub session_id: String,
    pub user_id: String,
    pub timestamp: SurrealDatetime,
}

/// Entity with its relationships (returned by graph queries)
#[derive(Debug, Clone)]
pub struct EntityProfile {
    pub entity: GraphEntity,
    pub relationships: Vec<(GraphRelationship, GraphEntity, bool)>,
    pub mention_count: i64,
    pub session_count: usize,
}

/// Disambiguation candidate
#[derive(Debug, Clone)]
pub struct DisambiguationCandidate {
    pub entity: GraphEntity,
    pub relationships: Vec<(String, String)>, // (relation_type, target_name)
    pub score: f32,
}

// =========================================================================
// P7b-perf: Batch ingestion input types
// =========================================================================

/// Entity data for batch ingestion
#[derive(Debug, Clone)]
pub struct EntityInput {
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
}

/// Relationship data for batch ingestion
#[derive(Debug, Clone)]
pub struct RelationshipInput {
    pub subject_name: String,
    pub relation_type: String,
    pub object_name: String,
    pub confidence: f32,
}

/// Mention data for batch ingestion (entity→fact link)
#[derive(Debug, Clone)]
pub struct MentionInput {
    pub entity_name: String,
    pub entity_type: String,
    pub fact_id: String,
}

impl GraphStore {
    /// Create a new GraphStore with RocksDB persistent backend
    pub async fn new_rocksdb(path: &str) -> Result<Self> {
        let db = Surreal::new::<RocksDb>(path)
            .await
            .context("Failed to open SurrealDB with RocksDB backend")?;

        let store = Self { db };
        store.init().await?;
        Ok(store)
    }

    /// Create a new GraphStore with in-memory backend (for tests)
    pub async fn new_memory() -> Result<Self> {
        let db = Surreal::new::<Mem>(())
            .await
            .context("Failed to create in-memory SurrealDB")?;

        let store = Self { db };
        store.init().await?;
        Ok(store)
    }

    /// Initialize namespace, database, and schema
    async fn init(&self) -> Result<()> {
        self.db
            .use_ns("engram")
            .use_db("knowledge_graph")
            .await
            .context("Failed to set namespace/database")?;

        self.init_schema().await?;
        Ok(())
    }

    /// Define schema tables, fields, and indexes
    async fn init_schema(&self) -> Result<()> {
        self.db
            .query(
                "
                DEFINE TABLE IF NOT EXISTS entity SCHEMAFULL;
                DEFINE FIELD IF NOT EXISTS name ON entity TYPE string;
                DEFINE FIELD IF NOT EXISTS entity_type ON entity TYPE string;
                DEFINE FIELD IF NOT EXISTS user_id ON entity TYPE string;
                DEFINE FIELD IF NOT EXISTS aliases ON entity TYPE option<array<string>> DEFAULT [];
                DEFINE FIELD IF NOT EXISTS first_seen ON entity TYPE datetime;
                DEFINE FIELD IF NOT EXISTS last_seen ON entity TYPE datetime;
                DEFINE FIELD IF NOT EXISTS mention_count ON entity TYPE int DEFAULT 0;
                DEFINE INDEX IF NOT EXISTS idx_entity_user ON entity FIELDS user_id;
                DEFINE INDEX IF NOT EXISTS idx_entity_lookup ON entity FIELDS user_id, name;
                DEFINE INDEX IF NOT EXISTS idx_entity_type ON entity FIELDS user_id, entity_type;

                DEFINE TABLE IF NOT EXISTS relates_to SCHEMAFULL TYPE RELATION FROM entity TO entity;
                DEFINE FIELD IF NOT EXISTS relation_type ON relates_to TYPE string;
                DEFINE FIELD IF NOT EXISTS user_id ON relates_to TYPE string;
                DEFINE FIELD IF NOT EXISTS confidence ON relates_to TYPE float DEFAULT 1.0;
                DEFINE FIELD IF NOT EXISTS first_seen ON relates_to TYPE datetime;
                DEFINE FIELD IF NOT EXISTS last_seen ON relates_to TYPE datetime;
                DEFINE INDEX IF NOT EXISTS idx_rel_user ON relates_to FIELDS user_id;
                DEFINE INDEX IF NOT EXISTS idx_rel_type ON relates_to FIELDS user_id, relation_type;

                DEFINE TABLE IF NOT EXISTS mention SCHEMAFULL;
                DEFINE FIELD IF NOT EXISTS entity_id ON mention TYPE record<entity>;
                DEFINE FIELD IF NOT EXISTS fact_id ON mention TYPE string;
                DEFINE FIELD IF NOT EXISTS session_id ON mention TYPE string;
                DEFINE FIELD IF NOT EXISTS user_id ON mention TYPE string;
                DEFINE FIELD IF NOT EXISTS timestamp ON mention TYPE datetime;
                DEFINE INDEX IF NOT EXISTS idx_mention_entity ON mention FIELDS entity_id;
                DEFINE INDEX IF NOT EXISTS idx_mention_session ON mention FIELDS user_id, session_id;
                DEFINE INDEX IF NOT EXISTS idx_mention_fact ON mention FIELDS fact_id;
            ",
            )
            .await
            .context("Failed to define schema")?;

        Ok(())
    }

    // =========================================================================
    // Entity operations
    // =========================================================================

    /// Generate a deterministic entity record ID from (user_id, name, entity_type).
    /// This eliminates race conditions under concurrent ingestion — two concurrent
    /// upserts for the same entity will target the same record ID.
    fn entity_record_id(user_id: &str, name: &str, entity_type: &str) -> Thing {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(format!(
            "{}:{}:{}",
            user_id,
            name.to_lowercase(),
            entity_type.to_lowercase()
        ));
        let hash = hex::encode(hasher.finalize());
        // Use first 16 chars of hash as record ID (collision-safe for practical use)
        Thing::from(("entity", &hash[..16]))
    }

    /// Upsert an entity. If an entity with the same (user_id, name, entity_type) exists,
    /// update it. Otherwise create a new one. Returns the entity's record ID.
    ///
    /// Uses a deterministic record ID derived from (user_id, name_lower, entity_type_lower)
    /// to prevent duplicate creation under concurrent ingestion.
    ///
    /// P7b-perf: Single UPSERT eliminates the SELECT+CREATE/UPDATE read-then-write pattern.
    /// Server-side alias merging via array::union, null-coalesce for first_seen, atomic increment.
    pub async fn upsert_entity(
        &self,
        user_id: &str,
        name: &str,
        entity_type: &str,
        _session_id: &str,
        aliases: &[String],
    ) -> Result<Thing> {
        let now = SurrealDatetime::default();
        let record_id = Self::entity_record_id(user_id, name, entity_type);
        let aliases_lower: Vec<String> = aliases.iter().map(|a| a.to_lowercase()).collect();

        self.db
            .query(
                "UPSERT $id SET \
                    name = $name, \
                    entity_type = $entity_type, \
                    user_id = $user_id, \
                    aliases = array::union(aliases ?? [], $new_aliases), \
                    first_seen = first_seen ?? $now, \
                    last_seen = $now, \
                    mention_count = (mention_count ?? 0) + 1 \
                 RETURN NONE",
            )
            .bind(("id", record_id.clone()))
            .bind(("name", name.to_string()))
            .bind(("entity_type", entity_type.to_string()))
            .bind(("user_id", user_id.to_string()))
            .bind(("new_aliases", aliases_lower))
            .bind(("now", now))
            .await
            .context("Failed to upsert entity")?
            .check()
            .context("Entity upsert statement failed")?;

        Ok(record_id)
    }

    /// Get all entities matching a name for a user (may return multiple for disambiguation)
    pub async fn get_entities_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Vec<GraphEntity>> {
        let user_id = user_id.to_string();
        let name = name.to_string();
        let name_lower = name.to_lowercase();

        let mut result = self
            .db
            .query("SELECT * FROM entity WHERE user_id = $user_id AND (string::lowercase(name) = $name_lower OR $name_lower IN aliases)")
            .bind(("user_id", user_id))
            .bind(("name_lower", name_lower))
            .await
            .context("Failed to query entities by name")?;

        Ok(result.take(0).unwrap_or_default())
    }

    /// Find entities by type for a user
    pub async fn find_entities_by_type(
        &self,
        user_id: &str,
        entity_type: &str,
    ) -> Result<Vec<GraphEntity>> {
        let user_id = user_id.to_string();
        let entity_type = entity_type.to_string();

        let mut result = self
            .db
            .query("SELECT * FROM entity WHERE user_id = $user_id AND entity_type = $entity_type ORDER BY mention_count DESC")
            .bind(("user_id", user_id))
            .bind(("entity_type", entity_type))
            .await
            .context("Failed to query entities by type")?;

        Ok(result.take(0).unwrap_or_default())
    }

    /// Get a specific entity by its record ID
    pub async fn get_entity(&self, id: &Thing) -> Result<Option<GraphEntity>> {
        let id = id.clone();
        let mut result = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", id))
            .await
            .context("Failed to get entity by ID")?;

        let entities: Vec<GraphEntity> = result.take(0).unwrap_or_default();
        Ok(entities.into_iter().next())
    }

    /// Minimum keyword length for fuzzy substring search (avoids matching everything)
    const FUZZY_MIN_LENGTH: usize = 3;

    /// Fuzzy substring search: find entities whose name OR aliases contain the keyword.
    /// Returns results sorted by mention_count DESC. Requires keyword.len() >= FUZZY_MIN_LENGTH.
    pub async fn search_entities_fuzzy(
        &self,
        user_id: &str,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<GraphEntity>> {
        let keyword = keyword.trim().to_lowercase();
        if keyword.len() < Self::FUZZY_MIN_LENGTH {
            return Ok(vec![]);
        }

        let limit_i64 = limit as i64;

        let mut result = self
            .db
            .query(
                "SELECT * FROM entity \
                 WHERE user_id = $user_id \
                   AND (string::contains(string::lowercase(name), $keyword) \
                        OR aliases.any(|$a| string::contains(string::lowercase($a), $keyword))) \
                 ORDER BY mention_count DESC \
                 LIMIT $limit",
            )
            .bind(("user_id", user_id.to_string()))
            .bind(("keyword", keyword))
            .bind(("limit", limit_i64))
            .await
            .context("Failed to fuzzy search entities")?;

        Ok(result.take(0).unwrap_or_default())
    }

    // =========================================================================
    // Relationship operations
    // =========================================================================

    /// Generate a deterministic **directional** relationship record ID from
    /// (subject_id, relation_type, object_id). NOT sorted — preserves edge direction.
    fn relationship_record_id(
        subject_id: &Thing,
        relation_type: &str,
        object_id: &Thing,
    ) -> Thing {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!(
            "{}|{}|{}",
            subject_id,
            relation_type.to_lowercase(),
            object_id,
        ));
        let hash = hex::encode(hasher.finalize());
        Thing::from(("relates_to", &hash[..16]))
    }

    /// Upsert a relationship between two entities.
    ///
    /// P7b-perf: Uses INSERT RELATION ... ON DUPLICATE KEY UPDATE (1 query instead of 2).
    /// UPSERT does NOT work on TYPE RELATION tables (confirmed by Codex live test).
    pub async fn upsert_relationship(
        &self,
        user_id: &str,
        subject_id: &Thing,
        relation_type: &str,
        object_id: &Thing,
        confidence: f32,
    ) -> Result<()> {
        let rel_id = Self::relationship_record_id(subject_id, relation_type, object_id);

        self.db
            .query(
                "INSERT RELATION INTO relates_to { \
                    id: $id, in: $subject, out: $object, \
                    relation_type: $rel_type, user_id: $user_id, \
                    confidence: $conf, first_seen: time::now(), last_seen: time::now() \
                 } ON DUPLICATE KEY UPDATE \
                    confidence = math::max([confidence, $input.confidence]), \
                    last_seen = time::now() \
                 RETURN NONE",
            )
            .bind(("id", rel_id))
            .bind(("subject", subject_id.clone()))
            .bind(("object", object_id.clone()))
            .bind(("rel_type", relation_type.to_string()))
            .bind(("user_id", user_id.to_string()))
            .bind(("conf", confidence as f64))
            .await
            .context("Failed to upsert relationship")?
            .check()
            .context("Relationship upsert statement failed")?;

        Ok(())
    }

    /// Get all relationships for an entity (both outgoing and incoming).
    /// Returns (relationship, other_entity, is_outgoing) where is_outgoing=true means
    /// the queried entity is the subject (e.g. "Alice works_for Google" when querying Alice).
    pub async fn get_relationships_for(
        &self,
        user_id: &str,
        entity_id: &Thing,
    ) -> Result<Vec<(GraphRelationship, GraphEntity, bool)>> {
        let user_id = user_id.to_string();
        let entity_id = entity_id.clone();

        let mut result = self
            .db
            .query("SELECT * FROM relates_to WHERE (out = $entity_id OR `in` = $entity_id) AND user_id = $user_id")
            .bind(("entity_id", entity_id.clone()))
            .bind(("user_id", user_id))
            .await
            .context("Failed to query relationships")?;

        let rels: Vec<GraphRelationship> = result.take(0).unwrap_or_default();
        let mut relationships = Vec::new();

        for rel in rels {
            // SurrealDB RELATE $a->edge->$b stores in=$a, out=$b
            // is_outgoing = true when queried entity is the subject (in_node)
            let is_outgoing = rel.in_node.as_ref() == Some(&entity_id);
            let other_id = if is_outgoing {
                rel.out_node.as_ref()
            } else {
                rel.in_node.as_ref()
            };

            if let Some(other_id) = other_id {
                if let Ok(Some(other_entity)) = self.get_entity(other_id).await {
                    relationships.push((rel, other_entity, is_outgoing));
                }
            }
        }

        Ok(relationships)
    }

    /// Find entities related by a specific relationship type to a target
    pub async fn entities_related_by(
        &self,
        user_id: &str,
        relation_type: &str,
        target_name: &str,
    ) -> Result<Vec<GraphEntity>> {
        let user_id = user_id.to_string();
        let relation_type = relation_type.to_string();
        let target_name = target_name.to_string();

        // Find target entity first, then follow edges
        let mut result = self
            .db
            .query(
                "
                LET $targets = SELECT id FROM entity WHERE user_id = $user_id AND string::lowercase(name) = string::lowercase($target_name);
                SELECT out.* FROM relates_to WHERE user_id = $user_id AND relation_type = $rel_type AND `in` INSIDE $targets.id;
                ",
            )
            .bind(("user_id", user_id))
            .bind(("rel_type", relation_type))
            .bind(("target_name", target_name))
            .await
            .context("Failed to query entities by relationship")?;

        // The SELECT out.* returns the entities; take from the second statement
        Ok(result.take(1).unwrap_or_default())
    }

    // =========================================================================
    // Mention operations
    // =========================================================================

    /// Record that an entity was mentioned in a fact.
    /// Uses a deterministic record ID from (entity_id, fact_id) for dedup without read-then-write.
    ///
    /// P7b-perf: Single UPSERT with null-coalesce for idempotent first-write fields.
    pub async fn record_mention(
        &self,
        entity_id: &Thing,
        fact_id: &str,
        session_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let now = SurrealDatetime::default();

        // Deterministic ID: hash of entity_id + fact_id for natural dedup
        let mention_id = Self::mention_record_id(entity_id, fact_id);

        self.db
            .query(
                "UPSERT $id SET \
                    entity_id = $entity_id, \
                    fact_id = $fact_id, \
                    session_id = session_id ?? $session_id, \
                    user_id = $user_id, \
                    timestamp = timestamp ?? $now \
                 RETURN NONE",
            )
            .bind(("id", mention_id))
            .bind(("entity_id", entity_id.clone()))
            .bind(("fact_id", fact_id.to_string()))
            .bind(("session_id", session_id.to_string()))
            .bind(("user_id", user_id.to_string()))
            .bind(("now", now))
            .await
            .context("Failed to upsert mention")?
            .check()
            .context("Mention upsert statement failed")?;

        Ok(())
    }

    /// Generate a deterministic mention record ID from (entity_id, fact_id).
    fn mention_record_id(entity_id: &Thing, fact_id: &str) -> Thing {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("{}:{}", entity_id, fact_id));
        let hash = hex::encode(hasher.finalize());
        Thing::from(("mention", &hash[..16]))
    }

    // =========================================================================
    // P7b-perf: Batch ingestion (all per-session writes in 1 query)
    // =========================================================================

    /// Batch-ingest all graph data for a single session in one transactional query.
    ///
    /// Builds a multi-statement SurrealQL string wrapped in BEGIN/COMMIT TRANSACTION,
    /// executed as a single `.query()` call. This reduces per-session round-trips from
    /// ~138 to 1 (~140x reduction).
    ///
    /// Returns a map of (name_lower, type_lower) → Thing for entity ID lookups.
    pub async fn ingest_session_batch(
        &self,
        user_id: &str,
        session_id: &str,
        entities: &[EntityInput],
        relationships: &[RelationshipInput],
        mentions: &[MentionInput],
    ) -> Result<HashMap<(String, String), Thing>> {
        if entities.is_empty() && relationships.is_empty() && mentions.is_empty() {
            return Ok(HashMap::new());
        }

        let now = SurrealDatetime::default();
        let mut stmts: Vec<String> = Vec::new();
        let mut entity_id_map: HashMap<(String, String), Thing> = HashMap::new();

        // --- Entity UPSERTs ---
        for ent in entities {
            let record_id = Self::entity_record_id(user_id, &ent.name, &ent.entity_type);
            let key = (ent.name.to_lowercase(), ent.entity_type.to_lowercase());
            entity_id_map.insert(key, record_id.clone());

            let aliases_json = serde_json::to_string(
                &ent.aliases.iter().map(|a| a.to_lowercase()).collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_string());

            stmts.push(format!(
                "UPSERT {record_id} SET \
                    name = {name}, \
                    entity_type = {etype}, \
                    user_id = {uid}, \
                    aliases = array::union(aliases ?? [], {aliases}), \
                    first_seen = first_seen ?? {now}, \
                    last_seen = {now}, \
                    mention_count = (mention_count ?? 0) + 1 \
                 RETURN NONE",
                record_id = Self::format_thing(&record_id),
                name = Self::quote_str(&ent.name),
                etype = Self::quote_str(&ent.entity_type),
                uid = Self::quote_str(user_id),
                aliases = aliases_json,
                now = Self::format_datetime(&now),
            ));
        }

        // --- Relationship INSERT RELATIONs ---
        for rel in relationships {
            // Resolve subject and object to entity IDs
            let subj_key = (rel.subject_name.to_lowercase(), String::new());
            let obj_key = (rel.object_name.to_lowercase(), String::new());

            // Find by name (any type) since relationship inputs don't carry entity_type
            let subj_id = entity_id_map.iter()
                .find(|((n, _), _)| *n == subj_key.0)
                .map(|(_, id)| id.clone());
            let obj_id = entity_id_map.iter()
                .find(|((n, _), _)| *n == obj_key.0)
                .map(|(_, id)| id.clone());

            if let (Some(subj_id), Some(obj_id)) = (subj_id, obj_id) {
                let rel_id = Self::relationship_record_id(&subj_id, &rel.relation_type, &obj_id);

                stmts.push(format!(
                    "INSERT RELATION INTO relates_to {{ \
                        id: {rel_id}, in: {subj}, out: {obj}, \
                        relation_type: {rel_type}, user_id: {uid}, \
                        confidence: {conf}, first_seen: time::now(), last_seen: time::now() \
                     }} ON DUPLICATE KEY UPDATE \
                        confidence = math::max([confidence, $input.confidence]), \
                        last_seen = time::now() \
                     RETURN NONE",
                    rel_id = Self::format_thing(&rel_id),
                    subj = Self::format_thing(&subj_id),
                    obj = Self::format_thing(&obj_id),
                    rel_type = Self::quote_str(&rel.relation_type),
                    uid = Self::quote_str(user_id),
                    conf = rel.confidence as f64,
                ));
            }
        }

        // --- Mention UPSERTs ---
        for mention in mentions {
            let ent_key_lower = (mention.entity_name.to_lowercase(), mention.entity_type.to_lowercase());
            // Try exact (name, type) match first, fall back to name-only
            let ent_id = entity_id_map.get(&ent_key_lower)
                .or_else(|| {
                    entity_id_map.iter()
                        .find(|((n, _), _)| *n == ent_key_lower.0)
                        .map(|(_, id)| id)
                });

            if let Some(ent_id) = ent_id {
                let mention_id = Self::mention_record_id(ent_id, &mention.fact_id);

                stmts.push(format!(
                    "UPSERT {mention_id} SET \
                        entity_id = {ent_id}, \
                        fact_id = {fact_id}, \
                        session_id = session_id ?? {session_id}, \
                        user_id = {uid}, \
                        timestamp = timestamp ?? {now} \
                     RETURN NONE",
                    mention_id = Self::format_thing(&mention_id),
                    ent_id = Self::format_thing(ent_id),
                    fact_id = Self::quote_str(&mention.fact_id),
                    session_id = Self::quote_str(session_id),
                    uid = Self::quote_str(user_id),
                    now = Self::format_datetime(&now),
                ));
            }
        }

        if stmts.is_empty() {
            return Ok(entity_id_map);
        }

        // Split into batches of 500 statements to avoid oversized queries
        const BATCH_SIZE: usize = 500;
        for chunk in stmts.chunks(BATCH_SIZE) {
            let body = chunk.join(";\n");
            let query = format!("BEGIN TRANSACTION;\n{body};\nCOMMIT TRANSACTION;");

            self.db
                .query(&query)
                .await
                .with_context(|| {
                    format!(
                        "Graph batch ingestion failed for session {} ({} statements)",
                        session_id,
                        chunk.len()
                    )
                })?
                .check()
                .with_context(|| {
                    format!(
                        "Graph batch statement error for session {} ({} statements)",
                        session_id,
                        chunk.len()
                    )
                })?;
        }

        Ok(entity_id_map)
    }

    // =========================================================================
    // Formatting helpers for raw SurrealQL string building
    // =========================================================================

    /// Format a Thing as a SurrealQL record reference: `table:id`
    fn format_thing(thing: &Thing) -> String {
        format!("{}:{}", thing.tb, Self::quote_surreal_id(&thing.id.to_raw()))
    }

    /// Quote a SurrealQL identifier (record ID part) — wraps in backticks
    fn quote_surreal_id(s: &str) -> String {
        // Use backtick quoting for record IDs to handle any special chars
        format!("`{}`", s.replace('`', ""))
    }

    /// Quote a string value for SurrealQL — single quotes with escaping
    fn quote_str(s: &str) -> String {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    /// Format a SurrealDatetime as a SurrealQL datetime literal.
    /// SurrealDatetime::to_string() already produces `d'...'` format.
    fn format_datetime(dt: &SurrealDatetime) -> String {
        dt.to_string()
    }

    /// Get all sessions where an entity is mentioned
    pub async fn sessions_for_entity(
        &self,
        user_id: &str,
        entity_id: &Thing,
    ) -> Result<Vec<String>> {
        let user_id = user_id.to_string();
        let entity_id = entity_id.clone();

        let mut result = self
            .db
            .query("SELECT * FROM mention WHERE entity_id = $entity_id AND user_id = $user_id")
            .bind(("entity_id", entity_id))
            .bind(("user_id", user_id))
            .await
            .context("Failed to query sessions for entity")?;

        let rows: Vec<GraphMention> = result.take(0).unwrap_or_default();
        let mut sessions: Vec<String> = rows.into_iter().map(|r| r.session_id).collect();
        sessions.sort();
        sessions.dedup();
        Ok(sessions)
    }

    /// Find entities that mention a specific fact (reverse of facts_for_entity).
    /// Used by P20 graph augmentation to seed entity expansion from prefetch fact IDs.
    pub async fn entities_for_fact(
        &self,
        user_id: &str,
        fact_id: &str,
    ) -> Result<Vec<GraphEntity>> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM mention WHERE fact_id = $fact_id AND user_id = $user_id",
            )
            .bind(("fact_id", fact_id.to_string()))
            .bind(("user_id", user_id.to_string()))
            .await
            .context("Failed to query entities for fact")?;

        let rows: Vec<GraphMention> = result.take(0).unwrap_or_default();
        let entity_ids: Vec<Thing> = rows.into_iter().map(|r| r.entity_id).collect();

        let mut entities = Vec::new();
        for eid in entity_ids {
            let mut ent_result = self
                .db
                .query("SELECT * FROM entity WHERE id = $id")
                .bind(("id", eid))
                .await
                .context("Failed to fetch entity by id")?;
            let found: Vec<GraphEntity> = ent_result.take(0).unwrap_or_default();
            entities.extend(found);
        }
        Ok(entities)
    }

    /// Get all fact IDs where an entity is mentioned
    pub async fn facts_for_entity(
        &self,
        user_id: &str,
        entity_id: &Thing,
    ) -> Result<Vec<String>> {
        let user_id = user_id.to_string();
        let entity_id = entity_id.clone();

        let mut result = self
            .db
            .query("SELECT * FROM mention WHERE entity_id = $entity_id AND user_id = $user_id ORDER BY timestamp DESC")
            .bind(("entity_id", entity_id))
            .bind(("user_id", user_id))
            .await
            .context("Failed to query facts for entity")?;

        let rows: Vec<GraphMention> = result.take(0).unwrap_or_default();
        Ok(rows.into_iter().map(|r| r.fact_id).collect())
    }

    // =========================================================================
    // Graph traversal
    // =========================================================================

    /// Get entities within N hops of a starting entity (BFS)
    pub async fn neighbors(
        &self,
        user_id: &str,
        entity_id: &Thing,
        hops: usize,
    ) -> Result<Vec<GraphEntity>> {
        if hops == 0 {
            return Ok(vec![]);
        }

        let user_id_owned = user_id.to_string();
        let mut visited: HashMap<String, GraphEntity> = HashMap::new();
        let mut frontier: Vec<Thing> = vec![entity_id.clone()];
        let start_key = entity_id.to_string();

        for _hop in 0..hops {
            if frontier.is_empty() {
                break;
            }

            let mut next_frontier = Vec::new();

            for node_id in &frontier {
                let rels = self
                    .get_relationships_for(&user_id_owned, node_id)
                    .await?;
                for (_, neighbor, _) in rels {
                    if let Some(ref nid) = neighbor.id {
                        let key = nid.to_string();
                        if key != start_key && !visited.contains_key(&key) {
                            next_frontier.push(nid.clone());
                            visited.insert(key, neighbor);
                        }
                    }
                }
            }

            frontier = next_frontier;
        }

        Ok(visited.into_values().collect())
    }

    /// Disambiguate an entity name: find all entities with that name and score them
    /// based on relationship overlap with context entities
    pub async fn disambiguate(
        &self,
        user_id: &str,
        name: &str,
        context_terms: &[String],
    ) -> Result<Vec<DisambiguationCandidate>> {
        let candidates = self.get_entities_by_name(user_id, name).await?;

        if candidates.len() <= 1 {
            return Ok(candidates
                .into_iter()
                .map(|e| DisambiguationCandidate {
                    entity: e,
                    relationships: vec![],
                    score: 1.0,
                })
                .collect());
        }

        let context_lower: Vec<String> = context_terms.iter().map(|s| s.to_lowercase()).collect();
        let mut scored = Vec::new();

        for candidate in candidates {
            let id = match &candidate.id {
                Some(id) => id,
                None => continue,
            };

            let rels = self.get_relationships_for(user_id, id).await?;
            let rel_pairs: Vec<(String, String)> = rels
                .iter()
                .map(|(r, e, _)| (r.relation_type.clone(), e.name.clone()))
                .collect();

            let mut score = 0.0f32;
            for ctx in &context_lower {
                for (rel_type, target_name) in &rel_pairs {
                    if target_name.to_lowercase().contains(ctx) {
                        score += 2.0;
                    }
                    if rel_type.contains(ctx) {
                        score += 1.0;
                    }
                }
                for alias in &candidate.aliases {
                    if alias.to_lowercase().contains(ctx) {
                        score += 1.5;
                    }
                }
            }

            // Boost by mention count
            score += (candidate.mention_count as f32).ln().max(0.0) * 0.5;

            scored.push(DisambiguationCandidate {
                entity: candidate,
                relationships: rel_pairs,
                score,
            });
        }

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored)
    }

    // =========================================================================
    // Stats & cleanup
    // =========================================================================

    /// Get entity profile: entity + relationships + mention stats
    pub async fn entity_profile(
        &self,
        user_id: &str,
        entity_id: &Thing,
    ) -> Result<Option<EntityProfile>> {
        let entity = match self.get_entity(entity_id).await? {
            Some(e) => e,
            None => return Ok(None),
        };

        let relationships = self.get_relationships_for(user_id, entity_id).await?;
        let sessions = self.sessions_for_entity(user_id, entity_id).await?;

        Ok(Some(EntityProfile {
            mention_count: entity.mention_count,
            entity,
            relationships,
            session_count: sessions.len(),
        }))
    }

    /// Get graph statistics for a user
    pub async fn stats(&self, user_id: &str) -> Result<GraphStats> {
        let user_id = user_id.to_string();

        let mut result = self
            .db
            .query(
                "
                SELECT count() AS c FROM entity WHERE user_id = $user_id GROUP ALL;
                SELECT count() AS c FROM relates_to WHERE user_id = $user_id GROUP ALL;
                SELECT count() AS c FROM mention WHERE user_id = $user_id GROUP ALL;
                ",
            )
            .bind(("user_id", user_id))
            .await
            .context("Failed to query graph stats")?;

        #[derive(Deserialize)]
        struct CountRow {
            c: i64,
        }

        let entities: Vec<CountRow> = result.take(0).unwrap_or_default();
        let relationships: Vec<CountRow> = result.take(1).unwrap_or_default();
        let mentions: Vec<CountRow> = result.take(2).unwrap_or_default();

        Ok(GraphStats {
            entity_count: entities.first().map(|r| r.c as usize).unwrap_or(0),
            relationship_count: relationships.first().map(|r| r.c as usize).unwrap_or(0),
            mention_count: mentions.first().map(|r| r.c as usize).unwrap_or(0),
        })
    }

    /// Get global graph statistics (all users)
    pub async fn stats_all(&self) -> Result<GraphStats> {
        let mut result = self
            .db
            .query(
                "
                SELECT count() AS c FROM entity GROUP ALL;
                SELECT count() AS c FROM relates_to GROUP ALL;
                SELECT count() AS c FROM mention GROUP ALL;
                ",
            )
            .await
            .context("Failed to query global graph stats")?;

        #[derive(Deserialize)]
        struct CountRow {
            c: i64,
        }

        let entities: Vec<CountRow> = result.take(0).unwrap_or_default();
        let relationships: Vec<CountRow> = result.take(1).unwrap_or_default();
        let mentions: Vec<CountRow> = result.take(2).unwrap_or_default();

        Ok(GraphStats {
            entity_count: entities.first().map(|r| r.c as usize).unwrap_or(0),
            relationship_count: relationships.first().map(|r| r.c as usize).unwrap_or(0),
            mention_count: mentions.first().map(|r| r.c as usize).unwrap_or(0),
        })
    }

    /// Clear all data for a user
    pub async fn clear_user(&self, user_id: &str) -> Result<()> {
        let user_id = user_id.to_string();

        self.db
            .query(
                "
                DELETE FROM mention WHERE user_id = $user_id;
                DELETE FROM relates_to WHERE user_id = $user_id;
                DELETE FROM entity WHERE user_id = $user_id;
                ",
            )
            .bind(("user_id", user_id))
            .await
            .context("Failed to clear user data")?;

        Ok(())
    }

    /// Clear all data (for benchmark re-ingestion)
    pub async fn clear_all(&self) -> Result<()> {
        self.db
            .query(
                "
                DELETE FROM mention;
                DELETE FROM relates_to;
                DELETE FROM entity;
                ",
            )
            .await
            .context("Failed to clear all data")?;

        Ok(())
    }
}

/// Graph statistics
#[derive(Debug, Clone)]
pub struct GraphStats {
    pub entity_count: usize,
    pub relationship_count: usize,
    pub mention_count: usize,
}

impl std::fmt::Display for GraphStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} entities, {} relationships, {} mentions",
            self.entity_count, self.relationship_count, self.mention_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> GraphStore {
        GraphStore::new_memory()
            .await
            .expect("Failed to create test store")
    }

    #[tokio::test]
    async fn test_create_and_get_entity() {
        let store = test_store().await;

        let id = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();

        let entity = store.get_entity(&id).await.unwrap().unwrap();
        assert_eq!(entity.name, "Alice");
        assert_eq!(entity.entity_type, "person");
        assert_eq!(entity.user_id, "user1");
        assert_eq!(entity.mention_count, 1);
    }

    #[tokio::test]
    async fn test_upsert_entity_increments_count() {
        let store = test_store().await;

        let id1 = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();
        let id2 = store
            .upsert_entity("user1", "Alice", "person", "session2", &[])
            .await
            .unwrap();

        assert_eq!(id1, id2);

        let entity = store.get_entity(&id1).await.unwrap().unwrap();
        assert_eq!(entity.mention_count, 2);
    }

    #[tokio::test]
    async fn test_same_name_different_type_creates_separate_entities() {
        let store = test_store().await;

        let id1 = store
            .upsert_entity("user1", "Paris", "person", "session1", &[])
            .await
            .unwrap();
        let id2 = store
            .upsert_entity("user1", "Paris", "location", "session1", &[])
            .await
            .unwrap();

        assert_ne!(id1, id2);

        let entities = store.get_entities_by_name("user1", "Paris").await.unwrap();
        assert_eq!(entities.len(), 2);
    }

    #[tokio::test]
    async fn test_user_isolation() {
        let store = test_store().await;

        store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();
        store
            .upsert_entity("user2", "Alice", "person", "session1", &[])
            .await
            .unwrap();

        let user1_entities = store.get_entities_by_name("user1", "Alice").await.unwrap();
        let user2_entities = store.get_entities_by_name("user2", "Alice").await.unwrap();

        assert_eq!(user1_entities.len(), 1);
        assert_eq!(user2_entities.len(), 1);
        assert_ne!(
            user1_entities[0].id.as_ref().unwrap(),
            user2_entities[0].id.as_ref().unwrap()
        );
    }

    #[tokio::test]
    async fn test_create_and_query_relationships() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();
        let google_id = store
            .upsert_entity("user1", "Google", "organization", "session1", &[])
            .await
            .unwrap();

        store
            .upsert_relationship("user1", &alice_id, "works_for", &google_id, 0.95)
            .await
            .unwrap();

        let rels = store
            .get_relationships_for("user1", &alice_id)
            .await
            .unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].0.relation_type, "works_for");
        assert_eq!(rels[0].1.name, "Google");
    }

    #[tokio::test]
    async fn test_upsert_relationship_updates_confidence() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();
        let google_id = store
            .upsert_entity("user1", "Google", "organization", "session1", &[])
            .await
            .unwrap();

        store
            .upsert_relationship("user1", &alice_id, "works_for", &google_id, 0.5)
            .await
            .unwrap();
        store
            .upsert_relationship("user1", &alice_id, "works_for", &google_id, 0.95)
            .await
            .unwrap();

        let rels = store
            .get_relationships_for("user1", &alice_id)
            .await
            .unwrap();
        assert_eq!(rels.len(), 1);
        assert!(rels[0].0.confidence >= 0.94);
    }

    #[tokio::test]
    async fn test_record_and_query_mentions() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();

        store
            .record_mention(&alice_id, "fact-001", "session1", "user1")
            .await
            .unwrap();
        store
            .record_mention(&alice_id, "fact-002", "session2", "user1")
            .await
            .unwrap();

        let sessions = store.sessions_for_entity("user1", &alice_id).await.unwrap();
        assert_eq!(sessions.len(), 2);

        let facts = store.facts_for_entity("user1", &alice_id).await.unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[tokio::test]
    async fn test_mention_deduplication() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "session1", &[])
            .await
            .unwrap();

        store
            .record_mention(&alice_id, "fact-001", "session1", "user1")
            .await
            .unwrap();
        store
            .record_mention(&alice_id, "fact-001", "session1", "user1")
            .await
            .unwrap();

        let facts = store.facts_for_entity("user1", &alice_id).await.unwrap();
        assert_eq!(facts.len(), 1);
    }

    #[tokio::test]
    async fn test_neighbors() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "s1", &[])
            .await
            .unwrap();
        let google_id = store
            .upsert_entity("user1", "Google", "organization", "s1", &[])
            .await
            .unwrap();
        let nyc_id = store
            .upsert_entity("user1", "New York", "location", "s1", &[])
            .await
            .unwrap();

        store
            .upsert_relationship("user1", &alice_id, "works_for", &google_id, 0.9)
            .await
            .unwrap();
        store
            .upsert_relationship("user1", &google_id, "located_in", &nyc_id, 0.9)
            .await
            .unwrap();

        let hop1 = store.neighbors("user1", &alice_id, 1).await.unwrap();
        assert_eq!(hop1.len(), 1);
        assert_eq!(hop1[0].name, "Google");

        let hop2 = store.neighbors("user1", &alice_id, 2).await.unwrap();
        assert_eq!(hop2.len(), 2);
    }

    #[tokio::test]
    async fn test_disambiguate() {
        let store = test_store().await;

        let rachel_id = store
            .upsert_entity("user1", "Rachel", "person", "s1", &[])
            .await
            .unwrap();
        let acme_id = store
            .upsert_entity("user1", "Acme Corp", "organization", "s1", &[])
            .await
            .unwrap();

        store
            .upsert_relationship("user1", &rachel_id, "works_for", &acme_id, 0.9)
            .await
            .unwrap();

        let candidates = store
            .disambiguate("user1", "Rachel", &["Acme Corp".to_string()])
            .await
            .unwrap();

        assert!(!candidates.is_empty());
        assert!(candidates[0].score > 0.0);
    }

    #[tokio::test]
    async fn test_stats() {
        let store = test_store().await;

        let alice_id = store
            .upsert_entity("user1", "Alice", "person", "s1", &[])
            .await
            .unwrap();
        let google_id = store
            .upsert_entity("user1", "Google", "organization", "s1", &[])
            .await
            .unwrap();

        store
            .upsert_relationship("user1", &alice_id, "works_for", &google_id, 0.9)
            .await
            .unwrap();
        store
            .record_mention(&alice_id, "f1", "s1", "user1")
            .await
            .unwrap();

        let stats = store.stats("user1").await.unwrap();
        assert_eq!(stats.entity_count, 2);
        assert_eq!(stats.relationship_count, 1);
        assert_eq!(stats.mention_count, 1);
    }

    #[tokio::test]
    async fn test_clear_user() {
        let store = test_store().await;

        let id = store
            .upsert_entity("user1", "Alice", "person", "s1", &[])
            .await
            .unwrap();
        store
            .record_mention(&id, "f1", "s1", "user1")
            .await
            .unwrap();

        store.clear_user("user1").await.unwrap();

        let stats = store.stats("user1").await.unwrap();
        assert_eq!(stats.entity_count, 0);
        assert_eq!(stats.mention_count, 0);
    }

    #[tokio::test]
    async fn test_aliases_merge_on_upsert() {
        let store = test_store().await;

        store
            .upsert_entity(
                "user1",
                "Alice",
                "person",
                "s1",
                &["Dr. Alice".to_string()],
            )
            .await
            .unwrap();
        store
            .upsert_entity(
                "user1",
                "Alice",
                "person",
                "s2",
                &["Alice Smith".to_string(), "Dr. Alice".to_string()],
            )
            .await
            .unwrap();

        let entities = store.get_entities_by_name("user1", "Alice").await.unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].aliases.len(), 2); // Dr. Alice + Alice Smith (no dup)
    }

    #[tokio::test]
    async fn test_ingest_session_batch() {
        let store = test_store().await;

        let entities = vec![
            EntityInput {
                name: "Alice".to_string(),
                entity_type: "person".to_string(),
                aliases: vec!["Dr. Alice".to_string()],
            },
            EntityInput {
                name: "Google".to_string(),
                entity_type: "organization".to_string(),
                aliases: vec![],
            },
            EntityInput {
                name: "New York".to_string(),
                entity_type: "location".to_string(),
                aliases: vec!["NYC".to_string()],
            },
        ];

        let relationships = vec![
            RelationshipInput {
                subject_name: "Alice".to_string(),
                relation_type: "works_for".to_string(),
                object_name: "Google".to_string(),
                confidence: 0.9,
            },
            RelationshipInput {
                subject_name: "Alice".to_string(),
                relation_type: "lives_in".to_string(),
                object_name: "New York".to_string(),
                confidence: 0.8,
            },
        ];

        let mentions = vec![
            MentionInput {
                entity_name: "Alice".to_string(),
                entity_type: "person".to_string(),
                fact_id: "fact-001".to_string(),
            },
            MentionInput {
                entity_name: "Google".to_string(),
                entity_type: "organization".to_string(),
                fact_id: "fact-001".to_string(),
            },
            MentionInput {
                entity_name: "Alice".to_string(),
                entity_type: "person".to_string(),
                fact_id: "fact-002".to_string(),
            },
            MentionInput {
                entity_name: "New York".to_string(),
                entity_type: "location".to_string(),
                fact_id: "fact-002".to_string(),
            },
        ];

        let id_map = store
            .ingest_session_batch("user1", "session1", &entities, &relationships, &mentions)
            .await
            .unwrap();

        // Verify entities created
        assert_eq!(id_map.len(), 3);
        let stats = store.stats("user1").await.unwrap();
        assert_eq!(stats.entity_count, 3);
        assert_eq!(stats.relationship_count, 2);
        assert_eq!(stats.mention_count, 4);

        // Verify entity data
        let alice_id = id_map.get(&("alice".to_string(), "person".to_string())).unwrap();
        let alice = store.get_entity(alice_id).await.unwrap().unwrap();
        assert_eq!(alice.name, "Alice");
        assert_eq!(alice.mention_count, 1);
        assert!(alice.aliases.contains(&"dr. alice".to_string()));

        // Verify relationships
        let rels = store.get_relationships_for("user1", alice_id).await.unwrap();
        assert_eq!(rels.len(), 2);

        // Verify mentions
        let facts = store.facts_for_entity("user1", alice_id).await.unwrap();
        assert_eq!(facts.len(), 2);

        // Verify idempotency — batch again, counts should increment
        let _id_map2 = store
            .ingest_session_batch("user1", "session2", &entities, &relationships, &mentions)
            .await
            .unwrap();
        let alice2 = store.get_entity(alice_id).await.unwrap().unwrap();
        assert_eq!(alice2.mention_count, 2);
        // Mentions should still be 4 (same fact_ids = dedup)
        let stats2 = store.stats("user1").await.unwrap();
        assert_eq!(stats2.mention_count, 4);
    }
}
