use serde::{Deserialize, Serialize};

/// The type of security / intelligence task a session represents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    SecurityReview,
    Pentest,
    ExploitDev,
    VulnAnalysis,
    ThreatModel,
    IncidentResponse,
    IntelAnalysis,
    ReverseEngineering,
    MalwareAnalysis,
    Ctf,
    General,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::SecurityReview => "security_review",
            TaskType::Pentest => "pentest",
            TaskType::ExploitDev => "exploit_dev",
            TaskType::VulnAnalysis => "vuln_analysis",
            TaskType::ThreatModel => "threat_model",
            TaskType::IncidentResponse => "incident_response",
            TaskType::IntelAnalysis => "intel_analysis",
            TaskType::ReverseEngineering => "reverse_engineering",
            TaskType::MalwareAnalysis => "malware_analysis",
            TaskType::Ctf => "ctf",
            TaskType::General => "general",
        }
    }
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskType {
    type Err = crate::domain::session::SessionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "security_review" => Ok(TaskType::SecurityReview),
            "pentest" => Ok(TaskType::Pentest),
            "exploit_dev" => Ok(TaskType::ExploitDev),
            "vuln_analysis" => Ok(TaskType::VulnAnalysis),
            "threat_model" => Ok(TaskType::ThreatModel),
            "incident_response" => Ok(TaskType::IncidentResponse),
            "intel_analysis" => Ok(TaskType::IntelAnalysis),
            "reverse_engineering" => Ok(TaskType::ReverseEngineering),
            "malware_analysis" => Ok(TaskType::MalwareAnalysis),
            "ctf" => Ok(TaskType::Ctf),
            "general" => Ok(TaskType::General),
            other => Err(SessionError::UnknownTaskType(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Paused,
    Completed,
    Archived,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Paused => "paused",
            SessionStatus::Completed => "completed",
            SessionStatus::Archived => "archived",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A collaboration session between a human and one or more AI actors.
/// Owns an [`crate::domain::mcw::McwState`] record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub description: String,
    pub task_type: TaskType,
    pub status: SessionStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Session {
    pub fn new(name: impl Into<String>, task_type: TaskType) -> Self {
        let now = crate::domain::session::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            description: String::new(),
            task_type,
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Domain errors for session operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("unknown task type: {0}")]
    UnknownTaskType(String),
}

/// Current time as Unix milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
