//! Benchmark result storage
//!
//! SQLite-based storage for benchmark results and comparisons.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use super::types::{BenchmarkResult, CategoryScore, QuestionCategory};
use crate::error::BenchmarkError;

/// Result type for storage operations
type Result<T> = crate::error::Result<T>;

/// Benchmark result storage
pub struct ResultStorage {
    conn: Connection,
}

impl ResultStorage {
    /// Create a new result storage
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path).map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Create an in-memory storage (for testing)
    pub fn in_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS benchmark_runs (
                run_id TEXT PRIMARY KEY,
                benchmark_name TEXT NOT NULL,
                extraction_mode TEXT NOT NULL,
                answer_model TEXT NOT NULL,
                judge_model TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT NOT NULL,
                total_questions INTEGER NOT NULL,
                correct_count INTEGER NOT NULL,
                accuracy REAL NOT NULL,
                total_time_seconds REAL NOT NULL,
                estimated_cost_usd REAL NOT NULL,
                config_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS question_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                question_id TEXT NOT NULL,
                category TEXT NOT NULL,
                question TEXT NOT NULL,
                expected_answer TEXT NOT NULL,
                generated_answer TEXT NOT NULL,
                is_correct INTEGER NOT NULL,
                judge_score REAL NOT NULL,
                judge_reasoning TEXT NOT NULL,
                retrieval_time_ms INTEGER NOT NULL,
                answer_time_ms INTEGER NOT NULL,
                total_time_ms INTEGER NOT NULL,
                FOREIGN KEY (run_id) REFERENCES benchmark_runs(run_id)
            );

            CREATE TABLE IF NOT EXISTS category_scores (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                category TEXT NOT NULL,
                total INTEGER NOT NULL,
                correct INTEGER NOT NULL,
                accuracy REAL NOT NULL,
                FOREIGN KEY (run_id) REFERENCES benchmark_runs(run_id)
            );

            CREATE INDEX IF NOT EXISTS idx_question_results_run
                ON question_results(run_id);

            CREATE INDEX IF NOT EXISTS idx_category_scores_run
                ON category_scores(run_id);

            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_name
                ON benchmark_runs(benchmark_name);

            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_started
                ON benchmark_runs(started_at DESC);
            ",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Save a benchmark result
    pub fn save_result(&self, result: &BenchmarkResult) -> Result<()> {
        let config_json = serde_json::to_string(&result.config)
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        self.conn
            .execute(
                "INSERT INTO benchmark_runs
             (run_id, benchmark_name, extraction_mode, answer_model, judge_model,
              started_at, completed_at, total_questions, correct_count, accuracy,
              total_time_seconds, estimated_cost_usd, config_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    result.run_id.to_string(),
                    result.benchmark_name,
                    result.config.extraction_mode,
                    result.config.answer_model,
                    result.config.judge_model,
                    result.started_at.to_rfc3339(),
                    result.completed_at.to_rfc3339(),
                    result.total_questions as i64,
                    result.correct_count as i64,
                    result.accuracy as f64,
                    result.total_time_seconds as f64,
                    result.estimated_cost_usd as f64,
                    config_json,
                ],
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        // Save question results
        for qr in &result.question_results {
            self.conn
                .execute(
                    "INSERT INTO question_results
                 (run_id, question_id, category, question, expected_answer,
                  generated_answer, is_correct, judge_score, judge_reasoning,
                  retrieval_time_ms, answer_time_ms, total_time_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        result.run_id.to_string(),
                        qr.question_id,
                        qr.category.as_str(),
                        qr.question,
                        qr.expected_answer,
                        qr.generated_answer,
                        qr.is_correct as i32,
                        qr.judge_score as f64,
                        qr.judge_reasoning,
                        qr.retrieval_time_ms as i64,
                        qr.answer_time_ms as i64,
                        qr.total_time_ms as i64,
                    ],
                )
                .map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        }

        // Save category scores
        for cs in &result.category_scores {
            self.conn
                .execute(
                    "INSERT INTO category_scores
                 (run_id, category, total, correct, accuracy)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        result.run_id.to_string(),
                        cs.category.as_str(),
                        cs.total as i64,
                        cs.correct as i64,
                        cs.accuracy as f64,
                    ],
                )
                .map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        }

        Ok(())
    }

    /// Get recent benchmark runs
    pub fn get_recent_runs(&self, limit: usize) -> Result<Vec<BenchmarkRunSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, benchmark_name, extraction_mode, started_at,
                    total_questions, accuracy, total_time_seconds
             FROM benchmark_runs
             ORDER BY started_at DESC
             LIMIT ?1",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let run_id_str: String = row.get(0)?;
                let started_at_str: String = row.get(3)?;

                Ok(BenchmarkRunSummary {
                    run_id: run_id_str.parse().unwrap_or_else(|_| Uuid::now_v7()),
                    benchmark_name: row.get(1)?,
                    extraction_mode: row.get(2)?,
                    started_at: DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    total_questions: row.get::<_, i64>(4)? as usize,
                    accuracy: row.get::<_, f64>(5)? as f32,
                    total_time_seconds: row.get::<_, f64>(6)? as f32,
                })
            })
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get a run by ID
    pub fn get_run(&self, run_id: Uuid) -> Result<Option<BenchmarkRunSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, benchmark_name, extraction_mode, started_at,
                    total_questions, accuracy, total_time_seconds
             FROM benchmark_runs
             WHERE run_id = ?1",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        let result = stmt.query_row(params![run_id.to_string()], |row| {
            let run_id_str: String = row.get(0)?;
            let started_at_str: String = row.get(3)?;

            Ok(BenchmarkRunSummary {
                run_id: run_id_str.parse().unwrap_or_else(|_| Uuid::now_v7()),
                benchmark_name: row.get(1)?,
                extraction_mode: row.get(2)?,
                started_at: DateTime::parse_from_rfc3339(&started_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                total_questions: row.get::<_, i64>(4)? as usize,
                accuracy: row.get::<_, f64>(5)? as f32,
                total_time_seconds: row.get::<_, f64>(6)? as f32,
            })
        });

        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(BenchmarkError::Storage(e.to_string()).into()),
        }
    }

    /// Get runs by benchmark name
    pub fn get_runs_by_name(&self, name: &str, limit: usize) -> Result<Vec<BenchmarkRunSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, benchmark_name, extraction_mode, started_at,
                    total_questions, accuracy, total_time_seconds
             FROM benchmark_runs
             WHERE benchmark_name = ?1
             ORDER BY started_at DESC
             LIMIT ?2",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![name, limit as i64], |row| {
                let run_id_str: String = row.get(0)?;
                let started_at_str: String = row.get(3)?;

                Ok(BenchmarkRunSummary {
                    run_id: run_id_str.parse().unwrap_or_else(|_| Uuid::now_v7()),
                    benchmark_name: row.get(1)?,
                    extraction_mode: row.get(2)?,
                    started_at: DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    total_questions: row.get::<_, i64>(4)? as usize,
                    accuracy: row.get::<_, f64>(5)? as f32,
                    total_time_seconds: row.get::<_, f64>(6)? as f32,
                })
            })
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get category scores for a run
    pub fn get_category_scores(&self, run_id: Uuid) -> Result<Vec<CategoryScore>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT category, total, correct, accuracy
             FROM category_scores
             WHERE run_id = ?1",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![run_id.to_string()], |row| {
                let category_str: String = row.get(0)?;
                Ok(CategoryScore {
                    category: category_str.parse().unwrap_or(QuestionCategory::Extraction),
                    total: row.get::<_, i64>(1)? as usize,
                    correct: row.get::<_, i64>(2)? as usize,
                    accuracy: row.get::<_, f64>(3)? as f32,
                })
            })
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Compare two benchmark runs
    pub fn compare_runs(&self, run1_id: Uuid, run2_id: Uuid) -> Result<Option<RunComparison>> {
        let run1 = match self.get_run(run1_id)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let run2 = match self.get_run(run2_id)? {
            Some(r) => r,
            None => return Ok(None),
        };

        let scores1 = self.get_category_scores(run1_id)?;
        let scores2 = self.get_category_scores(run2_id)?;

        let mut category_diffs = Vec::new();
        for cat in QuestionCategory::all() {
            let acc1 = scores1
                .iter()
                .find(|s| s.category == cat)
                .map(|s| s.accuracy)
                .unwrap_or(0.0);
            let acc2 = scores2
                .iter()
                .find(|s| s.category == cat)
                .map(|s| s.accuracy)
                .unwrap_or(0.0);

            category_diffs.push(CategoryDiff {
                category: cat,
                accuracy1: acc1,
                accuracy2: acc2,
                diff: acc2 - acc1,
            });
        }

        let accuracy_diff = run2.accuracy - run1.accuracy;
        Ok(Some(RunComparison {
            run1,
            run2,
            accuracy_diff,
            category_diffs,
        }))
    }

    /// Get the best run for a benchmark
    pub fn get_best_run(&self, benchmark_name: &str) -> Result<Option<BenchmarkRunSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT run_id, benchmark_name, extraction_mode, started_at,
                    total_questions, accuracy, total_time_seconds
             FROM benchmark_runs
             WHERE benchmark_name = ?1
             ORDER BY accuracy DESC
             LIMIT 1",
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        let result = stmt.query_row(params![benchmark_name], |row| {
            let run_id_str: String = row.get(0)?;
            let started_at_str: String = row.get(3)?;

            Ok(BenchmarkRunSummary {
                run_id: run_id_str.parse().unwrap_or_else(|_| Uuid::now_v7()),
                benchmark_name: row.get(1)?,
                extraction_mode: row.get(2)?,
                started_at: DateTime::parse_from_rfc3339(&started_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                total_questions: row.get::<_, i64>(4)? as usize,
                accuracy: row.get::<_, f64>(5)? as f32,
                total_time_seconds: row.get::<_, f64>(6)? as f32,
            })
        });

        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(BenchmarkError::Storage(e.to_string()).into()),
        }
    }

    /// Delete a run and its associated data
    pub fn delete_run(&self, run_id: Uuid) -> Result<bool> {
        let run_id_str = run_id.to_string();

        // Delete in order due to foreign keys
        self.conn
            .execute(
                "DELETE FROM question_results WHERE run_id = ?1",
                params![run_id_str],
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        self.conn
            .execute(
                "DELETE FROM category_scores WHERE run_id = ?1",
                params![run_id_str],
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        let deleted = self
            .conn
            .execute(
                "DELETE FROM benchmark_runs WHERE run_id = ?1",
                params![run_id_str],
            )
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;

        Ok(deleted > 0)
    }

    /// Count total runs
    pub fn count_runs(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM benchmark_runs", [], |row| row.get(0))
            .map_err(|e| BenchmarkError::Storage(e.to_string()))?;
        Ok(count as usize)
    }
}

/// Summary of a benchmark run
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkRunSummary {
    /// Unique run ID
    pub run_id: Uuid,
    /// Benchmark name
    pub benchmark_name: String,
    /// Extraction mode used
    pub extraction_mode: String,
    /// When the run started
    pub started_at: DateTime<Utc>,
    /// Total questions evaluated
    pub total_questions: usize,
    /// Overall accuracy
    pub accuracy: f32,
    /// Total time in seconds
    pub total_time_seconds: f32,
}

/// Comparison between two benchmark runs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunComparison {
    /// First run
    pub run1: BenchmarkRunSummary,
    /// Second run
    pub run2: BenchmarkRunSummary,
    /// Overall accuracy difference (run2 - run1)
    pub accuracy_diff: f32,
    /// Per-category differences
    pub category_diffs: Vec<CategoryDiff>,
}

