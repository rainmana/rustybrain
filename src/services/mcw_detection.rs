/// MCW failure mode detection heuristics.
///
/// These are initial falsifiable starting points — they will be revised as
/// real usage data accumulates. See the spec §9.1 for the full rationale.
use anyhow::Result;

use crate::db::connection::Db;
use crate::domain::mcw::{FailureMode, McwState};
use crate::repository;

/// Run all detection heuristics against the current session state.
/// Returns the list of failure modes detected.
pub async fn run_detection(db: &Db, mcw: &McwState) -> Result<Vec<FailureMode>> {
    let mut detected = Vec::new();

    // ── Heuristic: Drift ─────────────────────────────────────────────────────
    // Signal: drift_score has accumulated above threshold.
    if mcw.drift_score > 0.6 {
        detected.push(FailureMode::Drift);
    }

    // ── Heuristic: Asymmetric State Advancement ───────────────────────────────
    // Signal: no Synchronization repair has occurred and session is mature
    // (has IUs stored but last_sync_at is None).
    let iu_count =
        repository::iu::count_for_session(db, &mcw.session_id).await?;
    if iu_count > 10 && mcw.last_sync_at.is_none() {
        detected.push(FailureMode::AsymmetricStateAdvancement);
    }

    // ── Heuristic: Overcompression ───────────────────────────────────────────
    // Signal: compression_depth has grown without decompression.
    if mcw.compression_depth >= 3 {
        detected.push(FailureMode::Overcompression);
    }

    // ── Heuristic: Constraint Opacity ────────────────────────────────────────
    // Signal: repeated high-assumption IUs without clarification.
    if mcw.constraint_count >= 5 {
        detected.push(FailureMode::ConstraintOpacity);
    }

    // False Alignment and Repair Suppression require richer signal data
    // (embedding drift, question IU frequency) — stubbed for post-MVP.

    Ok(detected)
}
