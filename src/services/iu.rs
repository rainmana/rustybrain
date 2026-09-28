use anyhow::Result;
use serde_json::{json, Value};
use tracing::info;

use crate::app::AppState;
use crate::classifier::ClassificationRequest;
use crate::domain::iu::{ContentType, EdgeType, InformationUnit, IuEdge, TransferDirection};
use crate::repository;

pub async fn store_iu(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?
        .to_owned();
    let title = args["title"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("title is required"))?
        .to_owned();
    let content = args["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("content is required"))?
        .to_owned();

    let direction = parse_direction(args["transfer_direction"].as_str().unwrap_or("unknown"));

    let mut iu = InformationUnit::new(&session_id, &title, &content, direction);

    if let Some(ct) = args["content_type"].as_str() {
        iu.content_type = parse_content_type(ct);
    }
    if let Some(src) = args["source_actor"].as_str() {
        iu.source_actor = Some(src.to_owned());
    }
    if let Some(tgt) = args["target_actor"].as_str() {
        iu.target_actor = Some(tgt.to_owned());
    }
    if let Some(conf) = args["confidence"].as_f64() {
        iu.confidence = conf.clamp(0.0, 1.0);
    }
    if let Some(tags_str) = args["tags"].as_str() {
        iu.tags = serde_json::from_str(tags_str).unwrap_or_default();
    }

    // Write to DB with classification_status=pending.
    repository::iu::insert(&state.db, &iu).await?;
    info!(iu_id = %iu.id, "IU stored");

    let wait = args["wait_for_classification"].as_bool().unwrap_or(false);

    if wait {
        // Synchronous path: await Jev, update IU, return classified result.
        let req = build_classification_request(&iu);
        match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.classifier.classify(req),
        )
        .await
        {
            Ok(Ok(result)) => {
                repository::iu::update_classification(&state.db, &iu.id, &result).await?;
                let updated = repository::iu::get_by_id(&state.db, &iu.id).await?;
                return Ok(json!(updated));
            }
            Ok(Err(e)) => {
                // Jev failed — enqueue for retry, return pending IU.
                repository::iu::enqueue_classification(&state.db, &iu.id, &e.to_string())
                    .await?;
            }
            Err(_) => {
                // Timeout — enqueue for retry.
                repository::iu::enqueue_classification(&state.db, &iu.id, "timeout").await?;
            }
        }
    } else {
        // Async path: fire-and-forget, return immediately.
        let iu_id = iu.id.clone();
        let classifier = state.classifier.clone();
        let bg_db = state.db.clone();
        let req = build_classification_request(&iu);

        tokio::spawn(async move {
            match classifier.classify(req).await {
                Ok(result) => {
                    let conn = bg_db.new_connection().expect("bg connection");
                    let mini_db = crate::db::connection::Db { conn, db: bg_db.db.clone() };
                    let _ = repository::iu::update_classification(&mini_db, &iu_id, &result).await;
                }
                Err(e) => {
                    let conn = bg_db.new_connection().expect("bg connection");
                    let mini_db = crate::db::connection::Db { conn, db: bg_db.db.clone() };
                    let _ = repository::iu::enqueue_classification(&mini_db, &iu_id, &e.to_string()).await;
                }
            }
        });
    }

    Ok(json!(iu))
}

pub async fn get_iu(args: Value, state: &AppState) -> Result<Value> {
    let iu_id = args["iu_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("iu_id is required"))?;
    let iu = repository::iu::get_by_id(&state.db, iu_id).await?;
    Ok(json!(iu))
}

pub async fn search_ius(args: Value, state: &AppState) -> Result<Value> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let session_id = args["session_id"].as_str();
    let semantic_type = args["semantic_type"].as_str();
    let min_priority = args["min_priority"].as_i64();
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;

    let results = repository::iu::search_fts(
        &state.db,
        query,
        session_id,
        semantic_type,
        min_priority,
        limit,
        offset,
    )
    .await?;

    Ok(json!({ "results": results, "count": results.len() }))
}

