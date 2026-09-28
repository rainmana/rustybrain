mod jev;
#[cfg(test)]
pub mod mock;

pub use jev::JevClassifier;
#[cfg(test)]
pub use mock::MockClassifier;

use async_trait::async_trait;

use crate::domain::classification::ClassificationResult;

/// Input to the classifier for a single IU.
#[derive(Debug, Clone)]
pub struct ClassificationRequest {
    /// The IU's title (brief summary).
    pub title: String,
    /// The IU content, truncated to 4000 chars if necessary before sending.
    pub content: String,
    /// Transfer direction as a display string (e.g. "human→ai").
    pub transfer_direction: String,
    /// Recent MCW state summary (≤500 chars) for context.
    pub session_context: Option<String>,
}

/// All classifiers implement this trait, including [`JevClassifier`] and
/// [`MockClassifier`] (test only).
///
/// Swapping classifiers does not change the data model — all implementations
/// must return the same [`ClassificationResult`] shape.
#[async_trait]
pub trait Classifier: Send + Sync {
    async fn classify(&self, request: ClassificationRequest) -> anyhow::Result<ClassificationResult>;
}
