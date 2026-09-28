use serde::{Deserialize, Serialize};

/// The six MCW failure modes defined in the framework (Akin, 2026).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    /// Context diverges silently over time.
    Drift,
    /// One actor's state advances without the change being externalized.
    AsymmetricStateAdvancement,
    /// Shared language hides different interpretations.
    FalseAlignment,
    /// Summarization destroys distinctions needed later.
    Overcompression,
    /// A hidden variable shapes behavior without becoming legible.
    ConstraintOpacity,
    /// Clarification or uncertainty is penalized; questions stop.
    RepairSuppression,
}

impl FailureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureMode::Drift => "drift",
            FailureMode::AsymmetricStateAdvancement => "asymmetric_state_advancement",
            FailureMode::FalseAlignment => "false_alignment",
            FailureMode::Overcompression => "overcompression",
            FailureMode::ConstraintOpacity => "constraint_opacity",
            FailureMode::RepairSuppression => "repair_suppression",
        }
    }
}

impl std::fmt::Display for FailureMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The five canonical repair operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairType {
    /// Reintroduce foundational goals and assumptions.
    ReGrounding,
    /// Expand a compressed bundle back into its distinctions.
    Decompression,
    /// Explicitly change which IUs carry the most salience.
    Reweighting,
    /// Split an overloaded term into its competing interpretations.
    Disambiguation,
    /// Externalize state changes that occurred outside the shared exchange.
    /// This is the most critical repair — it bridges HCW advances that the
    /// AI cannot observe.
    Synchronization,
}

impl RepairType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepairType::ReGrounding => "re_grounding",
            RepairType::Decompression => "decompression",
            RepairType::Reweighting => "reweighting",
            RepairType::Disambiguation => "disambiguation",
            RepairType::Synchronization => "synchronization",
        }
    }
}

impl std::fmt::Display for RepairType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The overall health signal for a session's MCW.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthSignal {
    Nominal,
    DriftDetected,
    AsaDetected,
    FalseAlignment,
    Overcompressed,
    ConstraintOpaque,
    RepairSuppressed,
}

impl HealthSignal {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthSignal::Nominal => "nominal",
            HealthSignal::DriftDetected => "drift_detected",
            HealthSignal::AsaDetected => "asa_detected",
            HealthSignal::FalseAlignment => "false_alignment",
            HealthSignal::Overcompressed => "overcompressed",
            HealthSignal::ConstraintOpaque => "constraint_opaque",
            HealthSignal::RepairSuppressed => "repair_suppressed",
        }
    }
}

impl std::fmt::Display for HealthSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The live coordination state for a session.
///
/// One record per session. Updated by repair operations and drift detection.
/// Stored in the `mcw_state` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McwState {
    pub session_id: String,
    pub health_signal: HealthSignal,
    /// Active failure modes detected in this session.
    pub active_failure_modes: Vec<FailureMode>,
    /// Unix ms timestamp of the last repair operation.
    pub last_repair_at: Option<i64>,
    /// Unix ms timestamp of the last Synchronization repair.
    /// Monotonically-increasing gap here is the strongest Drift / ASA signal.
    pub last_sync_at: Option<i64>,
    /// Drift score 0.0 (none) → 1.0 (severe).
    pub drift_score: f64,
    /// How many Overcompression events have occurred since the last Decompression.
    pub compression_depth: i64,
    /// Count of Constraint Opacity signals detected.
    pub constraint_count: i64,
    /// IU IDs that represent active goals for this session.
    pub goals: Vec<String>,
    /// IU IDs that represent active constraints for this session.
    pub active_constraints: Vec<String>,
    pub updated_at: i64,
}

impl McwState {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            health_signal: HealthSignal::Nominal,
            active_failure_modes: Vec::new(),
            last_repair_at: None,
            last_sync_at: None,
            drift_score: 0.0,
            compression_depth: 0,
            constraint_count: 0,
            goals: Vec::new(),
            active_constraints: Vec::new(),
            updated_at: crate::domain::session::now_ms(),
        }
    }
}

/// A record of a single repair operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRecord {
    pub id: String,
    pub session_id: String,
    pub repair_type: RepairType,
    /// Which failure mode (if any) triggered this repair.
    pub failure_mode: Option<FailureMode>,
    pub description: String,
    /// IU IDs involved in this repair.
    pub iu_ids: Vec<String>,
    /// JSON snapshot of `mcw_state` before the repair.
    pub before_state: Option<String>,
    /// JSON snapshot of `mcw_state` after the repair.
    pub after_state: Option<String>,
    /// Who initiated this repair.
    pub initiated_by: InitiatedBy,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitiatedBy {
    Human,
    Ai,
    System,
}

impl InitiatedBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            InitiatedBy::Human => "human",
            InitiatedBy::Ai => "ai",
            InitiatedBy::System => "system",
        }
    }
}
