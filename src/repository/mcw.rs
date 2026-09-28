use anyhow::{Context, Result};
use turso::params;

use crate::db::connection::Db;
use crate::domain::mcw::{
    FailureMode, HealthSignal, InitiatedBy, McwState, RepairRecord, RepairType,
};
use crate::domain::session::now_ms;

pub async fn insert(db: &Db, mcw: &McwState) -> Result<()> {
    let failure_modes = serde_json::to_string(&mcw.active_failure_modes)?;
    let goals = serde_json::to_string(&mcw.goals)?;
    let constraints = serde_json::to_string(&mcw.active_constraints)?;

    db.conn
        .execute(
            "INSERT INTO mcw_state
             (session_id, health_signal, active_failure_modes, last_repair_at,
              last_sync_at, drift_score, compression_depth, constraint_count,
              goals, active_constraints, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                mcw.session_id.as_str(),
                mcw.health_signal.as_str(),
                failure_modes.as_str(),
                mcw.last_repair_at,
                mcw.last_sync_at,
                mcw.drift_score,
                mcw.compression_depth,
                mcw.constraint_count,
                goals.as_str(),
                constraints.as_str(),
                mcw.updated_at
            ],
        )
        .await
        .context("insert mcw_state")?;
    Ok(())
}

pub async fn update(db: &Db, mcw: &McwState) -> Result<()> {
    let failure_modes = serde_json::to_string(&mcw.active_failure_modes)?;
    let goals = serde_json::to_string(&mcw.goals)?;
    let constraints = serde_json::to_string(&mcw.active_constraints)?;

    db.conn
        .execute(
            "UPDATE mcw_state SET
             health_signal=?1, active_failure_modes=?2, last_repair_at=?3,
             last_sync_at=?4, drift_score=?5, compression_depth=?6,
             constraint_count=?7, goals=?8, active_constraints=?9, updated_at=?10
             WHERE session_id=?11",
            params![
                mcw.health_signal.as_str(),
                failure_modes.as_str(),
                mcw.last_repair_at,
                mcw.last_sync_at,
                mcw.drift_score,
                mcw.compression_depth,
                mcw.constraint_count,
                goals.as_str(),
                constraints.as_str(),
                now_ms(),
                mcw.session_id.as_str()
            ],
        )
        .await
        .context("update mcw_state")?;
    Ok(())
}

pub async fn get_by_session(db: &Db, session_id: &str) -> Result<McwState> {
    let mut rows = db
        .conn
        .query(
            "SELECT session_id, health_signal, active_failure_modes,
             last_repair_at, last_sync_at, drift_score, compression_depth,
             constraint_count, goals, active_constraints, updated_at
             FROM mcw_state WHERE session_id = ?1",
            params![session_id],
        )
        .await
        .context("get mcw_state")?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("mcw_state not found for session: {session_id}"))?;

    row_to_mcw(&row)
}

pub async fn insert_repair_record(db: &Db, r: &RepairRecord) -> Result<()> {
    let iu_ids = serde_json::to_string(&r.iu_ids)?;
    let failure_mode: Option<&str> = r.failure_mode.as_ref().map(|f| f.as_str());

    db.conn
        .execute(
            "INSERT INTO repair_log
             (id, session_id, repair_type, failure_mode, description,
              iu_ids, before_state, after_state, initiated_by, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                r.id.as_str(),
                r.session_id.as_str(),
                r.repair_type.as_str(),
                failure_mode,
                r.description.as_str(),
                iu_ids.as_str(),
                r.before_state.as_deref(),
                r.after_state.as_deref(),
                r.initiated_by.as_str(),
                r.created_at
            ],
        )
        .await
        .context("insert repair_log")?;
    Ok(())
}

pub async fn list_repair_records(
    db: &Db,
    session_id: &str,
    limit: usize,
) -> Result<Vec<RepairRecord>> {
    let mut rows = db
        .conn
        .query(
            "SELECT id, session_id, repair_type, failure_mode, description,
             iu_ids, before_state, after_state, initiated_by, created_at
             FROM repair_log WHERE session_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
            params![session_id, limit as i64],
        )
        .await
        .context("list repair_log")?;

    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push(row_to_repair(&row)?);
    }
    Ok(records)
}

pub async fn count_repairs(db: &Db, session_id: &str) -> Result<i64> {
    let mut rows = db
        .conn
        .query(
            "SELECT COUNT(*) FROM repair_log WHERE session_id = ?1",
            params![session_id],
        )
        .await?;
    let row = rows.next().await?.ok_or_else(|| anyhow::anyhow!("no rows"))?;
    Ok(row.get(0)?)
}

// ─── Row helpers ─────────────────────────────────────────────────────────────

fn row_to_mcw(row: &turso::Row) -> Result<McwState> {
    let failure_modes: Vec<FailureMode> =
        serde_json::from_str(&row.get::<String>(2)?).unwrap_or_default();
    let goals: Vec<String> =
        serde_json::from_str(&row.get::<String>(8)?).unwrap_or_default();
    let constraints: Vec<String> =
        serde_json::from_str(&row.get::<String>(9)?).unwrap_or_default();

    let health_signal = match row.get::<String>(1)?.as_str() {
        "drift_detected" => HealthSignal::DriftDetected,
        "asa_detected" => HealthSignal::AsaDetected,
        "false_alignment" => HealthSignal::FalseAlignment,
        "overcompressed" => HealthSignal::Overcompressed,
        "constraint_opaque" => HealthSignal::ConstraintOpaque,
        "repair_suppressed" => HealthSignal::RepairSuppressed,
        _ => HealthSignal::Nominal,
    };

    Ok(McwState {
        session_id: row.get(0)?,
        health_signal,
        active_failure_modes: failure_modes,
        last_repair_at: row.get(3)?,
        last_sync_at: row.get(4)?,
        drift_score: row.get(5)?,
        compression_depth: row.get(6)?,
        constraint_count: row.get(7)?,
        goals,
        active_constraints: constraints,
        updated_at: row.get(10)?,
    })
}

fn row_to_repair(row: &turso::Row) -> Result<RepairRecord> {
    let repair_type = match row.get::<String>(2)?.as_str() {
        "decompression" => RepairType::Decompression,
        "reweighting" => RepairType::Reweighting,
        "disambiguation" => RepairType::Disambiguation,
        "synchronization" => RepairType::Synchronization,
        _ => RepairType::ReGrounding,
    };

    let failure_mode: Option<FailureMode> = row
        .get::<Option<String>>(3)?
        .as_deref()
        .and_then(|s| match s {
            "drift" => Some(FailureMode::Drift),
            "asymmetric_state_advancement" => Some(FailureMode::AsymmetricStateAdvancement),
            "false_alignment" => Some(FailureMode::FalseAlignment),
            "overcompression" => Some(FailureMode::Overcompression),
            "constraint_opacity" => Some(FailureMode::ConstraintOpacity),
            "repair_suppression" => Some(FailureMode::RepairSuppression),
            _ => None,
        });

    let iu_ids: Vec<String> =
        serde_json::from_str(&row.get::<String>(5)?).unwrap_or_default();

    let initiated_by = match row.get::<String>(8)?.as_str() {
        "ai" => InitiatedBy::Ai,
        "system" => InitiatedBy::System,
        _ => InitiatedBy::Human,
    };

    Ok(RepairRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        repair_type,
        failure_mode,
        description: row.get(4)?,
        iu_ids,
        before_state: row.get(6)?,
        after_state: row.get(7)?,
        initiated_by,
        created_at: row.get(9)?,
    })
}
