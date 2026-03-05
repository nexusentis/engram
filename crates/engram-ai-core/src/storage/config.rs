use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantConfig {
    /// "embedded" or "external"
    pub mode: String,
    /// URL for external mode (e.g., "http://localhost:6334")
    pub url: Option<String>,
    /// Path for embedded mode data storage
    pub path: Option<String>,
    /// Vector dimension (1536 for text-embedding-3-small)
    pub vector_size: u64,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            mode: "external".to_string(),
            url: Some("http://localhost:6334".to_string()),
            path: None,
            vector_size: 1536,
        }
    }
}

impl QdrantConfig {
    pub fn external(url: impl Into<String>) -> Self {
        Self {
            mode: "external".to_string(),
            url: Some(url.into()),
            path: None,
            vector_size: 1536,
        }
    }

    pub fn with_vector_size(mut self, size: u64) -> Self {
        self.vector_size = size;
        self
    }
}
