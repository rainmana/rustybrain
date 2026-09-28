mod openai_compat;

pub use openai_compat::OpenAiCompatProvider;

use async_trait::async_trait;

/// All embedding providers implement this trait.
///
/// The system supports two backends from day one:
/// - [`OpenAiCompatProvider`]: any endpoint speaking the OpenAI embeddings API
///   (first-party OpenAI, Voyage, LM Studio, Ollama, vLLM, llama.cpp, …)
/// - `LocalProvider` (behind the `local-embed` feature): fastembed-rs ONNX
///   model running in-process; fully air-gapped, no API key required.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Fixed output dimensionality. Must match `F32_BLOB(N)` in the schema.
    fn dimensions(&self) -> usize;

    /// Provider name for logging and diagnostics.
    fn name(&self) -> &str;

    /// Embed a single text string. Used at query time.
    async fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>>;

    /// Embed a batch of texts. Used at store time.
    /// Default implementation calls `embed_one` in sequence; providers may
    /// override with a native batch endpoint.
    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_one(text).await?);
        }
        Ok(results)
    }
}
