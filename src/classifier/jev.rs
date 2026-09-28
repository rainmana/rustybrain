use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::domain::classification::{
    ClassificationResult, MitreTactic, SemanticType, TransferLossiness,
};

use super::{ClassificationRequest, Classifier};

const JEV_API_URL: &str = "https://api.typesafeai.com/v1/decide";

/// TypeSafe Jev classifier.
///
/// Sends a structured `state` + `questions` payload to the Jev API and maps
/// the typed response back to a [`ClassificationResult`].
///
/// Every IU is classified — there are no unclassified IUs in production.
/// If the Jev call fails, the IU is queued for retry and stored with
/// `classification_status: pending`.
pub struct JevClassifier {
    api_key: String,
    client: Client,
}

impl JevClassifier {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JevRequest {
    state: String,
    questions: Vec<JevQuestion>,
}

#[derive(Serialize)]
struct JevQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    key: &'static str,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    levels: Option<u8>,
}

#[derive(Deserialize, Debug)]
struct JevResponse {
    answers: Vec<JevAnswer>,
}

#[derive(Deserialize, Debug)]
struct JevAnswer {
    key: String,
    // choice answer
    value: Option<serde_json::Value>,
    // noul / score
    probability: Option<f64>,
    score: Option<f64>,
}

// ─── Classifier impl ─────────────────────────────────────────────────────────

#[async_trait]
impl Classifier for JevClassifier {
    #[instrument(skip(self, request), fields(direction = %request.transfer_direction))]
    async fn classify(&self, request: ClassificationRequest) -> anyhow::Result<ClassificationResult> {
        let content_preview = if request.content.len() > 4000 {
            &request.content[..4000]
        } else {
            &request.content
        };

        let state = format!(
            "Title: {}\nContent: {}\nTransfer direction: {}\n{}",
            request.title,
            content_preview,
            request.transfer_direction,
            request
                .session_context
                .as_deref()
                .map(|ctx| format!("Session context: {ctx}"))
                .unwrap_or_default(),
        );

        let questions = vec![
            JevQuestion {
                kind: "choice",
                key: "semantic_type",
                prompt: "What is the primary semantic type of this information unit?".into(),
                options: Some(vec![
                    "observation", "hypothesis", "finding", "vulnerability", "exploit",
                    "payload", "technique", "tool_result", "reference", "constraint",
                    "goal", "assumption", "correction", "clarification", "evidence",
                    "recommendation", "ioc", "ttp", "threat_actor", "campaign",
                    "osint", "humint", "sigint", "geoint", "techint", "finint",
                    "malware_artifact", "binary_artifact", "protocol_artifact",
                    "coordination_signal", "repair_signal", "unknown",
                ]),
                levels: None,
            },
            JevQuestion {
                kind: "score",
                key: "priority",
                prompt: "How operationally critical is this IU to the current task? (0=background, 4=critical)".into(),
                options: None,
                levels: Some(5),
            },
            JevQuestion {
                kind: "noul",
                key: "is_coordination_signal",
                prompt: "Does this IU primarily update the coordination state (goals, constraints, assumptions) rather than add factual content?".into(),
                options: None,
                levels: None,
            },
            JevQuestion {
                kind: "noul",
                key: "is_repair_attempt",
                prompt: "Does this IU represent an attempt to repair a coordination failure (re-grounding, synchronization, disambiguation, decompression, re-weighting)?".into(),
                options: None,
                levels: None,
            },
            JevQuestion {
                kind: "choice",
                key: "transfer_lossiness",
                prompt: "How much meaning is likely lost in this transfer given its type and channel?".into(),
                options: Some(vec!["minimal", "moderate", "high", "severe"]),
                levels: None,
            },
            JevQuestion {
                kind: "noul",
                key: "contains_assumption",
                prompt: "Does this IU introduce or rely on an unstated assumption?".into(),
                options: None,
                levels: None,
            },
            JevQuestion {
                kind: "noul",
                key: "contains_pii",
                prompt: "Does this IU contain personally identifiable information?".into(),
                options: None,
                levels: None,
            },
            JevQuestion {
                kind: "choice",
                key: "mitre_tactic",
                prompt: "If this IU maps to a MITRE ATT&CK tactic, which is most applicable?".into(),
                options: Some(vec![
                    "reconnaissance", "resource_development", "initial_access", "execution",
                    "persistence", "privilege_escalation", "defense_evasion",
                    "credential_access", "discovery", "lateral_movement", "collection",
                    "command_and_control", "exfiltration", "impact", "none",
                ]),
                levels: None,
            },
        ];

        debug!("sending classification request to Jev");

        let payload = JevRequest { state, questions };

        let resp: JevResponse = self
            .client
            .post(JEV_API_URL)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(parse_jev_response(resp))
    }
}