impl RunComparison {
    /// Check if run2 is better than run1
    pub fn is_improvement(&self) -> bool {
        self.accuracy_diff > 0.0
    }

    /// Get the improvement percentage
    pub fn improvement_percent(&self) -> f32 {
        if self.run1.accuracy > 0.0 {
            (self.accuracy_diff / self.run1.accuracy) * 100.0
        } else {
            0.0
        }
    }
}

/// Per-category difference between runs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CategoryDiff {
    /// Category
    pub category: QuestionCategory,
    /// Accuracy in run 1
    pub accuracy1: f32,
    /// Accuracy in run 2
    pub accuracy2: f32,
    /// Difference (run2 - run1)
    pub diff: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BenchmarkConfig, BenchmarkResult, QuestionResult};

    fn create_test_result() -> BenchmarkResult {
        let config = BenchmarkConfig::default();
        let mut result = BenchmarkResult::new("test", config);

        result.add_result(QuestionResult {
            question_id: "q1".to_string(),
            category: QuestionCategory::Extraction,
            question: "Test?".to_string(),
            expected_answer: "Yes".to_string(),
            generated_answer: "Yes".to_string(),
            retrieved_memories: vec![],
            is_correct: true,
            judge_score: 1.0,
            judge_reasoning: "Correct".to_string(),
            retrieval_time_ms: 50,
            answer_time_ms: 100,
            total_time_ms: 150,
        });

        result.calculate_scores()
    }

    #[test]
    fn test_result_storage_creation() {
        let storage = ResultStorage::in_memory().unwrap();
        assert_eq!(storage.count_runs().unwrap(), 0);
    }

    #[test]
    fn test_save_and_retrieve() {
        let storage = ResultStorage::in_memory().unwrap();
        let result = create_test_result();
        let run_id = result.run_id;

        storage.save_result(&result).unwrap();

        let retrieved = storage.get_run(run_id).unwrap();
        assert!(retrieved.is_some());

        let run = retrieved.unwrap();
        assert_eq!(run.run_id, run_id);
        assert_eq!(run.benchmark_name, "test");
    }

    #[test]
    fn test_get_recent_runs() {
        let storage = ResultStorage::in_memory().unwrap();

        // Save multiple results
        for i in 0..5 {
            let config = BenchmarkConfig::new(format!("test-{}", i));
            let result = BenchmarkResult::new(format!("test-{}", i), config).calculate_scores();
            storage.save_result(&result).unwrap();
        }

        let recent = storage.get_recent_runs(3).unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_get_runs_by_name() {
        let storage = ResultStorage::in_memory().unwrap();

        // Save results with different names
        let result1 =
            BenchmarkResult::new("longmemeval", BenchmarkConfig::default()).calculate_scores();
        let result2 = BenchmarkResult::new("locomo", BenchmarkConfig::default()).calculate_scores();
        let result3 =
            BenchmarkResult::new("longmemeval", BenchmarkConfig::default()).calculate_scores();

        storage.save_result(&result1).unwrap();
        storage.save_result(&result2).unwrap();
        storage.save_result(&result3).unwrap();

        let longmemeval_runs = storage.get_runs_by_name("longmemeval", 10).unwrap();
        assert_eq!(longmemeval_runs.len(), 2);

        let locomo_runs = storage.get_runs_by_name("locomo", 10).unwrap();
        assert_eq!(locomo_runs.len(), 1);
    }

    #[test]
    fn test_get_category_scores() {
        let storage = ResultStorage::in_memory().unwrap();
        let result = create_test_result();
        let run_id = result.run_id;

        storage.save_result(&result).unwrap();

        let scores = storage.get_category_scores(run_id).unwrap();
        assert!(!scores.is_empty());
    }

    #[test]
    fn test_compare_runs() {
        let storage = ResultStorage::in_memory().unwrap();

        let mut result1 = create_test_result();
        result1.accuracy = 0.8;
        let run1_id = result1.run_id;

        let mut result2 = create_test_result();
        result2.accuracy = 0.9;
        let run2_id = result2.run_id;

        storage.save_result(&result1).unwrap();
        storage.save_result(&result2).unwrap();

        let comparison = storage.compare_runs(run1_id, run2_id).unwrap().unwrap();
        assert!(comparison.is_improvement());
        assert!((comparison.accuracy_diff - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_get_best_run() {
        let storage = ResultStorage::in_memory().unwrap();

        for accuracy in [0.7, 0.9, 0.8] {
            let config = BenchmarkConfig::new("test");
            let mut result = BenchmarkResult::new("test", config).calculate_scores();
            result.accuracy = accuracy;
            storage.save_result(&result).unwrap();
        }

        let best = storage.get_best_run("test").unwrap().unwrap();
        assert!((best.accuracy - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_delete_run() {
        let storage = ResultStorage::in_memory().unwrap();
        let result = create_test_result();
        let run_id = result.run_id;

        storage.save_result(&result).unwrap();
        assert_eq!(storage.count_runs().unwrap(), 1);

        let deleted = storage.delete_run(run_id).unwrap();
        assert!(deleted);
        assert_eq!(storage.count_runs().unwrap(), 0);
    }

    #[test]
    fn test_run_comparison_methods() {
        let run1 = BenchmarkRunSummary {
            run_id: Uuid::now_v7(),
            benchmark_name: "test".to_string(),
            extraction_mode: "local".to_string(),
            started_at: Utc::now(),
            total_questions: 100,
            accuracy: 0.8,
            total_time_seconds: 60.0,
        };

        let run2 = BenchmarkRunSummary {
            run_id: Uuid::now_v7(),
            benchmark_name: "test".to_string(),
            extraction_mode: "api".to_string(),
            started_at: Utc::now(),
            total_questions: 100,
            accuracy: 0.9,
            total_time_seconds: 120.0,
        };

        let comparison = RunComparison {
            run1,
            run2,
            accuracy_diff: 0.1,
            category_diffs: vec![],
        };

        assert!(comparison.is_improvement());
        assert!((comparison.improvement_percent() - 12.5).abs() < 0.1);
    }
}
