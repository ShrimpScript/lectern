//! Lectern as an MCP (Model Context Protocol) server over stdio.
//!
//! Exposes the shared brain — memory recall + learned skills — as MCP tools, so any
//! MCP client (Claude Code in the app, a terminal agent, future backends) can query
//! the one global brain mid-task. This is the "host" side of MCP: rather than only
//! consuming other servers' tools, Lectern publishes its own over the standard
//! protocol. Transport is newline-delimited JSON-RPC 2.0 on stdin/stdout.

use crate::{Engine, Skill, Workspace};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// The brain tools this server advertises.
fn tool_specs() -> Value {
    json!([
        {
            "name": "recall_memory",
            "description": "Search Lectern's memory of the CURRENT project and return the files most relevant to a query. Use this to pull in context you don't already have before answering or editing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look up" },
                    "limit": { "type": "integer", "description": "Max files to return (default 6)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "list_skills",
            "description": "List the user's learned/recorded Lectern skills (shared across every project), each with a one-line description. Call get_skill to see a recipe.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_skill",
            "description": "Get the full recipe (rules + steps) of a learned Lectern skill by name, so you can apply it.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "The skill name" } },
                "required": ["name"]
            }
        }
    ])
}

fn render_skill(s: &Skill) -> String {
    let mut out = format!("# {}\n{}\n", s.name, s.description);
    if !s.body.rules.is_empty() {
        out.push_str("\nRules:\n");
        for r in &s.body.rules {
            out.push_str(&format!("- {r}\n"));
        }
    }
    if !s.body.steps.is_empty() {
        out.push_str("\nSteps:\n");
        for (i, st) in s.body.steps.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, st));
        }
    }
    out
}

/// Build a JSON-RPC text-content result for a tools/call.
fn text_result(id: Option<Value>, text: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": text }] } })
}

fn handle_call(engine: &Engine, ws: &Workspace, id: Option<Value>, req: &Value) -> Value {
    let params = req.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = match name {
        "recall_memory" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(6)
                .clamp(1, 30);
            let files = engine.recall(ws, q, limit);
            if files.is_empty() {
                format!("No files in memory matched {q:?}.")
            } else {
                let list = files
                    .iter()
                    .map(|f| format!("- {f}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Files in this project most relevant to {q:?}:\n{list}")
            }
        }
        "list_skills" => match engine.list_skills(ws) {
            Ok(sk) if !sk.is_empty() => sk
                .iter()
                .map(|s| format!("- {} — {}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n"),
            Ok(_) => "No learned skills yet.".to_string(),
            Err(e) => format!("Error listing skills: {e}"),
        },
        "get_skill" => {
            let want = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            match engine.list_skills(ws) {
                Ok(sk) => sk
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(want))
                    .map(render_skill)
                    .unwrap_or_else(|| format!("No learned skill named {want:?}.")),
                Err(e) => format!("Error: {e}"),
            }
        }
        other => {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32602, "message": format!("unknown tool: {other}") }
            });
        }
    };
    text_result(id, text)
}

/// Run the stdio MCP server until stdin closes. Only JSON-RPC responses are written
/// to stdout (anything else would corrupt the stream); status goes to stderr.
pub fn serve_stdio(engine: &Engine, ws: &Workspace) -> Result<()> {
    eprintln!(
        "lectern mcp: serving brain for {} (stdio)",
        ws.root.display()
    );
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF — client closed
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // ignore malformed lines
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "lectern-brain", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
            "tools/list" => Some(json!({
                "jsonrpc": "2.0", "id": id, "result": { "tools": tool_specs() }
            })),
            "tools/call" => Some(handle_call(engine, ws, id, &req)),
            "ping" => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
            // Notifications (no id), e.g. notifications/initialized — no reply.
            _ if id.is_some() => Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") }
            })),
            _ => None,
        };
        if let Some(r) = resp {
            writeln!(out, "{}", serde_json::to_string(&r)?)?;
            out.flush()?;
        }
    }
    Ok(())
}
