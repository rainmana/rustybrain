use anyhow::Result;
use serde_json::{json, Value};

use crate::app::AppState;

pub async fn health_check(state: &AppState) -> Result<Value> {
    // Ping DB with a lightweight query.
    let db_ok = state
        .db
        .conn
        .query("SELECT 1", ())
        .await
        .is_ok();

    // Classifier is always present (may be mock in tests).
    let classifier_name = "jev";

    // Embedder is optional.
    let embed_status = match &state.embedder {
        Some(e) => json!({ "configured": true, "provider": e.name(), "dimensions": e.dimensions() }),
        None => json!({ "configured": false, "note": "vector search disabled; set RUSTYBRAIN_EMBED_PROVIDER" }),
    };

    Ok(json!({
        "status": if db_ok { "healthy" } else { "degraded" },
        "version": state.version,
        "database": if db_ok { "ok" } else { "error" },
        "classifier": classifier_name,
        "embedder": embed_status
    }))
}

pub async fn get_stats(state: &AppState) -> Result<Value> {
    let mut rows = state
        .db
        .conn
        .query(
            "SELECT
               (SELECT COUNT(*) FROM sessions)              AS sessions,
               (SELECT COUNT(*) FROM information_units)     AS ius,
               (SELECT COUNT(*) FROM iu_edges)              AS edges,
               (SELECT COUNT(*) FROM repair_log)            AS repairs,
               (SELECT COUNT(*) FROM classification_queue)  AS pending_classifications",
            (),
        )
        .await?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("stats query returned no rows"))?;

    Ok(json!({
        "sessions":                 row.get::<i64>(0)?,
        "information_units":        row.get::<i64>(1)?,
        "edges":                    row.get::<i64>(2)?,
        "repairs":                  row.get::<i64>(3)?,
        "pending_classifications":  row.get::<i64>(4)?,
        "version":                  state.version
    }))
}
