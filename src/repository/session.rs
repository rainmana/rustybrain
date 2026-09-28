use anyhow::{Context, Result};
use turso::{params, Value};

use crate::db::connection::Db;
use crate::domain::session::{Session, SessionStatus, TaskType};

pub async fn insert(db: &Db, session: &Session) -> Result<()> {
    db.conn
        .execute(
            "INSERT INTO sessions (id, name, description, task_type, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id.as_str(),
                session.name.as_str(),
                session.description.as_str(),
                session.task_type.as_str(),
                session.status.as_str(),
                session.created_at,
                session.updated_at
            ],
        )
        .await
        .context("insert session")?;
    Ok(())
}

pub async fn get_by_id(db: &Db, id: &str) -> Result<Session> {
    let mut rows = db
        .conn
        .query(
            "SELECT id, name, description, task_type, status, created_at, updated_at
             FROM sessions WHERE id = ?1",
            params![id],
        )
        .await
        .context("get session by id")?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

    row_to_session(&row)
}

pub async fn list(
    db: &Db,
    task_type: Option<&str>,
    status: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<Session>> {
    // Build dynamic positional params as a Vec<Value>.
    let mut sql = String::from(
        "SELECT id, name, description, task_type, status, created_at, updated_at
         FROM sessions WHERE 1=1",
    );
    let mut vals: Vec<Value> = Vec::new();
    let mut idx = 1usize;

    if let Some(tt) = task_type {
        sql.push_str(&format!(" AND task_type = ?{idx}"));
        vals.push(Value::Text(tt.to_owned()));
        idx += 1;
    }
    if let Some(st) = status {
        sql.push_str(&format!(" AND status = ?{idx}"));
        vals.push(Value::Text(st.to_owned()));
        idx += 1;
    }
    sql.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ?{idx} OFFSET ?{}",
        idx + 1
    ));
    vals.push(Value::Integer(limit as i64));
    vals.push(Value::Integer(offset as i64));

    let mut rows = db
        .conn
        .query(&sql, turso::params::Params::Positional(vals))
        .await
        .context("list sessions")?;

    let mut sessions = Vec::new();
    while let Some(row) = rows.next().await? {
        sessions.push(row_to_session(&row)?);
    }
    Ok(sessions)
}

fn row_to_session(row: &turso::Row) -> Result<Session> {
    let task_type_str: String = row.get(3)?;
    let status_str: String = row.get(4)?;

    let task_type: TaskType = task_type_str.parse().unwrap_or(TaskType::General);

    let status = match status_str.as_str() {
        "paused" => SessionStatus::Paused,
        "completed" => SessionStatus::Completed,
        "archived" => SessionStatus::Archived,
        _ => SessionStatus::Active,
    };

    Ok(Session {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        task_type,
        status,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
