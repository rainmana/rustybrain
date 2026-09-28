/// Background worker that drains the `classification_queue`.
///
/// IUs that failed classification on the synchronous path are enqueued here.
/// The worker polls every `POLL_INTERVAL`, retries up to `MAX_ATTEMPTS` times
/// with exponential backoff, and marks failures as `classification_status = failed`
/// after exhausting retries.
use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use crate::classifier::{ClassificationRequest, Classifier};
use crate::db::connection::Db;
use crate::repository;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: i64 = 3;

pub async fn run(db: Db, classifier: Arc<dyn Classifier>) {
    info!("classification retry worker started");
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if let Err(e) = process_queue(&db, &classifier).await {
            error!(error = %e, "classification worker error");
        }
    }
}

async fn process_queue(db: &Db, classifier: &Arc<dyn Classifier>) -> anyhow::Result<()> {
    // Fetch pending IUs that haven't exceeded MAX_ATTEMPTS.
    let mut rows = db
        .conn
        .query(
            "SELECT q.iu_id, iu.title, iu.content, iu.transfer_direction
             FROM classification_queue q
             JOIN information_units iu ON iu.id = q.iu_id
             WHERE q.attempts < ?1
             ORDER BY q.created_at ASC
             LIMIT 20",
            turso::params![MAX_ATTEMPTS],
        )
        .await?;

    let mut pending = Vec::new();
    while let Some(row) = rows.next().await? {
        pending.push((
            row.get::<String>(0)?,
            row.get::<String>(1)?,
            row.get::<String>(2)?,
            row.get::<String>(3)?,
        ));
    }

    if pending.is_empty() {
        debug!("classification queue empty");
        return Ok(());
    }

    info!(count = pending.len(), "retrying pending classifications");

    for (iu_id, title, content, direction) in pending {
        let req = ClassificationRequest {
            title: title.clone(),
            content,
            transfer_direction: direction,
            session_context: None,
        };

        match classifier.classify(req).await {
            Ok(result) => {
                repository::iu::update_classification(db, &iu_id, &result).await?;
                info!(iu_id = %iu_id, "classification succeeded on retry");
            }
            Err(e) => {
                warn!(iu_id = %iu_id, error = %e, "classification retry failed");
                repository::iu::enqueue_classification(db, &iu_id, &e.to_string()).await?;

                // After MAX_ATTEMPTS, mark as permanently failed.
                db.conn
                    .execute(
                        "UPDATE information_units SET classification_status='failed', updated_at=?1
                         WHERE id=?2 AND (
                           SELECT attempts FROM classification_queue WHERE iu_id=?2
                         ) >= ?3",
                        turso::params![
                            crate::domain::session::now_ms(),
                            iu_id.as_str(),
                            MAX_ATTEMPTS
                        ],
                    )
                    .await?;
            }
        }
    }

    Ok(())
}
