use anyhow::{Context, Result};
use std::collections::HashMap;
use turso::{params, Value};

use crate::db::connection::Db;
use crate::domain::classification::{
    ClassificationResult, ClassificationStatus, MitreTactic, SemanticType, TransferLossiness,
};
use crate::domain::iu::{ContentType, InformationUnit, IuEdge, TransferDirection};
use crate::domain::session::now_ms;

const IU_COLS: &str = "id, session_id, title, content, content_type,
    transfer_direction, source_actor, target_actor,
    classification_status, semantic_type, priority,
    is_coordination_signal, is_repair_attempt, transfer_lossiness,
    contains_assumption, contains_pii, mitre_tactic,
    tags, confidence, created_at, updated_at, accessed_at, access_count";

// ─── Insert ──────────────────────────────────────────────────────────────────

pub async fn insert(db: &Db, iu: &InformationUnit) -> Result<()> {
    let tags = serde_json::to_string(&iu.tags)?;
    db.conn
        .execute(
            &format!(
                "INSERT INTO information_units ({IU_COLS})
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)"
            ),
            params![
                iu.id.as_str(),
                iu.session_id.as_str(),
                iu.title.as_str(),
                iu.content.as_str(),
                iu.content_type.as_str(),
                iu.transfer_direction.as_str(),
                iu.source_actor.as_deref(),
                iu.target_actor.as_deref(),
                iu.classification_status.as_str(),
                Option::<&str>::None, // semantic_type
                Option::<i64>::None,  // priority
                Option::<f64>::None,  // is_coordination_signal
                Option::<f64>::None,  // is_repair_attempt
                Option::<&str>::None, // transfer_lossiness
                Option::<f64>::None,  // contains_assumption
                Option::<f64>::None,  // contains_pii
                Option::<&str>::None, // mitre_tactic
                tags.as_str(),
                iu.confidence,
                iu.created_at,
                iu.updated_at,
                iu.accessed_at,
                iu.access_count
            ],
        )
        .await
        .context("insert information_unit")?;
    Ok(())
}

pub async fn update_classification(
    db: &Db,
    iu_id: &str,
    result: &ClassificationResult,
) -> Result<()> {
    let semantic_type: Option<&str> = result.semantic_type.as_ref().map(|s| s.as_str());
    let lossiness: Option<&str> = result.transfer_lossiness.as_ref().map(|l| l.as_str());
    let tactic: Option<&str> = result.mitre_tactic.as_ref().map(|t| t.as_str());

    db.conn
        .execute(
            "UPDATE information_units SET
             classification_status=?1, semantic_type=?2, priority=?3,
             is_coordination_signal=?4, is_repair_attempt=?5,
             transfer_lossiness=?6, contains_assumption=?7, contains_pii=?8,
             mitre_tactic=?9, updated_at=?10
             WHERE id=?11",
            params![
                ClassificationStatus::Classified.as_str(),
                semantic_type,
                result.priority,
                result.is_coordination_signal,
                result.is_repair_attempt,
                lossiness,
                result.contains_assumption,
                result.contains_pii,
                tactic,
                now_ms(),
                iu_id
            ],
        )
        .await
        .context("update classification")?;

    db.conn
        .execute(
            "DELETE FROM classification_queue WHERE iu_id=?1",
            params![iu_id],
        )
        .await?;

    Ok(())
}

pub async fn enqueue_classification(db: &Db, iu_id: &str, error: &str) -> Result<()> {
    let now = now_ms();
    db.conn
        .execute(
            "INSERT INTO classification_queue (iu_id, attempts, last_attempt, last_error, created_at)
             VALUES (?1, 1, ?2, ?3, ?4)
             ON CONFLICT(iu_id) DO UPDATE SET
               attempts=attempts+1, last_attempt=excluded.last_attempt,
               last_error=excluded.last_error",
            params![iu_id, now, error, now],
        )
        .await
        .context("enqueue classification")?;
    Ok(())
}

// ─── Get ─────────────────────────────────────────────────────────────────────

pub async fn get_by_id(db: &Db, id: &str) -> Result<InformationUnit> {
    db.conn
        .execute(
            "UPDATE information_units SET access_count=access_count+1, accessed_at=?1 WHERE id=?2",
            params![now_ms(), id],
        )
        .await?;

    let mut rows = db
        .conn
        .query(
            &format!("SELECT {IU_COLS} FROM information_units WHERE id=?1"),
            params![id],
        )
        .await
        .context("get iu by id")?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("IU not found: {id}"))?;

    row_to_iu(&row)
}

