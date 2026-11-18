//! Minimal MCP server (JSON-RPC 2.0, newline-delimited). Use [`serve`] over stdio, or hand it
//! any reader/writer pair to embed it in your own harness or transport.

use crate::{LEVELS, TinyBrain};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// Serve MCP requests until `input` closes. Secrets are masked in tool output unless the
/// caller passes `reveal: true` (so creds don't end up in transcripts by accident).
pub fn serve(tb: &TinyBrain, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    for line in input.lines() {
        let Ok(msg) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        }; // notifications need no reply
        let params = &msg["params"];
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "tinybrain", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Long-term memory for security work. `remember` findings, creds, recon and TODOs as you go; `recall` before you test something."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => Ok(call(
                tb,
                params["name"].as_str().unwrap_or(""),
                &params["arguments"],
            )),
            m => Err(json!({"code": -32601, "message": format!("method not found: {m}")})),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        writeln!(output, "{resp}")?;
        output.flush()?;
    }
    Ok(())
}

fn tools() -> Value {
    let project = json!({"type": "string", "description": "Engagement/project name to scope to"});
    let reveal =
        json!({"type": "boolean", "description": "Show secret values unmasked (default false)"});
    json!([
        {"name": "remember", "description": "Store a memory: a finding, vuln, credential, recon detail, technique, TODO. It is auto-classified by category, importance and whether it holds a secret.",
         "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}, "project": project}, "required": ["text"]}},
        {"name": "recall", "description": "Find memories relevant to a query, ranked by meaning.",
         "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer", "default": 5}, "project": project, "reveal": reveal}, "required": ["query"]}},
        {"name": "recent", "description": "List the newest memories, optionally by category (finding, vulnerability, credential, recon, technique, exploit, todo, idea, reference, note).",
         "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "default": 10}, "category": {"type": "string"}, "project": project, "reveal": reveal}}},
        {"name": "forget", "description": "Delete a memory by id.",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}},
    ])
}

fn call(tb: &TinyBrain, name: &str, a: &Value) -> Value {
    let project = a["project"].as_str();
    let reveal = a["reveal"].as_bool().unwrap_or(false);
    let line = |m: &crate::Memory, rel: Option<f64>| {
        let rel = rel.map_or(String::new(), |r| format!(" relevance={:.0}%", r * 100.0));
        let text = if reveal { m.text.clone() } else { m.masked() };
        let proj = m
            .project
            .as_deref()
            .map_or(String::new(), |p| format!(" project={p}"));
        format!(
            "#{} [{}, {}{}]{proj}{rel} {text}",
            m.id,
            m.category,
            LEVELS[m.importance as usize],
            if m.secret { ", secret" } else { "" }
        )
    };
    let out: Result<String, String> = match name {
        "remember" => match a["text"].as_str().filter(|t| !t.trim().is_empty()) {
            None => Err("text is required".into()),
            Some(t) => tb.remember(t, project).map_err(|e| e.to_string()).map(|r| {
                let fb = r
                    .verdict
                    .fallback
                    .map_or(String::new(), |f| format!(" (keyword fallback: {f})"));
                format!("remembered {}{fb}", line(&r.memory, None))
            }),
        },
        "recall" => match a["query"].as_str() {
            None => Err("query is required".into()),
            Some(q) => tb
                .recall(q, a["limit"].as_u64().unwrap_or(5) as usize, project)
                .map_err(|e| e.to_string())
                .map(|hits| {
                    if hits.is_empty() {
                        "nothing relevant remembered".into()
                    } else {
                        hits.iter()
                            .map(|h| line(&h.memory, h.relevance))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                }),
        },
        "recent" => tb
            .recent(
                a["limit"].as_u64().unwrap_or(10) as usize,
                project,
                a["category"].as_str(),
            )
            .map_err(|e| e.to_string())
            .map(|ms| {
                if ms.is_empty() {
                    "no memories yet".into()
                } else {
                    ms.iter()
                        .map(|m| line(m, None))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }),
        "forget" => match a["id"].as_i64() {
            None => Err("id is required".into()),
            Some(id) => tb.forget(id).map_err(|e| e.to_string()).map(|ok| {
                if ok {
                    format!("forgot #{id}")
                } else {
                    format!("no memory #{id}")
                }
            }),
        },
        _ => Err(format!("unknown tool {name}")),
    };
    let (text, is_error) = match out {
        Ok(t) => (t, false),
        Err(e) => (e, true),
    };
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Brain;

    #[test]
    fn round_trip() {
        let tb = TinyBrain::in_memory(Brain::offline()).unwrap();
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"remember","arguments":{"text":"db creds sa password=Winter2026"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"recall","arguments":{"query":"db creds"}}}"#,
        ]
        .join("\n");
        let mut out = Vec::new();
        serve(&tb, input.as_bytes(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out.lines().count(), 3);
        assert!(out.contains("credential") && out.contains("Wi•••") && !out.contains("Winter2026"));
    }
}
