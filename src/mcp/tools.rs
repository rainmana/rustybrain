use serde_json::{json, Value};

/// A registered MCP tool.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Returns all registered tools.
///
/// Tool handlers live in [`crate::mcp::connection::McpServer`]; this function
/// only provides the schema definitions surfaced to MCP clients via
/// `tools/list`.
pub fn all_tools() -> Vec<Tool> {
    vec![
        // ── IU Operations ────────────────────────────────────────────────────
        Tool {
            name: "store_iu",
            description: "Store a new Information Unit. Triggers async Jev classification.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":          { "type": "string", "description": "ID of the session this IU belongs to" },
                    "title":               { "type": "string", "description": "Brief summary of the IU" },
                    "content":             { "type": "string", "description": "The IU content" },
                    "content_type":        { "type": "string", "description": "text | code | json | yaml | markdown | binary_ref | structured" },
                    "transfer_direction":  { "type": "string", "description": "human→ai | ai→human | ai→subagent | subagent→ai | rag→ai | tool→ai | …" },
                    "source_actor":        { "type": "string" },
                    "target_actor":        { "type": "string" },
                    "tags":                { "type": "string", "description": "JSON array of tags" },
                    "confidence":          { "type": "number", "description": "0.0–1.0 (default 0.5)" },
                    "wait_for_classification": { "type": "boolean", "description": "Block until Jev classifies this IU (default false)" }
                },
                "required": ["session_id", "title", "content"]
            }),
        },
        Tool {
            name: "get_iu",
            description: "Retrieve an Information Unit by ID.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "iu_id": { "type": "string" }
                },
                "required": ["iu_id"]
            }),
        },
        Tool {
            name: "search_ius",
            description: "Search IUs by text (FTS5), tags, semantic type, priority, or session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query":        { "type": "string" },
                    "session_id":   { "type": "string" },
                    "semantic_type":{ "type": "string" },
                    "tags":         { "type": "string", "description": "JSON array of tags" },
                    "min_priority": { "type": "number" },
                    "limit":        { "type": "number", "description": "Default 20" },
                    "offset":       { "type": "number" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "semantic_search_ius",
            description: "Vector similarity search using stored embeddings. Requires an embedding provider to be configured.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query":      { "type": "string" },
                    "session_id": { "type": "string" },
                    "limit":      { "type": "number", "description": "Default 10" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "create_edge",
            description: "Create a typed directed edge between two IUs in the graph.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source_iu_id": { "type": "string" },
                    "target_iu_id": { "type": "string" },
                    "edge_type":    { "type": "string", "description": "depends_on | causes | mitigates | exploits | references | contradicts | supports | related_to | parent_of | child_of | repairs | precedes | supersedes | corroborates | refutes | instantiates | exemplifies | abstracts" },
                    "strength":     { "type": "number", "description": "0.0–1.0 (default 0.5)" },
                    "description":  { "type": "string" }
                },
                "required": ["source_iu_id", "target_iu_id", "edge_type"]
            }),
        },
        Tool {
            name: "correlate_ius",
            description: "Find non-obvious connections between IUs across the graph using embedding similarity and shared Jev classifications.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":   { "type": "string" },
                    "iu_ids":       { "type": "string", "description": "JSON array of IU IDs to correlate. If omitted, searches all IUs in the session." },
                    "auto_create":  { "type": "boolean", "description": "Automatically create proposed edges (default false)" },
                    "limit":        { "type": "number", "description": "Max proposed correlations to return (default 10)" }
                },
                "required": ["session_id"]
            }),
        },
        // ── Session Operations ───────────────────────────────────────────────
        Tool {
            name: "create_session",
            description: "Create a new session. Initialises an MCW state record.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name":        { "type": "string" },
                    "description": { "type": "string" },
                    "task_type":   { "type": "string", "description": "security_review | pentest | exploit_dev | vuln_analysis | threat_model | incident_response | intel_analysis | reverse_engineering | malware_analysis | ctf | general" }
                },
                "required": ["name", "task_type"]
            }),
        },
        Tool {
            name: "get_session",
            description: "Retrieve a session and its current MCW state.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
        Tool {
            name: "list_sessions",
            description: "List sessions with optional filtering.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_type": { "type": "string" },
                    "status":    { "type": "string", "description": "active | paused | completed | archived" },
                    "limit":     { "type": "number" },
                    "offset":    { "type": "number" }
                }
            }),
        },
        // ── MCW Operations ───────────────────────────────────────────────────
        Tool {
            name: "get_mcw_state",
            description: "Get the current MCW health state for a session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
        Tool {
            name: "detect_failure_modes",
            description: "Run MCW failure mode detection against the current IU graph for a session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
        Tool {
            name: "repair_synchronize",
            description: "Synchronization repair: externalize HCW state changes that occurred off-turn. The most critical repair operation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":  { "type": "string" },
                    "description": { "type": "string", "description": "What changed in your thinking or context since the last interaction" },
                    "iu_ids":      { "type": "string", "description": "JSON array of IU IDs that are superseded or contradicted by this update" }
                },
                "required": ["session_id", "description"]
            }),
        },
        Tool {
            name: "repair_reground",
            description: "Re-grounding repair: reintroduce foundational goals and assumptions.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":  { "type": "string" },
                    "description": { "type": "string" }
                },
                "required": ["session_id", "description"]
            }),
        },
        Tool {
            name: "repair_decompress",
            description: "Decompression repair: expand a compressed IU back into its constituent distinctions.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":    { "type": "string" },
                    "compressed_iu": { "type": "string", "description": "ID of the IU to decompress" },
                    "description":   { "type": "string" }
                },
                "required": ["session_id", "description"]
            }),
        },
        Tool {
            name: "repair_reweight",
            description: "Re-weighting repair: explicitly change which IUs carry the most salience.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":  { "type": "string" },
                    "description": { "type": "string" },
                    "iu_ids":      { "type": "string", "description": "JSON array of IU IDs to re-weight" }
                },
                "required": ["session_id", "description"]
            }),
        },
        Tool {
            name: "repair_disambiguate",
            description: "Disambiguation repair: split an overloaded term or IU into its competing interpretations.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":  { "type": "string" },
                    "description": { "type": "string" },
                    "iu_ids":      { "type": "string", "description": "JSON array of IU IDs involved" }
                },
                "required": ["session_id", "description"]
            }),
        },
        Tool {
            name: "get_repair_history",
            description: "Get the repair log for a session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "limit":      { "type": "number" }
                },
                "required": ["session_id"]
            }),
        },
        Tool {
            name: "get_context_summary",
            description: "Get an MCW-aware summary of the session for resuming or handing off work.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id":   { "type": "string" },
                    "current_task": { "type": "string", "description": "What are you working on right now?" }
                },
                "required": ["session_id"]
            }),
        },
        // ── System Operations ────────────────────────────────────────────────
        Tool {
            name: "health_check",
            description: "Check database connectivity and embedding/Jev provider availability.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "get_stats",
            description: "IU counts, edge counts, MCW health distribution, classification coverage.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
    ]
}