// ─── Search ──────────────────────────────────────────────────────────────────

pub async fn search_fts(
    db: &Db,
    query: &str,
    session_id: Option<&str>,
    semantic_type: Option<&str>,
    min_priority: Option<i64>,
    limit: usize,
    offset: usize,
) -> Result<Vec<InformationUnit>> {
    let mut sql = format!(
        "SELECT {IU_COLS} FROM information_units
         WHERE rowid IN (SELECT rowid FROM iu_fts WHERE iu_fts MATCH ?1)"
    );
    let mut vals: Vec<Value> = vec![Value::Text(sanitize_fts_query(query))];
    let mut idx = 2usize;

    if let Some(sid) = session_id {
        sql.push_str(&format!(" AND session_id=?{idx}"));
        vals.push(Value::Text(sid.to_owned()));
        idx += 1;
    }
    if let Some(st) = semantic_type {
        sql.push_str(&format!(" AND semantic_type=?{idx}"));
        vals.push(Value::Text(st.to_owned()));
        idx += 1;
    }
    if let Some(mp) = min_priority {
        sql.push_str(&format!(" AND priority>=?{idx}"));
        vals.push(Value::Integer(mp));
        idx += 1;
    }
    sql.push_str(&format!(
        " ORDER BY priority DESC, confidence DESC, created_at DESC LIMIT ?{idx} OFFSET ?{}",
        idx + 1
    ));
    vals.push(Value::Integer(limit as i64));
    vals.push(Value::Integer(offset as i64));

    let mut rows = db
        .conn
        .query(&sql, turso::params::Params::Positional(vals))
        .await
        .context("search_fts")?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push(row_to_iu(&row)?);
    }
    Ok(results)
}

pub async fn search_vector(
    db: &Db,
    query_vec: &[f32],
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<InformationUnit>> {
    let vec_str = format!(
        "[{}]",
        query_vec
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // vector_top_k returns rowids; join back to get full IU columns.
    let base_sql = format!(
        "SELECT iu.{} FROM vector_top_k('idx_iu_embedding', vector32(?1), {limit})
         JOIN information_units iu ON iu.rowid = id",
        IU_COLS.replace('\n', "").replace("  ", " ")
    );

    let sql = if let Some(_sid) = session_id {
        format!("{base_sql} WHERE iu.session_id=?2")
    } else {
        base_sql
    };

    let mut vals = vec![Value::Text(vec_str)];
    if session_id.is_some() {
        vals.push(Value::Text(session_id.unwrap().to_owned()));
    }

    let mut rows = db
        .conn
        .query(&sql, turso::params::Params::Positional(vals))
        .await
        .context("search_vector")?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push(row_to_iu(&row)?);
    }
    Ok(results)
}

// ─── Utility queries ─────────────────────────────────────────────────────────

pub async fn count_for_session(db: &Db, session_id: &str) -> Result<i64> {
    let mut rows = db
        .conn
        .query(
            "SELECT COUNT(*) FROM information_units WHERE session_id=?1",
            params![session_id],
        )
        .await?;
    let row = rows.next().await?.ok_or_else(|| anyhow::anyhow!("no rows"))?;
    Ok(row.get(0)?)
}

pub async fn list_ids_for_session(db: &Db, session_id: &str) -> Result<Vec<String>> {
    let mut rows = db
        .conn
        .query(
            "SELECT id FROM information_units WHERE session_id=?1",
            params![session_id],
        )
        .await?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await? {
        ids.push(row.get::<String>(0)?);
    }
    Ok(ids)
}

pub async fn list_recent_high_priority(
    db: &Db,
    session_id: &str,
    limit: usize,
) -> Result<Vec<InformationUnit>> {
    let mut rows = db
        .conn
        .query(
            &format!(
                "SELECT {IU_COLS} FROM information_units WHERE session_id=?1
                 ORDER BY priority DESC, created_at DESC LIMIT ?2"
            ),
            params![session_id, limit as i64],
        )
        .await?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push(row_to_iu(&row)?);
    }
    Ok(results)
}

