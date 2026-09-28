use serde::{Deserialize, Serialize};

use crate::domain::classification::{ClassificationResult, ClassificationStatus};

/// The direction an Information Unit is travelling between knowledge centers.
///
/// IU transfer is always lossy; the direction informs how much lossiness to
/// expect and which MCW failure modes are more likely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    HumanToAi,
    AiToHuman,
    AiToSubagent,
    SubagentToAi,
    RagToAi,
    AiToRag,
    ToolToAi,
    AiToTool,
    HumanToHuman,
    Unknown,
}

impl TransferDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransferDirection::HumanToAi => "human→ai",
            TransferDirection::AiToHuman => "ai→human",
            TransferDirection::AiToSubagent => "ai→subagent",
            TransferDirection::SubagentToAi => "subagent→ai",
            TransferDirection::RagToAi => "rag→ai",
            TransferDirection::AiToRag => "ai→rag",
            TransferDirection::ToolToAi => "tool→ai",
            TransferDirection::AiToTool => "ai→tool",
            TransferDirection::HumanToHuman => "human→human",
            TransferDirection::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for TransferDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The encoding/content type of an IU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Text,
    Code,
    Json,
    Yaml,
    Markdown,
    BinaryRef,
    Structured,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Text => "text",
            ContentType::Code => "code",
            ContentType::Json => "json",
            ContentType::Yaml => "yaml",
            ContentType::Markdown => "markdown",
            ContentType::BinaryRef => "binary_ref",
            ContentType::Structured => "structured",
        }
    }
}

impl Default for ContentType {
    fn default() -> Self {
        ContentType::Text
    }
}

/// The typed relationship between two IUs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    DependsOn,
    Causes,
    Mitigates,
    Exploits,
    References,
    Contradicts,
    Supports,
    RelatedTo,
    ParentOf,
    ChildOf,
    Repairs,
    Precedes,
    Supersedes,
    Corroborates,
    Refutes,
    Instantiates,
    Exemplifies,
    Abstracts,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::DependsOn => "depends_on",
            EdgeType::Causes => "causes",
            EdgeType::Mitigates => "mitigates",
            EdgeType::Exploits => "exploits",
            EdgeType::References => "references",
            EdgeType::Contradicts => "contradicts",
            EdgeType::Supports => "supports",
            EdgeType::RelatedTo => "related_to",
            EdgeType::ParentOf => "parent_of",
            EdgeType::ChildOf => "child_of",
            EdgeType::Repairs => "repairs",
            EdgeType::Precedes => "precedes",
            EdgeType::Supersedes => "supersedes",
            EdgeType::Corroborates => "corroborates",
            EdgeType::Refutes => "refutes",
            EdgeType::Instantiates => "instantiates",
            EdgeType::Exemplifies => "exemplifies",
            EdgeType::Abstracts => "abstracts",
        }
    }
}

/// An Information Unit — the fundamental primitive of the system.
///
/// Represents any piece of information crossing a boundary between knowledge
/// centers. Always lossy by nature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InformationUnit {
    pub id: String,
    pub session_id: String,

    // Content
    pub title: String,
    pub content: String,
    pub content_type: ContentType,

    // Transfer metadata
    pub transfer_direction: TransferDirection,
    pub source_actor: Option<String>,
    pub target_actor: Option<String>,

    // Classification
    pub classification_status: ClassificationStatus,
    #[serde(flatten)]
    pub classification: ClassificationResult,

    // Human / AI metadata
    pub tags: Vec<String>,
    pub confidence: f64,

    // Embedding — None until an embedding provider is configured.
    // Stored in DB; not loaded on regular reads. Use semantic_search_ius for ANN queries.
    #[allow(dead_code)]
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,

    // Lifecycle
    pub created_at: i64,
    pub updated_at: i64,
    pub accessed_at: i64,
    pub access_count: i64,
}

impl InformationUnit {
    pub fn new(
        session_id: impl Into<String>,
        title: impl Into<String>,
        content: impl Into<String>,
        direction: TransferDirection,
    ) -> Self {
        let now = crate::domain::session::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            title: title.into(),
            content: content.into(),
            content_type: ContentType::default(),
            transfer_direction: direction,
            source_actor: None,
            target_actor: None,
            classification_status: ClassificationStatus::Pending,
            classification: ClassificationResult::default(),
            tags: Vec::new(),
            confidence: 0.5,
            embedding: None,
            created_at: now,
            updated_at: now,
            accessed_at: now,
            access_count: 0,
        }
    }
}

/// A typed, directed edge between two IUs in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IuEdge {
    pub id: String,
    pub source_iu_id: String,
    pub target_iu_id: String,
    pub edge_type: EdgeType,
    /// Strength 0.0 (weak) → 1.0 (strong).
    pub strength: f64,
    pub description: String,
    /// `true` if this edge was proposed by the system rather than created
    /// explicitly by the human or AI.
    pub inferred: bool,
    pub created_at: i64,
}

impl IuEdge {
    pub fn new(
        source_iu_id: impl Into<String>,
        target_iu_id: impl Into<String>,
        edge_type: EdgeType,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_iu_id: source_iu_id.into(),
            target_iu_id: target_iu_id.into(),
            edge_type,
            strength: 0.5,
            description: String::new(),
            inferred: false,
            created_at: crate::domain::session::now_ms(),
        }
    }
}
