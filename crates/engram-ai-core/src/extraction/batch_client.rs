//! OpenAI Batch API client
//!
//! Handles file upload, batch submission, status polling, and result download.

use std::time::Duration;

use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{ExtractionError, Result};

/// Status of a batch job
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchStatus {
    /// Batch is being validated
    Validating,
    /// Batch is queued for processing
    InProgress {
        completed: usize,
        failed: usize,
        total: usize,
    },
    /// Batch completed successfully
    Completed {
        output_file_id: String,
        error_file_id: Option<String>,
    },
    /// Batch failed
    Failed { error: String },
    /// Batch expired (not completed within 24h)
    Expired,
    /// Batch was cancelled
    Cancelled,
}

/// OpenAI Batch API response
#[derive(Debug, Deserialize)]
struct BatchResponse {
    id: String,
    status: String,
    #[serde(default)]
    request_counts: Option<RequestCounts>,
    output_file_id: Option<String>,
    error_file_id: Option<String>,
    errors: Option<BatchErrors>,
}

#[derive(Debug, Deserialize)]
struct RequestCounts {
    total: usize,
    completed: usize,
    failed: usize,
}

#[derive(Debug, Deserialize)]
struct BatchErrors {
    data: Vec<BatchErrorData>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BatchErrorData {
    code: String,
    message: String,
}

/// OpenAI File API response
#[derive(Debug, Deserialize)]
struct FileResponse {
    id: String,
}

/// Result of polling a batch
#[derive(Debug, Clone)]
pub struct BatchPollResult {
    pub status: BatchStatus,
    pub batch_id: String,
}

/// OpenAI Batch API client
pub struct BatchClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl BatchClient {
    /// Create a new batch client
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // 5 min timeout for large uploads
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_key: api_key.into(),
            base_url: "https://api.openai.com".to_string(),
        }
    }

    /// Create from environment variable
    pub fn from_env() -> Option<Self> {
        std::env::var("OPENAI_API_KEY")
            .ok()
            .map(|key| Self::new(key))
    }

    /// Upload JSONL content to OpenAI Files API
    ///
    /// Returns the file_id for use in batch submission.
    pub async fn upload_file(&self, content: &str) -> Result<String> {
        let url = format!("{}/v1/files", self.base_url);

        // Create multipart form with the JSONL content
        let part = Part::text(content.to_string())
            .file_name("batch_input.jsonl")
            .mime_str("application/jsonl")
            .map_err(|e| ExtractionError::Api(format!("Failed to create file part: {}", e)))?;

        let form = Form::new().text("purpose", "batch").part("file", part);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| ExtractionError::Api(format!("File upload request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(
                ExtractionError::Api(format!("File upload failed ({}): {}", status, body)).into(),
            );
        }

        let file_response: FileResponse = response
            .json()
            .await
            .map_err(|e| ExtractionError::Api(format!("Failed to parse file response: {}", e)))?;

        Ok(file_response.id)
    }

    /// Submit a batch job
    ///
    /// Returns the batch_id for polling.
    pub async fn submit_batch(&self, file_id: &str) -> Result<String> {
        let url = format!("{}/v1/batches", self.base_url);

        #[derive(Serialize)]
        struct BatchSubmitRequest {
            input_file_id: String,
            endpoint: String,
            completion_window: String,
        }

        let request = BatchSubmitRequest {
            input_file_id: file_id.to_string(),
            endpoint: "/v1/chat/completions".to_string(),
            completion_window: "24h".to_string(),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ExtractionError::Api(format!("Batch submit request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ExtractionError::Api(format!(
                "Batch submit failed ({}): {}",
                status, body
            ))
            .into());
        }

        let batch_response: BatchResponse = response
            .json()
            .await
            .map_err(|e| ExtractionError::Api(format!("Failed to parse batch response: {}", e)))?;

        Ok(batch_response.id)
    }

    /// Poll batch status
    pub async fn get_batch_status(&self, batch_id: &str) -> Result<BatchPollResult> {
        let url = format!("{}/v1/batches/{}", self.base_url, batch_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ExtractionError::Api(format!("Batch status request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ExtractionError::Api(format!(
                "Batch status failed ({}): {}",
                status, body
            ))
            .into());
        }

        let batch_response: BatchResponse = response.json().await.map_err(|e| {
            ExtractionError::Api(format!("Failed to parse batch status response: {}", e))
        })?;

        // Debug: log the status we received
        eprintln!(
            "[BatchClient] Batch {} status='{}' counts={:?}",
            batch_response.id, batch_response.status, batch_response.request_counts
        );

        let status = match batch_response.status.as_str() {
            "validating" => BatchStatus::Validating,
            "in_progress" | "finalizing" => {
                let counts = batch_response.request_counts.unwrap_or(RequestCounts {
                    total: 0,
                    completed: 0,
                    failed: 0,
                });
                BatchStatus::InProgress {
                    completed: counts.completed,
                    failed: counts.failed,
                    total: counts.total,
                }
            }
            "completed" => BatchStatus::Completed {
                output_file_id: batch_response.output_file_id.unwrap_or_default(),
                error_file_id: batch_response.error_file_id,
            },
            "failed" => {
                let error = batch_response
                    .errors
                    .and_then(|e| e.data.first().map(|d| d.message.clone()))
                    .unwrap_or_else(|| "Unknown error".to_string());
                BatchStatus::Failed { error }
            }
            "expired" => BatchStatus::Expired,
            "cancelled" | "cancelling" => BatchStatus::Cancelled,
            other => BatchStatus::Failed {
                error: format!("Unknown status: {}", other),
            },
        };

        Ok(BatchPollResult {
            status,
            batch_id: batch_response.id,
        })
    }

    /// Download results from a completed batch
    ///
    /// Returns the JSONL content.
    pub async fn get_results(&self, output_file_id: &str) -> Result<String> {
        let url = format!("{}/v1/files/{}/content", self.base_url, output_file_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ExtractionError::Api(format!("File download request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ExtractionError::Api(format!(
                "File download failed ({}): {}",
                status, body
            ))
            .into());
        }

        response
            .text()
            .await
            .map_err(|e| ExtractionError::Api(format!("Failed to read file content: {}", e)).into())
    }

    /// Poll with exponential backoff until complete or failed
    ///
    /// Returns the final status.
    pub async fn poll_until_complete(
        &self,
        batch_id: &str,
        initial_delay: Duration,
        max_delay: Duration,
        max_attempts: usize,
    ) -> Result<BatchPollResult> {
        let mut delay = initial_delay;
        let mut attempts = 0;

        loop {
            let result = self.get_batch_status(batch_id).await?;

            match &result.status {
                BatchStatus::Completed { .. }
                | BatchStatus::Failed { .. }
                | BatchStatus::Expired
                | BatchStatus::Cancelled => {
                    return Ok(result);
                }
                BatchStatus::InProgress {
                    completed,
                    failed,
                    total,
                } => {
                    tracing::info!(
                        "Batch {} in progress: {}/{} completed, {} failed",
                        batch_id,
                        completed,
                        total,
                        failed
                    );
                }
                BatchStatus::Validating => {
                    tracing::info!("Batch {} is validating...", batch_id);
                }
            }

            attempts += 1;
            if attempts >= max_attempts {
                return Err(ExtractionError::Api(format!(
                    "Batch {} did not complete after {} attempts",
                    batch_id, max_attempts
                ))
                .into());
            }

            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(max_delay);
        }
    }

    /// Cancel a batch job
    pub async fn cancel_batch(&self, batch_id: &str) -> Result<()> {
        let url = format!("{}/v1/batches/{}/cancel", self.base_url, batch_id);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ExtractionError::Api(format!("Batch cancel request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ExtractionError::Api(format!(
                "Batch cancel failed ({}): {}",
                status, body
            ))
            .into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_status_variants() {
        let validating = BatchStatus::Validating;
        assert_eq!(validating, BatchStatus::Validating);

        let in_progress = BatchStatus::InProgress {
            completed: 10,
            failed: 2,
            total: 100,
        };
        if let BatchStatus::InProgress {
            completed,
            failed,
            total,
        } = in_progress
        {
            assert_eq!(completed, 10);
            assert_eq!(failed, 2);
            assert_eq!(total, 100);
        }

        let completed = BatchStatus::Completed {
            output_file_id: "file_123".to_string(),
            error_file_id: None,
        };
        if let BatchStatus::Completed {
            output_file_id,
            error_file_id,
        } = completed
        {
            assert_eq!(output_file_id, "file_123");
            assert!(error_file_id.is_none());
        }
    }

    #[test]
    fn test_client_from_env() {
        // This will return None if OPENAI_API_KEY is not set
        let _client = BatchClient::from_env();
    }

    #[tokio::test]
    #[ignore] // Requires OPENAI_API_KEY
    async fn test_upload_and_submit() {
        let client = BatchClient::from_env().expect("OPENAI_API_KEY must be set");

        // Create a minimal JSONL file
        let jsonl = r#"{"custom_id":"test_1","method":"POST","url":"/v1/chat/completions","body":{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Say hello"}]}}"#;

        let file_id = client
            .upload_file(jsonl)
            .await
            .expect("Upload should succeed");
        println!("Uploaded file: {}", file_id);

        let batch_id = client
            .submit_batch(&file_id)
            .await
            .expect("Submit should succeed");
        println!("Submitted batch: {}", batch_id);

        // Cancel it immediately to avoid charges
        client
            .cancel_batch(&batch_id)
            .await
            .expect("Cancel should succeed");
        println!("Cancelled batch");
    }
}