fn parse_jev_response(resp: JevResponse) -> ClassificationResult {
    let mut result = ClassificationResult::default();

    for answer in &resp.answers {
        match answer.key.as_str() {
            "semantic_type" => {
                if let Some(serde_json::Value::String(v)) = &answer.value {
                    result.semantic_type = parse_semantic_type(v);
                }
            }
            "priority" => {
                result.priority = answer.score.map(|s| s.round() as i64);
            }
            "is_coordination_signal" => {
                result.is_coordination_signal = answer.probability;
            }
            "is_repair_attempt" => {
                result.is_repair_attempt = answer.probability;
            }
            "transfer_lossiness" => {
                if let Some(serde_json::Value::String(v)) = &answer.value {
                    result.transfer_lossiness = parse_lossiness(v);
                }
            }
            "contains_assumption" => {
                result.contains_assumption = answer.probability;
            }
            "contains_pii" => {
                result.contains_pii = answer.probability;
            }
            "mitre_tactic" => {
                if let Some(serde_json::Value::String(v)) = &answer.value {
                    result.mitre_tactic = parse_mitre_tactic(v);
                }
            }
            _ => {}
        }
    }

    result
}

fn parse_semantic_type(s: &str) -> Option<SemanticType> {
    Some(match s {
        "observation" => SemanticType::Observation,
        "hypothesis" => SemanticType::Hypothesis,
        "finding" => SemanticType::Finding,
        "vulnerability" => SemanticType::Vulnerability,
        "exploit" => SemanticType::Exploit,
        "payload" => SemanticType::Payload,
        "technique" => SemanticType::Technique,
        "tool_result" => SemanticType::ToolResult,
        "reference" => SemanticType::Reference,
        "constraint" => SemanticType::Constraint,
        "goal" => SemanticType::Goal,
        "assumption" => SemanticType::Assumption,
        "correction" => SemanticType::Correction,
        "clarification" => SemanticType::Clarification,
        "evidence" => SemanticType::Evidence,
        "recommendation" => SemanticType::Recommendation,
        "ioc" => SemanticType::Ioc,
        "ttp" => SemanticType::Ttp,
        "threat_actor" => SemanticType::ThreatActor,
        "campaign" => SemanticType::Campaign,
        "osint" => SemanticType::Osint,
        "humint" => SemanticType::Humint,
        "sigint" => SemanticType::Sigint,
        "geoint" => SemanticType::Geoint,
        "techint" => SemanticType::Techint,
        "finint" => SemanticType::Finint,
        "malware_artifact" => SemanticType::MalwareArtifact,
        "binary_artifact" => SemanticType::BinaryArtifact,
        "protocol_artifact" => SemanticType::ProtocolArtifact,
        "coordination_signal" => SemanticType::CoordinationSignal,
        "repair_signal" => SemanticType::RepairSignal,
        _ => SemanticType::Unknown,
    })
}

fn parse_lossiness(s: &str) -> Option<TransferLossiness> {
    Some(match s {
        "minimal" => TransferLossiness::Minimal,
        "moderate" => TransferLossiness::Moderate,
        "high" => TransferLossiness::High,
        "severe" => TransferLossiness::Severe,
        _ => return None,
    })
}

fn parse_mitre_tactic(s: &str) -> Option<MitreTactic> {
    Some(match s {
        "reconnaissance" => MitreTactic::Reconnaissance,
        "resource_development" => MitreTactic::ResourceDevelopment,
        "initial_access" => MitreTactic::InitialAccess,
        "execution" => MitreTactic::Execution,
        "persistence" => MitreTactic::Persistence,
        "privilege_escalation" => MitreTactic::PrivilegeEscalation,
        "defense_evasion" => MitreTactic::DefenseEvasion,
        "credential_access" => MitreTactic::CredentialAccess,
        "discovery" => MitreTactic::Discovery,
        "lateral_movement" => MitreTactic::LateralMovement,
        "collection" => MitreTactic::Collection,
        "command_and_control" => MitreTactic::CommandAndControl,
        "exfiltration" => MitreTactic::Exfiltration,
        "impact" => MitreTactic::Impact,
        _ => MitreTactic::None,
    })
}
