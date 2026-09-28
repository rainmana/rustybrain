use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::app::AppState;
use crate::mcp::protocol::{
    tool_result, RpcRequest, RpcResponse, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND,
    PARSE_ERROR,
};
use crate::mcp::tools::all_tools;

/// Serve the MCP protocol over stdin/stdout until stdin closes.
///
/// One line per JSON-RPC message. Notifications (methods beginning with
/// `notifications/`) are silently accepted and produce no response.
pub async fn serve_stdio(state: Arc<AppState>) -> Result<()> {
    info!("MCP server ready (stdio)");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            info!("stdin closed, exiting");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                error!("failed to parse JSON-RPC request: {e}");
                let resp = RpcResponse::err(None, PARSE_ERROR, "Parse error");
                write_response(&mut stdout, &resp).await?;
                continue;
            }
        };

        debug!(method = %req.method, "handling request");

        // Notifications must not receive a response.
        if req.method.starts_with("notifications/") {
            continue;
        }

        let resp = handle_request(req, Arc::clone(&state)).await;
        write_response(&mut stdout, &resp).await?;
    }

    Ok(())
}

async fn write_response(
    stdout: &mut tokio::io::Stdout,
    resp: &RpcResponse,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(resp)?;
    bytes.push(b'\n');
    stdout.write_all(&bytes).await?;
    stdout.flush().await?;
    Ok(())
}

async fn handle_request(req: RpcRequest, state: Arc<AppState>) -> RpcResponse {
    let id = req.id.clone();
    match req.method.as_str() {
        "initialize" => handle_initialize(id, &state),
        "ping" => RpcResponse::ok(id, json!({})),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, req.params, state).await,
        other => {
            warn!(method = %other, "unknown method");
            RpcResponse::err(id, METHOD_NOT_FOUND, format!("method not found: {other}"))
        }
    }
}

fn handle_initialize(id: Option<Value>, state: &AppState) -> RpcResponse {
    RpcResponse::ok(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "rustybrain",
                "version": state.version
            }
        }),
    )
}

fn handle_tools_list(id: Option<Value>) -> RpcResponse {
    let tools: Vec<Value> = all_tools()
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema
            })
        })
        .collect();

    RpcResponse::ok(id, json!({ "tools": tools }))
}

async fn handle_tools_call(
    id: Option<Value>,
    params: Option<Value>,
    state: Arc<AppState>,
) -> RpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return RpcResponse::err(id, INVALID_PARAMS, "tools/call requires params");
        }
    };

    let tool_name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n.to_owned(),
        None => {
            return RpcResponse::err(id, INVALID_PARAMS, "tools/call requires params.name");
        }
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(json!({}));

    match dispatch_tool(&tool_name, args, Arc::clone(&state)).await {
        Ok(result) => RpcResponse::ok(id, tool_result(result)),
        Err(e) => {
            error!(tool = %tool_name, error = %e, "tool call failed");
            RpcResponse::err(id, INTERNAL_ERROR, e.to_string())
        }
    }
}

/// Route a tool call to its handler.
///
/// Handlers return `anyhow::Result<Value>`. Errors are surfaced as JSON-RPC
/// internal errors; domain-level "not found" errors should be returned as
/// successful responses with an `{ "error": "…" }` payload.
async fn dispatch_tool(
    name: &str,
    args: Value,
    state: Arc<AppState>,
) -> anyhow::Result<Value> {
    use crate::services;

    match name {
        // ── Session operations ─────────────────────────────────────────────
        "create_session" => services::session::create_session(args, &state).await,
        "get_session" => services::session::get_session(args, &state).await,
        "list_sessions" => services::session::list_sessions(args, &state).await,

        // ── IU operations ──────────────────────────────────────────────────
        "store_iu" => services::iu::store_iu(args, &state).await,
        "get_iu" => services::iu::get_iu(args, &state).await,
        "search_ius" => services::iu::search_ius(args, &state).await,
        "semantic_search_ius" => services::iu::semantic_search_ius(args, &state).await,
        "create_edge" => services::iu::create_edge(args, &state).await,
        "correlate_ius" => services::iu::correlate_ius(args, &state).await,

        // ── MCW operations ─────────────────────────────────────────────────
        "get_mcw_state" => services::mcw::get_mcw_state(args, &state).await,
        "detect_failure_modes" => services::mcw::detect_failure_modes(args, &state).await,
        "repair_synchronize" => services::mcw::repair(args, &state, crate::domain::mcw::RepairType::Synchronization).await,
        "repair_reground" => services::mcw::repair(args, &state, crate::domain::mcw::RepairType::ReGrounding).await,
        "repair_decompress" => services::mcw::repair(args, &state, crate::domain::mcw::RepairType::Decompression).await,
        "repair_reweight" => services::mcw::repair(args, &state, crate::domain::mcw::RepairType::Reweighting).await,
        "repair_disambiguate" => services::mcw::repair(args, &state, crate::domain::mcw::RepairType::Disambiguation).await,
        "get_repair_history" => services::mcw::get_repair_history(args, &state).await,
        "get_context_summary" => services::mcw::get_context_summary(args, &state).await,

        // ── System ─────────────────────────────────────────────────────────
        "health_check" => services::system::health_check(&state).await,
        "get_stats" => services::system::get_stats(&state).await,

        other => anyhow::bail!("unknown tool: {other}"),
    }
}
