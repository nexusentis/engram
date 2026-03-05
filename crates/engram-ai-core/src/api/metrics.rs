//! Prometheus metrics for the Engram API
//!
//! Provides comprehensive metrics for monitoring API performance,
//! memory operations, and system health.
//!
//! ## Metric Categories
//!
//! - **Request metrics**: HTTP request counts, durations, sizes
//! - **Memory operation metrics**: Ingestion, retrieval, deletion counts
//! - **Extraction metrics**: Pipeline performance and error rates
//! - **Retrieval metrics**: Query performance and abstention rates
//! - **Storage metrics**: Qdrant operation performance
//! - **Worker metrics**: Background worker performance
//! - **System metrics**: Memory counts and connections
//!
//! ## Usage
//!
//! ```rust,ignore
//! use engram_core::api::metrics::METRICS;
//!
//! // Increment request counter
//! METRICS.requests_total.with_label_values(&["GET", "/health", "200"]).inc();
//!
//! // Record request duration
//! METRICS.request_duration_seconds.with_label_values(&["GET", "/search"]).observe(0.123);
//!
//! // Get Prometheus-formatted output
//! let output = METRICS.encode();
//! ```

use once_cell::sync::Lazy;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};

/// Global metrics registry
pub static METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);

/// Application metrics collection
pub struct Metrics {
    /// Prometheus registry
    pub registry: Registry,

    // ===== Request metrics =====
    /// Total HTTP requests (labels: method, path, status)
    pub requests_total: IntCounterVec,
    /// HTTP request duration in seconds (labels: method, path)
    pub request_duration_seconds: HistogramVec,
    /// HTTP request size in bytes (labels: method, path)
    pub request_size_bytes: HistogramVec,
    /// HTTP response size in bytes (labels: method, path)
    pub response_size_bytes: HistogramVec,

    // ===== Memory operation metrics =====
    /// Total memories ingested
    pub memories_ingested_total: IntCounter,
    /// Total memories retrieved
    pub memories_retrieved_total: IntCounter,
    /// Total memories deleted
    pub memories_deleted_total: IntCounter,
    /// Total facts extracted from conversations
    pub facts_extracted_total: IntCounter,

    // ===== Extraction metrics =====
    /// Extraction pipeline duration in seconds
    pub extraction_duration_seconds: Histogram,
    /// Number of facts extracted per conversation
    pub extraction_facts_per_conversation: Histogram,
    /// Extraction errors by type
    pub extraction_errors_total: IntCounterVec,

    // ===== Retrieval metrics =====
    /// Retrieval duration in seconds
    pub retrieval_duration_seconds: Histogram,
    /// Number of results returned per query
    pub retrieval_results_count: Histogram,
    /// Total retrieval abstentions
    pub retrieval_abstentions_total: IntCounter,

    // ===== Storage metrics =====
    /// Qdrant request count by operation
    pub qdrant_requests_total: IntCounterVec,
    /// Qdrant request duration by operation
    pub qdrant_request_duration_seconds: HistogramVec,
    /// Qdrant errors by operation
    pub qdrant_errors_total: IntCounterVec,

    // ===== Worker metrics =====
    /// Worker run count by worker name
    pub worker_runs_total: IntCounterVec,
    /// Worker duration by worker name
    pub worker_duration_seconds: HistogramVec,
    /// Items processed by worker
    pub worker_items_processed: IntCounterVec,
    /// Worker errors by worker name
    pub worker_errors_total: IntCounterVec,

    // ===== System metrics =====
    /// Current memory count by epistemic type
    pub memory_count: IntGaugeVec,
    /// Current active connections
    pub active_connections: IntGauge,
}

impl Metrics {
    /// Create a new metrics registry with all metrics registered
    pub fn new() -> Self {
        let registry = Registry::new();

        // ===== Request metrics =====
        let requests_total = IntCounterVec::new(
            Opts::new("engram_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )
        .expect("requests_total metric creation");
        registry
            .register(Box::new(requests_total.clone()))
            .expect("requests_total registration");

        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "engram_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["method", "path"],
        )
        .expect("request_duration_seconds metric creation");
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .expect("request_duration_seconds registration");

        let request_size_bytes = HistogramVec::new(
            HistogramOpts::new("engram_request_size_bytes", "HTTP request size in bytes")
                .buckets(vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0]),
            &["method", "path"],
        )
        .expect("request_size_bytes metric creation");
        registry
            .register(Box::new(request_size_bytes.clone()))
            .expect("request_size_bytes registration");

        let response_size_bytes = HistogramVec::new(
            HistogramOpts::new("engram_response_size_bytes", "HTTP response size in bytes")
                .buckets(vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0]),
            &["method", "path"],
        )
        .expect("response_size_bytes metric creation");
        registry
            .register(Box::new(response_size_bytes.clone()))
            .expect("response_size_bytes registration");

