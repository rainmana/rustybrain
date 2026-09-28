use async_trait::async_trait;

use crate::domain::classification::{ClassificationResult, SemanticType, TransferLossiness};

use super::{ClassificationRequest, Classifier};

/// Test-only classifier that returns deterministic results without network calls.
pub struct MockClassifier;

#[async_trait]
impl Classifier for MockClassifier {
    async fn classify(
        &self,
        _request: ClassificationRequest,
    ) -> anyhow::Result<ClassificationResult> {
        Ok(ClassificationResult {
            semantic_type: Some(SemanticType::Finding),
            priority: Some(2),
            is_coordination_signal: Some(0.1),
            is_repair_attempt: Some(0.0),
            transfer_lossiness: Some(TransferLossiness::Moderate),
            contains_assumption: Some(0.2),
            contains_pii: Some(0.0),
            mitre_tactic: None,
        })
    }
}
