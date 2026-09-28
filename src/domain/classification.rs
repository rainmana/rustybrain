use serde::{Deserialize, Serialize};

/// Classification status of an IU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationStatus {
    Pending,
    Classified,
    Failed,
}

impl ClassificationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClassificationStatus::Pending => "pending",
            ClassificationStatus::Classified => "classified",
            ClassificationStatus::Failed => "failed",
        }
    }
}

impl std::fmt::Display for ClassificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Jev question 1: semantic type of the IU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticType {
    Observation,
    Hypothesis,
    Finding,
    Vulnerability,
    Exploit,
    Payload,
    Technique,
    ToolResult,
    Reference,
    Constraint,
    Goal,
    Assumption,
    Correction,
    Clarification,
    Evidence,
    Recommendation,
    Ioc,
    Ttp,
    ThreatActor,
    Campaign,
    Osint,
    Humint,
    Sigint,
    Geoint,
    Techint,
    Finint,
    MalwareArtifact,
    BinaryArtifact,
    ProtocolArtifact,
    CoordinationSignal,
    RepairSignal,
    Unknown,
}

impl SemanticType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SemanticType::Observation => "observation",
            SemanticType::Hypothesis => "hypothesis",
            SemanticType::Finding => "finding",
            SemanticType::Vulnerability => "vulnerability",
            SemanticType::Exploit => "exploit",
            SemanticType::Payload => "payload",
            SemanticType::Technique => "technique",
            SemanticType::ToolResult => "tool_result",
            SemanticType::Reference => "reference",
            SemanticType::Constraint => "constraint",
            SemanticType::Goal => "goal",
            SemanticType::Assumption => "assumption",
            SemanticType::Correction => "correction",
            SemanticType::Clarification => "clarification",
            SemanticType::Evidence => "evidence",
            SemanticType::Recommendation => "recommendation",
            SemanticType::Ioc => "ioc",
            SemanticType::Ttp => "ttp",
            SemanticType::ThreatActor => "threat_actor",
            SemanticType::Campaign => "campaign",
            SemanticType::Osint => "osint",
            SemanticType::Humint => "humint",
            SemanticType::Sigint => "sigint",
            SemanticType::Geoint => "geoint",
            SemanticType::Techint => "techint",
            SemanticType::Finint => "finint",
            SemanticType::MalwareArtifact => "malware_artifact",
            SemanticType::BinaryArtifact => "binary_artifact",
            SemanticType::ProtocolArtifact => "protocol_artifact",
            SemanticType::CoordinationSignal => "coordination_signal",
            SemanticType::RepairSignal => "repair_signal",
            SemanticType::Unknown => "unknown",
        }
    }
}

/// Jev question 5: how much meaning is likely lost in this transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferLossiness {
    Minimal,
    Moderate,
    High,
    Severe,
}

impl TransferLossiness {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransferLossiness::Minimal => "minimal",
            TransferLossiness::Moderate => "moderate",
            TransferLossiness::High => "high",
            TransferLossiness::Severe => "severe",
        }
    }
}

/// MITRE ATT&CK tactic (Jev question 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MitreTactic {
    Reconnaissance,
    ResourceDevelopment,
    InitialAccess,
    Execution,
    Persistence,
    PrivilegeEscalation,
    DefenseEvasion,
    CredentialAccess,
    Discovery,
    LateralMovement,
    Collection,
    CommandAndControl,
    Exfiltration,
    Impact,
    None,
}

impl MitreTactic {
    pub fn as_str(&self) -> &'static str {
        match self {
            MitreTactic::Reconnaissance => "reconnaissance",
            MitreTactic::ResourceDevelopment => "resource_development",
            MitreTactic::InitialAccess => "initial_access",
            MitreTactic::Execution => "execution",
            MitreTactic::Persistence => "persistence",
            MitreTactic::PrivilegeEscalation => "privilege_escalation",
            MitreTactic::DefenseEvasion => "defense_evasion",
            MitreTactic::CredentialAccess => "credential_access",
            MitreTactic::Discovery => "discovery",
            MitreTactic::LateralMovement => "lateral_movement",
            MitreTactic::Collection => "collection",
            MitreTactic::CommandAndControl => "command_and_control",
            MitreTactic::Exfiltration => "exfiltration",
            MitreTactic::Impact => "impact",
            MitreTactic::None => "none",
        }
    }
}

/// The structured result returned by Jev for a single IU.
///
/// All fields are `Option` because classification is async; an IU may be
/// stored before all fields are populated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub semantic_type: Option<SemanticType>,
    /// 0–4 priority score from Jev question 2.
    pub priority: Option<i64>,
    /// Probability 0.0–1.0 that this IU is a coordination signal (question 3).
    pub is_coordination_signal: Option<f64>,
    /// Probability 0.0–1.0 that this IU is a repair attempt (question 4).
    pub is_repair_attempt: Option<f64>,
    pub transfer_lossiness: Option<TransferLossiness>,
    /// Probability 0.0–1.0 that this IU contains an unstated assumption (question 6).
    pub contains_assumption: Option<f64>,
    /// Probability 0.0–1.0 that this IU contains PII (question 7).
    pub contains_pii: Option<f64>,
    pub mitre_tactic: Option<MitreTactic>,
}
