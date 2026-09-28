use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use super::EmbeddingProvider;

/// OpenAI-compatible embedding provider.
///
/// Supports any endpoint that speaks `POST /v1/embeddings`:
/// - OpenAI (`https://api.openai.com/v1`)
/// - Voyage AI (`https://api.voyageai.com/v1`)
/// - Local: LM Studio (`http://localhost:1234/v1`)
/// - Local: Ollama (`http://localhost:11434/v1`)
/// - Local: vLLM, llama.cpp server, etc.
///
/// For local servers pass an empty string as `api_key`.
pub struct OpenAiCompatProvider {
    base_url: String,
    api_key: String,
    model: String,
    dimensions: usize,
    client: Client,
}

impl OpenAiCompatProvider {
    /// Create a new provider.
    ///
    /// - `base_url`: e.g. `"https://api.openai.com/v1"` or `"http://localhost:11434/v1"`
    /// - `api_key`: bearer token; empty string for local servers
    /// - `model`: e.g. `"text-embedding-3-large"` or `"nomic-embed-text"`
    /// - `dimensions`: must match the actual output dimensionality of the model
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        dimensions: usize,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
            dimensions,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
    /// Matryoshka dimension reduction; pass `None` to use the model's native
    /// output size.
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

// ─── Trait impl ──────────────────────────────────────────────────────────────

#[async_trait]
impl EmbeddingProvider for OpenAiCompatProvider {
    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn name(&self) -> &str {
        &self.model
    }

    #[instrument(skip(self, texts), fields(model = %self.model, n = texts.len()))]
    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        debug!(n = texts.len(), "embedding batch");

        let url = format!("{}/embeddings", self.base_url);

        let body = EmbedRequest {
            model: &self.model,
            input: texts,
            // Only pass `dimensions` if the model supports Matryoshka reduction
            // (OpenAI text-embedding-3-*, voyage-3-large, etc.).
            // For models that don't, omit it — they'll return their native size.
            dimensions: None,
        };

        let mut req = self.client.post(&url).json(&body);

        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

        let resp = req.send().await?.error_for_status()?;
        let parsed: EmbedResponse = resp.json().await?;

        if parsed.data.len() != texts.len() {
            anyhow::bail!(
                "embedding provider returned {} vectors for {} inputs",
                parsed.data.len(),
                texts.len()
            );
        }

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    async fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut batch = self.embed_batch(&[text]).await?;
        batch
            .pop()
            .ok_or_else(|| anyhow::anyhow!("embedding provider returned empty batch"))
    }
}