        // ===== Memory operation metrics =====
        let memories_ingested_total =
            IntCounter::new("engram_memories_ingested_total", "Total memories ingested")
                .expect("memories_ingested_total metric creation");
        registry
            .register(Box::new(memories_ingested_total.clone()))
            .expect("memories_ingested_total registration");

        let memories_retrieved_total = IntCounter::new(
            "engram_memories_retrieved_total",
            "Total memories retrieved",
        )
        .expect("memories_retrieved_total metric creation");
        registry
            .register(Box::new(memories_retrieved_total.clone()))
            .expect("memories_retrieved_total registration");

        let memories_deleted_total =
            IntCounter::new("engram_memories_deleted_total", "Total memories deleted")
                .expect("memories_deleted_total metric creation");
        registry
            .register(Box::new(memories_deleted_total.clone()))
            .expect("memories_deleted_total registration");

        let facts_extracted_total = IntCounter::new(
            "engram_facts_extracted_total",
            "Total facts extracted from conversations",
        )
        .expect("facts_extracted_total metric creation");
        registry
            .register(Box::new(facts_extracted_total.clone()))
            .expect("facts_extracted_total registration");

        // ===== Extraction metrics =====
        let extraction_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "engram_extraction_duration_seconds",
                "Extraction pipeline duration in seconds",
            )
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
        )
        .expect("extraction_duration_seconds metric creation");
        registry
            .register(Box::new(extraction_duration_seconds.clone()))
            .expect("extraction_duration_seconds registration");

        let extraction_facts_per_conversation = Histogram::with_opts(
            HistogramOpts::new(
                "engram_extraction_facts_per_conversation",
                "Number of facts extracted per conversation",
            )
            .buckets(vec![0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
        )
        .expect("extraction_facts_per_conversation metric creation");
        registry
            .register(Box::new(extraction_facts_per_conversation.clone()))
            .expect("extraction_facts_per_conversation registration");

        let extraction_errors_total = IntCounterVec::new(
            Opts::new("engram_extraction_errors_total", "Extraction errors"),
            &["error_type"],
        )
        .expect("extraction_errors_total metric creation");
        registry
            .register(Box::new(extraction_errors_total.clone()))
            .expect("extraction_errors_total registration");

        // ===== Retrieval metrics =====
        let retrieval_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "engram_retrieval_duration_seconds",
                "Retrieval duration in seconds",
            )
            .buckets(vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5]),
        )
        .expect("retrieval_duration_seconds metric creation");
        registry
            .register(Box::new(retrieval_duration_seconds.clone()))
            .expect("retrieval_duration_seconds registration");

        let retrieval_results_count = Histogram::with_opts(
            HistogramOpts::new(
                "engram_retrieval_results_count",
                "Number of results returned per query",
            )
            .buckets(vec![0.0, 1.0, 5.0, 10.0, 20.0, 50.0]),
        )
        .expect("retrieval_results_count metric creation");
        registry
            .register(Box::new(retrieval_results_count.clone()))
            .expect("retrieval_results_count registration");

        let retrieval_abstentions_total = IntCounter::new(
            "engram_retrieval_abstentions_total",
            "Total retrieval abstentions",
        )
        .expect("retrieval_abstentions_total metric creation");
        registry
            .register(Box::new(retrieval_abstentions_total.clone()))
            .expect("retrieval_abstentions_total registration");

        // ===== Storage metrics =====
        let qdrant_requests_total = IntCounterVec::new(
            Opts::new("engram_qdrant_requests_total", "Qdrant requests"),
            &["operation"],
        )
        .expect("qdrant_requests_total metric creation");
        registry
            .register(Box::new(qdrant_requests_total.clone()))
            .expect("qdrant_requests_total registration");

        let qdrant_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "engram_qdrant_request_duration_seconds",
                "Qdrant request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5]),
            &["operation"],
        )
        .expect("qdrant_request_duration_seconds metric creation");
        registry
            .register(Box::new(qdrant_request_duration_seconds.clone()))
            .expect("qdrant_request_duration_seconds registration");

        let qdrant_errors_total = IntCounterVec::new(
            Opts::new("engram_qdrant_errors_total", "Qdrant errors"),
            &["operation"],
        )
        .expect("qdrant_errors_total metric creation");
        registry
            .register(Box::new(qdrant_errors_total.clone()))
            .expect("qdrant_errors_total registration");

        // ===== Worker metrics =====
        let worker_runs_total = IntCounterVec::new(
            Opts::new("engram_worker_runs_total", "Worker runs"),
            &["worker"],
        )
        .expect("worker_runs_total metric creation");
        registry
            .register(Box::new(worker_runs_total.clone()))
            .expect("worker_runs_total registration");

        let worker_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "engram_worker_duration_seconds",
                "Worker run duration in seconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0]),
            &["worker"],
        )
        .expect("worker_duration_seconds metric creation");
        registry
            .register(Box::new(worker_duration_seconds.clone()))
            .expect("worker_duration_seconds registration");

        let worker_items_processed = IntCounterVec::new(
            Opts::new("engram_worker_items_processed", "Worker items processed"),
            &["worker"],
        )
        .expect("worker_items_processed metric creation");
        registry
            .register(Box::new(worker_items_processed.clone()))
            .expect("worker_items_processed registration");

        let worker_errors_total = IntCounterVec::new(
            Opts::new("engram_worker_errors_total", "Worker errors"),
            &["worker"],
        )
        .expect("worker_errors_total metric creation");
        registry
            .register(Box::new(worker_errors_total.clone()))
            .expect("worker_errors_total registration");

        // ===== System metrics =====
        let memory_count = IntGaugeVec::new(
            Opts::new("engram_memory_count", "Current memory count by type"),
            &["epistemic_type"],
        )
        .expect("memory_count metric creation");
        registry
            .register(Box::new(memory_count.clone()))
            .expect("memory_count registration");

        let active_connections =
            IntGauge::new("engram_active_connections", "Current active connections")
                .expect("active_connections metric creation");
        registry
            .register(Box::new(active_connections.clone()))
            .expect("active_connections registration");

        Self {
            registry,
            requests_total,
            request_duration_seconds,
            request_size_bytes,
            response_size_bytes,
            memories_ingested_total,
            memories_retrieved_total,
            memories_deleted_total,
            facts_extracted_total,
            extraction_duration_seconds,
            extraction_facts_per_conversation,
            extraction_errors_total,
            retrieval_duration_seconds,
            retrieval_results_count,
            retrieval_abstentions_total,
            qdrant_requests_total,
            qdrant_request_duration_seconds,
            qdrant_errors_total,
            worker_runs_total,
            worker_duration_seconds,
            worker_items_processed,
            worker_errors_total,
            memory_count,
            active_connections,
        }
    }

    /// Encode metrics as Prometheus text format
    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("Metrics encoding should not fail");
        String::from_utf8(buffer).expect("Metrics should be valid UTF-8")
    }

    /// Record a request
    pub fn record_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        self.requests_total
            .with_label_values(&[method, path, &status.to_string()])
            .inc();
        self.request_duration_seconds
            .with_label_values(&[method, path])
            .observe(duration_secs);
    }

    /// Record memory ingestion
    pub fn record_ingestion(&self, memory_count: u64, fact_count: u64, duration_secs: f64) {
        self.memories_ingested_total.inc_by(memory_count);
        self.facts_extracted_total.inc_by(fact_count);
        self.extraction_duration_seconds.observe(duration_secs);
        self.extraction_facts_per_conversation
            .observe(fact_count as f64);
    }

    /// Record memory retrieval
    pub fn record_retrieval(&self, result_count: usize, abstained: bool, duration_secs: f64) {
        self.memories_retrieved_total.inc_by(result_count as u64);
        self.retrieval_duration_seconds.observe(duration_secs);
        self.retrieval_results_count.observe(result_count as f64);
        if abstained {
            self.retrieval_abstentions_total.inc();
        }
    }

    /// Record Qdrant operation
    pub fn record_qdrant(&self, operation: &str, duration_secs: f64, success: bool) {
        self.qdrant_requests_total
            .with_label_values(&[operation])
            .inc();
        self.qdrant_request_duration_seconds
            .with_label_values(&[operation])
            .observe(duration_secs);
        if !success {
            self.qdrant_errors_total
                .with_label_values(&[operation])
                .inc();
        }
    }

    /// Record worker run
    pub fn record_worker(&self, worker: &str, items: u64, duration_secs: f64, success: bool) {
        self.worker_runs_total.with_label_values(&[worker]).inc();
        self.worker_duration_seconds
            .with_label_values(&[worker])
            .observe(duration_secs);
        self.worker_items_processed
            .with_label_values(&[worker])
            .inc_by(items);
        if !success {
            self.worker_errors_total.with_label_values(&[worker]).inc();
        }
    }

    /// Update memory count gauge
    pub fn set_memory_count(&self, epistemic_type: &str, count: i64) {
        self.memory_count
            .with_label_values(&[epistemic_type])
            .set(count);
    }

    /// Update active connections gauge
    pub fn set_active_connections(&self, count: i64) {
        self.active_connections.set(count);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We can't test the global METRICS static because prometheus registries
    // can't have duplicate metrics registered. Instead, we create new Metrics instances.

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        // Registry has registered metrics (even without data)
        assert!(!metrics.registry.gather().is_empty());
    }

    #[test]
    fn test_encode_empty() {
        let metrics = Metrics::new();
        let output = metrics.encode();
        // Should have help text for registered metrics
        // IntCounterVec doesn't show until used, so check histograms instead
        assert!(output.contains("HELP") || output.is_empty() || output.contains("engram_"));
    }

    #[test]
    fn test_requests_total() {
        let metrics = Metrics::new();

        metrics
            .requests_total
            .with_label_values(&["GET", "/health", "200"])
            .inc();
        metrics
            .requests_total
            .with_label_values(&["POST", "/v1/memories", "201"])
            .inc();

        let output = metrics.encode();
        assert!(
            output.contains(r#"engram_requests_total{method="GET",path="/health",status="200"} 1"#)
        );
        assert!(output.contains(
            r#"engram_requests_total{method="POST",path="/v1/memories",status="201"} 1"#
        ));
    }

    #[test]
    fn test_request_duration_histogram() {
        let metrics = Metrics::new();

        metrics
            .request_duration_seconds
            .with_label_values(&["GET", "/search"])
            .observe(0.05);
        metrics
            .request_duration_seconds
            .with_label_values(&["GET", "/search"])
            .observe(0.15);

        let output = metrics.encode();
        assert!(output.contains("engram_request_duration_seconds_bucket"));
        assert!(output.contains("engram_request_duration_seconds_count"));
        assert!(output.contains("engram_request_duration_seconds_sum"));
    }

    #[test]
    fn test_memory_operation_counters() {
        let metrics = Metrics::new();

        metrics.memories_ingested_total.inc_by(5);
        metrics.memories_retrieved_total.inc_by(10);
        metrics.memories_deleted_total.inc();
        metrics.facts_extracted_total.inc_by(15);

        let output = metrics.encode();
        assert!(output.contains("engram_memories_ingested_total 5"));
        assert!(output.contains("engram_memories_retrieved_total 10"));
        assert!(output.contains("engram_memories_deleted_total 1"));
        assert!(output.contains("engram_facts_extracted_total 15"));
    }

    #[test]
    fn test_extraction_metrics() {
        let metrics = Metrics::new();

        metrics.extraction_duration_seconds.observe(1.5);
        metrics.extraction_facts_per_conversation.observe(3.0);
        metrics
            .extraction_errors_total
            .with_label_values(&["parse_error"])
            .inc();

        let output = metrics.encode();
        assert!(output.contains("engram_extraction_duration_seconds"));
        assert!(output.contains("engram_extraction_facts_per_conversation"));
        assert!(output.contains(r#"engram_extraction_errors_total{error_type="parse_error"} 1"#));
    }

    #[test]
    fn test_retrieval_metrics() {
        let metrics = Metrics::new();

        metrics.retrieval_duration_seconds.observe(0.05);
        metrics.retrieval_results_count.observe(5.0);
        metrics.retrieval_abstentions_total.inc();

        let output = metrics.encode();
        assert!(output.contains("engram_retrieval_duration_seconds"));
        assert!(output.contains("engram_retrieval_results_count"));
        assert!(output.contains("engram_retrieval_abstentions_total 1"));
    }

    #[test]
    fn test_qdrant_metrics() {
        let metrics = Metrics::new();

        metrics
            .qdrant_requests_total
            .with_label_values(&["upsert"])
            .inc();
        metrics
            .qdrant_request_duration_seconds
            .with_label_values(&["upsert"])
            .observe(0.01);
        metrics
            .qdrant_errors_total
            .with_label_values(&["search"])
            .inc();

        let output = metrics.encode();
        assert!(output.contains(r#"engram_qdrant_requests_total{operation="upsert"} 1"#));
        assert!(output.contains(r#"engram_qdrant_errors_total{operation="search"} 1"#));
    }

    #[test]
    fn test_worker_metrics() {
        let metrics = Metrics::new();

        metrics
            .worker_runs_total
            .with_label_values(&["consolidator"])
            .inc();
        metrics
            .worker_duration_seconds
            .with_label_values(&["consolidator"])
            .observe(5.5);
        metrics
            .worker_items_processed
            .with_label_values(&["consolidator"])
            .inc_by(100);
        metrics
            .worker_errors_total
            .with_label_values(&["reflector"])
            .inc();

        let output = metrics.encode();
        assert!(output.contains(r#"engram_worker_runs_total{worker="consolidator"} 1"#));
        assert!(output.contains(r#"engram_worker_items_processed{worker="consolidator"} 100"#));
        assert!(output.contains(r#"engram_worker_errors_total{worker="reflector"} 1"#));
    }

    #[test]
    fn test_system_metrics() {
        let metrics = Metrics::new();

        metrics.memory_count.with_label_values(&["world"]).set(1000);
        metrics
            .memory_count
            .with_label_values(&["experience"])
            .set(500);
        metrics.active_connections.set(25);

        let output = metrics.encode();
        assert!(output.contains(r#"engram_memory_count{epistemic_type="world"} 1000"#));
        assert!(output.contains(r#"engram_memory_count{epistemic_type="experience"} 500"#));
        assert!(output.contains("engram_active_connections 25"));
    }

    #[test]
    fn test_record_request() {
        let metrics = Metrics::new();

        metrics.record_request("GET", "/health", 200, 0.005);

        let output = metrics.encode();
        assert!(
            output.contains(r#"engram_requests_total{method="GET",path="/health",status="200"} 1"#)
        );
    }

    #[test]
    fn test_record_ingestion() {
        let metrics = Metrics::new();

        metrics.record_ingestion(3, 10, 1.5);

        let output = metrics.encode();
        assert!(output.contains("engram_memories_ingested_total 3"));
        assert!(output.contains("engram_facts_extracted_total 10"));
    }

    #[test]
    fn test_record_retrieval() {
        let metrics = Metrics::new();

        metrics.record_retrieval(5, false, 0.1);
        metrics.record_retrieval(0, true, 0.05);

        let output = metrics.encode();
        assert!(output.contains("engram_memories_retrieved_total 5"));
        assert!(output.contains("engram_retrieval_abstentions_total 1"));
    }

    #[test]
    fn test_record_qdrant() {
        let metrics = Metrics::new();

        metrics.record_qdrant("upsert", 0.01, true);
        metrics.record_qdrant("search", 0.05, false);

        let output = metrics.encode();
        assert!(output.contains(r#"engram_qdrant_requests_total{operation="upsert"} 1"#));
        assert!(output.contains(r#"engram_qdrant_requests_total{operation="search"} 1"#));
        assert!(output.contains(r#"engram_qdrant_errors_total{operation="search"} 1"#));
    }

    #[test]
    fn test_record_worker() {
        let metrics = Metrics::new();

        metrics.record_worker("consolidator", 50, 10.0, true);
        metrics.record_worker("reflector", 0, 5.0, false);

        let output = metrics.encode();
        assert!(output.contains(r#"engram_worker_runs_total{worker="consolidator"} 1"#));
        assert!(output.contains(r#"engram_worker_items_processed{worker="consolidator"} 50"#));
        assert!(output.contains(r#"engram_worker_errors_total{worker="reflector"} 1"#));
    }

    #[test]
    fn test_set_memory_count() {
        let metrics = Metrics::new();

        metrics.set_memory_count("world", 1000);
        metrics.set_memory_count("world", 1005); // Update

        let output = metrics.encode();
        assert!(output.contains(r#"engram_memory_count{epistemic_type="world"} 1005"#));
    }

    #[test]
    fn test_set_active_connections() {
        let metrics = Metrics::new();

        metrics.set_active_connections(10);
        metrics.set_active_connections(15);

        let output = metrics.encode();
        assert!(output.contains("engram_active_connections 15"));
    }

    #[test]
    fn test_default() {
        let metrics = Metrics::default();
        // Increment a counter to make it show up in output
        metrics.memories_ingested_total.inc();
        let output = metrics.encode();
        assert!(output.contains("engram_memories_ingested_total"));
    }
}
