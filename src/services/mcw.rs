use anyhow::Result;
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::mcw::{InitiatedBy, RepairRecord, RepairType};
use crate::domain::session::now_ms;
use crate::repository;

pub async fn get_mcw_state(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let mcw = repository::mcw::get_by_session(&state.db, session_id).await?;
    Ok(json!(mcw))
}

pub async fn detect_failure_modes(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let mut mcw = repository::mcw::get_by_session(&state.db, session_id).await?;
    let detected = crate::services::mcw_detection::run_detection(&state.db, &mcw).await?;

    mcw.active_failure_modes = detected.clone();
    if !detected.is_empty() {
        mcw.health_signal = crate::domain::mcw::HealthSignal::DriftDetected; // first signal wins for now
    }
    mcw.updated_at = now_ms();

    repository::mcw::update(&state.db, &mcw).await?;

    Ok(json!({
        "session_id": session_id,
        "detected_failure_modes": detected.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
        "health_signal": mcw.health_signal.as_str()
    }))
}

pub async fn repair(args: Value, state: &AppState, repair_type: RepairType) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?
        .to_owned();
    let description = args["description"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("description is required"))?
        .to_owned();

    let iu_ids: Vec<String> = args["iu_ids"]
        .as_str()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let mut mcw = repository::mcw::get_by_session(&state.db, &session_id).await?;

    // Snapshot before state.
    let before = serde_json::to_string(&mcw).ok();

    // Apply repair effects to MCW state.
    let now = now_ms();
    mcw.active_failure_modes.clear();
    mcw.health_signal = crate::domain::mcw::HealthSignal::Nominal;
    mcw.last_repair_at = Some(now);

    match repair_type {
        RepairType::Synchronization => {
            mcw.last_sync_at = Some(now);
            mcw.drift_score = (mcw.drift_score - 0.3).max(0.0);
        }
        RepairType::Decompression => {
            mcw.compression_depth = (mcw.compression_depth - 1).max(0);
        }
        RepairType::ReGrounding => {
            mcw.drift_score = (mcw.drift_score - 0.2).max(0.0);
        }
        RepairType::Reweighting | RepairType::Disambiguation => {
            mcw.constraint_count = (mcw.constraint_count - 1).max(0);
        }
    }
    mcw.updated_at = now;

    let after = serde_json::to_string(&mcw).ok();

    repository::mcw::update(&state.db, &mcw).await?;

    // Write repair log record.
    let record = RepairRecord {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        repair_type: repair_type.clone(),
        failure_mode: None,
        description: description.clone(),
        iu_ids: iu_ids.clone(),
        before_state: before,
        after_state: after,
        initiated_by: InitiatedBy::Human,
        created_at: now,
    };
    repository::mcw::insert_repair_record(&state.db, &record).await?;

    info!(session_id = %session_id, repair = %repair_type, "repair executed");

    Ok(json!({
        "session_id": session_id,
        "repair_type": repair_type.as_str(),
        "description": description,
        "mcw_state": mcw
    }))
}

pub async fn get_repair_history(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;

    let records = repository::mcw::list_repair_records(&state.db, session_id, limit).await?;
    Ok(json!({ "records": records, "count": records.len() }))
}

pub async fn get_context_summary(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;
    let current_task = args["current_task"].as_str().unwrap_or("(not specified)");

    let mcw = repository::mcw::get_by_session(&state.db, session_id).await?;
    let session = repository::session::get_by_id(&state.db, session_id).await?;

    // Pull recent high-priority IUs for the summary.
    let recent_ius =
        repository::iu::list_recent_high_priority(&state.db, session_id, 10).await?;

    let repair_count = repository::mcw::count_repairs(&state.db, session_id).await?;

    Ok(json!({
        "session": session,
        "current_task": current_task,
        "mcw_health": mcw.health_signal.as_str(),
        "active_failure_modes": mcw.active_failure_modes.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
        "drift_score": mcw.drift_score,
        "last_sync_at": mcw.last_sync_at,
        "goals": mcw.goals,
        "active_constraints": mcw.active_constraints,
        "repair_count": repair_count,
        "recent_high_priority_ius": recent_ius,
        "sync_recommended": mcw.last_sync_at.is_none()
            || (now_ms() - mcw.last_sync_at.unwrap_or(0)) > 3_600_000
    }))
}