pub async fn semantic_search_ius(args: Value, state: &AppState) -> Result<Value> {
    let embedder = match &state.embedder {
        Some(e) => e.clone(),
        None => anyhow::bail!(
            "no embedding provider configured; set RUSTYBRAIN_EMBED_PROVIDER and related env vars"
        ),
    };

    let query = args["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("query is required"))?;
    let session_id = args["session_id"].as_str();
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;

    let query_vec = embedder.embed_one(query).await?;

    let results =
        repository::iu::search_vector(&state.db, &query_vec, session_id, limit).await?;

    Ok(json!({ "results": results, "count": results.len() }))
}

pub async fn create_edge(args: Value, state: &AppState) -> Result<Value> {
    let source = args["source_iu_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("source_iu_id is required"))?;
    let target = args["target_iu_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("target_iu_id is required"))?;
    let edge_type_str = args["edge_type"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("edge_type is required"))?;

    let edge_type = parse_edge_type(edge_type_str)
        .ok_or_else(|| anyhow::anyhow!("unknown edge_type: {edge_type_str}"))?;

    let mut edge = IuEdge::new(source, target, edge_type);

    if let Some(s) = args["strength"].as_f64() {
        edge.strength = s.clamp(0.0, 1.0);
    }
    if let Some(d) = args["description"].as_str() {
        edge.description = d.to_owned();
    }

    repository::iu::insert_edge(&state.db, &edge).await?;
    Ok(json!(edge))
}

pub async fn correlate_ius(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;

    // Collect candidate IU IDs: either the explicit list or all in the session.
    let candidate_ids: Vec<String> = if let Some(ids_str) = args["iu_ids"].as_str() {
        serde_json::from_str(ids_str).unwrap_or_default()
    } else {
        repository::iu::list_ids_for_session(&state.db, session_id).await?
    };

    if candidate_ids.len() < 2 {
        return Ok(json!({ "proposed_edges": [], "note": "need at least 2 IUs to correlate" }));
    }

    // Load classification data for correlation heuristics.
    let mut proposed: Vec<Value> = Vec::new();

    // Heuristic 1: shared MITRE tactic across different semantic types.
    let tactic_groups =
        repository::iu::group_by_tactic(&state.db, &candidate_ids).await?;
    for (tactic, ids) in &tactic_groups {
        if ids.len() < 2 || tactic == "none" {
            continue;
        }
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                proposed.push(json!({
                    "source_iu_id": ids[i],
                    "target_iu_id": ids[j],
                    "suggested_edge_type": "related_to",
                    "strength": 0.6,
                    "reason": format!("shared MITRE tactic: {tactic}"),
                    "inferred": true
                }));
            }
        }
    }

    // Heuristic 2: vector similarity (if embedder is configured).
    if let Some(embedder) = &state.embedder {
        let _ = embedder; // placeholder — full ANN sweep post-MVP
    }

    proposed.truncate(limit);

    let auto_create = args["auto_create"].as_bool().unwrap_or(false);
    if auto_create {
        for p in &proposed {
            if let (Some(src), Some(tgt), Some(et_str)) = (
                p["source_iu_id"].as_str(),
                p["target_iu_id"].as_str(),
                p["suggested_edge_type"].as_str(),
            ) {
                if let Some(edge_type) = parse_edge_type(et_str) {
                    let mut edge = IuEdge::new(src, tgt, edge_type);
                    edge.strength = p["strength"].as_f64().unwrap_or(0.5);
                    edge.description = p["reason"]
                        .as_str()
                        .unwrap_or("auto-correlated")
                        .to_owned();
                    edge.inferred = true;
                    let _ = repository::iu::insert_edge(&state.db, &edge).await;
                }
            }
        }
    }

    Ok(json!({
        "proposed_edges": proposed,
        "count": proposed.len(),
        "auto_created": auto_create
    }))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_classification_request(iu: &InformationUnit) -> ClassificationRequest {
    ClassificationRequest {
        title: iu.title.clone(),
        content: iu.content.clone(),
        transfer_direction: iu.transfer_direction.to_string(),
        session_context: None,
    }
}

fn parse_direction(s: &str) -> TransferDirection {
    match s {
        "human→ai" | "human_to_ai" => TransferDirection::HumanToAi,
        "ai→human" | "ai_to_human" => TransferDirection::AiToHuman,
        "ai→subagent" | "ai_to_subagent" => TransferDirection::AiToSubagent,
        "subagent→ai" | "subagent_to_ai" => TransferDirection::SubagentToAi,
        "rag→ai" | "rag_to_ai" => TransferDirection::RagToAi,
        "ai→rag" | "ai_to_rag" => TransferDirection::AiToRag,
        "tool→ai" | "tool_to_ai" => TransferDirection::ToolToAi,
        "ai→tool" | "ai_to_tool" => TransferDirection::AiToTool,
        "human→human" | "human_to_human" => TransferDirection::HumanToHuman,
        _ => TransferDirection::Unknown,
    }
}

fn parse_content_type(s: &str) -> ContentType {
    match s {
        "code" => ContentType::Code,
        "json" => ContentType::Json,
        "yaml" => ContentType::Yaml,
        "markdown" => ContentType::Markdown,
        "binary_ref" => ContentType::BinaryRef,
        "structured" => ContentType::Structured,
        _ => ContentType::Text,
    }
}

fn parse_edge_type(s: &str) -> Option<EdgeType> {
    Some(match s {
        "depends_on" => EdgeType::DependsOn,
        "causes" => EdgeType::Causes,
        "mitigates" => EdgeType::Mitigates,
        "exploits" => EdgeType::Exploits,
        "references" => EdgeType::References,
        "contradicts" => EdgeType::Contradicts,
        "supports" => EdgeType::Supports,
        "related_to" => EdgeType::RelatedTo,
        "parent_of" => EdgeType::ParentOf,
        "child_of" => EdgeType::ChildOf,
        "repairs" => EdgeType::Repairs,
        "precedes" => EdgeType::Precedes,
        "supersedes" => EdgeType::Supersedes,
        "corroborates" => EdgeType::Corroborates,
        "refutes" => EdgeType::Refutes,
        "instantiates" => EdgeType::Instantiates,
        "exemplifies" => EdgeType::Exemplifies,
        "abstracts" => EdgeType::Abstracts,
        _ => return None,
    })
}
