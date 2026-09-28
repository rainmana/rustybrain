use anyhow::{bail, Result};
use serde_json::{json, Value};
use tracing::info;

use crate::app::AppState;
use crate::domain::mcw::McwState;
use crate::domain::session::{now_ms, Session, SessionStatus, TaskType};
use crate::repository;

pub async fn create_session(args: Value, state: &AppState) -> Result<Value> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("name is required"))?
        .to_owned();

    let task_type_str = args["task_type"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("task_type is required"))?;

    let task_type: TaskType = task_type_str
        .parse()
        .map_err(|_| anyhow::anyhow!("unknown task_type: {task_type_str}"))?;

    let description = args["description"]
        .as_str()
        .unwrap_or("")
        .to_owned();

    let mut session = Session::new(name, task_type);
    session.description = description;

    repository::session::insert(&state.db, &session).await?;

    // Initialise MCW state for the new session.
    let mcw = McwState::new(&session.id);
    repository::mcw::insert(&state.db, &mcw).await?;

    info!(session_id = %session.id, "session created");
    Ok(json!(session))
}

pub async fn get_session(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let session = repository::session::get_by_id(&state.db, session_id).await?;
    let mcw = repository::mcw::get_by_session(&state.db, session_id).await?;

    Ok(json!({ "session": session, "mcw_state": mcw }))
}

pub async fn list_sessions(args: Value, state: &AppState) -> Result<Value> {
    let task_type = args["task_type"].as_str();
    let status = args["status"].as_str();
    let limit = args["limit"].as_u64().unwrap_or(50) as usize;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;

    let sessions =
        repository::session::list(&state.db, task_type, status, limit, offset).await?;

    Ok(json!({ "sessions": sessions, "count": sessions.len() }))
}

pub async fn update_session_status(
    state: &AppState,
    session_id: &str,
    status: SessionStatus,
) -> Result<()> {
    let now = now_ms();
    state
        .db
        .conn
        .execute(
            "UPDATE sessions SET status = ?, updated_at = ? WHERE id = ?",
            (status.as_str(), now, session_id),
        )
        .await?;
    Ok(())
}

pub async fn _update_session(args: Value, state: &AppState) -> Result<Value> {
    let session_id = args["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("session_id is required"))?;

    let status_str = args["status"].as_str();

    if let Some(s) = status_str {
        let status: SessionStatus = match s {
            "active" => SessionStatus::Active,
            "paused" => SessionStatus::Paused,
            "completed" => SessionStatus::Completed,
            "archived" => SessionStatus::Archived,
            other => bail!("unknown status: {other}"),
        };
        update_session_status(state, session_id, status).await?;
    }

    let session = repository::session::get_by_id(&state.db, session_id).await?;
    Ok(json!(session))
}