pub async fn group_by_tactic(
    db: &Db,
    iu_ids: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    if iu_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Build IN clause with positional params.
    let placeholders: String = (1..=iu_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, mitre_tactic FROM information_units
         WHERE id IN ({placeholders}) AND mitre_tactic IS NOT NULL AND mitre_tactic != 'none'"
    );
    let vals: Vec<Value> = iu_ids
        .iter()
        .map(|s| Value::Text(s.clone()))
        .collect();

    let mut rows = db
        .conn
        .query(&sql, turso::params::Params::Positional(vals))
        .await?;

    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let tactic: String = row.get(1)?;
        groups.entry(tactic).or_default().push(id);
    }
    groups.retain(|_, v| v.len() >= 2);
    Ok(groups)
}

// ─── Edges ───────────────────────────────────────────────────────────────────

pub async fn insert_edge(db: &Db, edge: &IuEdge) -> Result<()> {
    db.conn
        .execute(
            "INSERT INTO iu_edges (id, source_iu_id, target_iu_id, edge_type, strength, description, inferred, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                edge.id.as_str(),
                edge.source_iu_id.as_str(),
                edge.target_iu_id.as_str(),
                edge.edge_type.as_str(),
                edge.strength,
                edge.description.as_str(),
                edge.inferred as i64,
                edge.created_at
            ],
        )
        .await
        .context("insert iu_edge")?;
    Ok(())
}

// ─── Row → domain ────────────────────────────────────────────────────────────

fn row_to_iu(row: &turso::Row) -> Result<InformationUnit> {
    let content_type = match row.get::<String>(4)?.as_str() {
        "code" => ContentType::Code,
        "json" => ContentType::Json,
        "yaml" => ContentType::Yaml,
        "markdown" => ContentType::Markdown,
        "binary_ref" => ContentType::BinaryRef,
        "structured" => ContentType::Structured,
        _ => ContentType::Text,
    };

    let direction = match row.get::<String>(5)?.as_str() {
        "human→ai" => TransferDirection::HumanToAi,
        "ai→human" => TransferDirection::AiToHuman,
        "ai→subagent" => TransferDirection::AiToSubagent,
        "subagent→ai" => TransferDirection::SubagentToAi,
        "rag→ai" => TransferDirection::RagToAi,
        "ai→rag" => TransferDirection::AiToRag,
        "tool→ai" => TransferDirection::ToolToAi,
        "ai→tool" => TransferDirection::AiToTool,
        "human→human" => TransferDirection::HumanToHuman,
        _ => TransferDirection::Unknown,
    };

    let classification_status = match row.get::<String>(8)?.as_str() {
        "classified" => ClassificationStatus::Classified,
        "failed" => ClassificationStatus::Failed,
        _ => ClassificationStatus::Pending,
    };

    let semantic_type = row
        .get::<Option<String>>(9)?
        .as_deref()
        .and_then(parse_semantic_type_str);

    let transfer_lossiness = row
        .get::<Option<String>>(13)?
        .as_deref()
        .and_then(|s| match s {
            "minimal" => Some(TransferLossiness::Minimal),
            "moderate" => Some(TransferLossiness::Moderate),
            "high" => Some(TransferLossiness::High),
            "severe" => Some(TransferLossiness::Severe),
            _ => None,
        });

    let mitre_tactic = row
        .get::<Option<String>>(16)?
        .as_deref()
        .and_then(parse_mitre_str);

    let tags: Vec<String> =
        serde_json::from_str(&row.get::<String>(17)?).unwrap_or_default();

    Ok(InformationUnit {
        id: row.get(0)?,
        session_id: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        content_type,
        transfer_direction: direction,
        source_actor: row.get(6)?,
        target_actor: row.get(7)?,
        classification_status,
        classification: ClassificationResult {
            semantic_type,
            priority: row.get(10)?,
            is_coordination_signal: row.get(11)?,
            is_repair_attempt: row.get(12)?,
            transfer_lossiness,
            contains_assumption: row.get(14)?,
            contains_pii: row.get(15)?,
            mitre_tactic,
        },
        tags,
        confidence: row.get(18)?,
        embedding: None,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        accessed_at: row.get(21)?,
        access_count: row.get(22)?,
    })
}

fn sanitize_fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn parse_semantic_type_str(s: &str) -> Option<SemanticType> {
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

fn parse_mitre_str(s: &str) -> Option<MitreTactic> {
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
