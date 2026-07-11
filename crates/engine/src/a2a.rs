//! Agent2Agent (A2A) v1.0 — local interop types + the Lectern agent card.
//!
//! A2A (v1.0, Apr 2026, Linux Foundation) is the inter-agent standard that
//! complements MCP: MCP equips one agent with tools; A2A lets equipped agents
//! hand work to each other. This module holds the wire types and the agent card;
//! the loopback listener that serves them lives in `lecternd`, and the request
//! handler + client land in later slices. See docs/a2a-design.md.
//!
//! Wire format is **ProtoJSON** (A2A v1.0): field names are `camelCase` and enum
//! values are `SCREAMING_SNAKE_CASE` (`TASK_STATE_COMPLETED`, `ROLE_USER`) — not
//! the pre-1.0 lowercase / `kind`-discriminated form. A `Part` is a flat protobuf
//! oneof: `{"text": "…"}`, not `{"kind":"text","text":"…"}`.

use serde::{Deserialize, Serialize};

/// The protocol version this implementation targets.
pub const PROTOCOL_VERSION: &str = "1.0";

// ─────────────────────────── Agent Card ───────────────────────────

/// An A2A Agent Card — the "business card" served at
/// `/.well-known/agent-card.json` so other agents can discover what this agent
/// can do.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub protocol_version: String,
    pub name: String,
    pub description: String,
    /// The service endpoint clients POST JSON-RPC to.
    pub url: String,
    pub version: String,
    pub provider: AgentProvider,
    pub capabilities: AgentCapabilities,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_transport: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub organization: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
}

/// Build Lectern's agent card. `version` is the daemon version and `url` is the
/// JSON-RPC endpoint the listener actually bound (both filled at runtime).
pub fn agent_card(version: &str, url: &str) -> AgentCard {
    AgentCard {
        protocol_version: PROTOCOL_VERSION.to_string(),
        name: "Lectern".to_string(),
        description: "Local-first agent orchestration. Runs a coding task in a workspace \
                      with per-task model routing and a persistent brain, and returns the \
                      result and any file changes."
            .to_string(),
        url: url.to_string(),
        version: version.to_string(),
        provider: AgentProvider {
            organization: "Lectern".to_string(),
            url: "https://github.com/ShrimpScript/lectern".to_string(),
        },
        // Streaming (SSE) and push notifications are deferred (see the design doc),
        // so the card advertises them off rather than claiming support we lack.
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: false,
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills: vec![AgentSkill {
            id: "run".to_string(),
            name: "Run a coding task".to_string(),
            description: "Execute a software task in a Lectern workspace and return the \
                          result and any proposed file changes."
                .to_string(),
            tags: vec!["code".to_string(), "agent".to_string(), "local".to_string()],
            examples: Some(vec![
                "add a settings page".to_string(),
                "fix the failing test in the parser".to_string(),
            ]),
        }],
        preferred_transport: Some("JSONRPC".to_string()),
    }
}

// ─────────────────────────── Messages & Tasks ───────────────────────────

/// Who authored a message. Serializes as `ROLE_USER` / `ROLE_AGENT` (ProtoJSON).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

/// A single content part. A2A v1.0 flattens the protobuf oneof, so a text part
/// serializes as `{"text": "…"}` (no `kind` discriminator).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    Text(String),
    Data(serde_json::Value),
    File(FilePart),
}

impl Part {
    pub fn text(s: impl Into<String>) -> Self {
        Part::Text(s.into())
    }
    /// The concatenated text of a message's parts (ignores non-text parts).
    pub fn joined_text(parts: &[Part]) -> String {
        parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub message_id: String,
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl Message {
    /// A fresh user message carrying a single text part.
    pub fn user_text(text: impl Into<String>) -> Self {
        Message {
            message_id: uuid::Uuid::new_v4().to_string(),
            role: Role::User,
            parts: vec![Part::text(text)],
            context_id: None,
            task_id: None,
        }
    }
    /// A fresh agent reply carrying a single text part, tied to a task/context.
    pub fn agent_text(text: impl Into<String>, task_id: &str, context_id: &str) -> Self {
        Message {
            message_id: uuid::Uuid::new_v4().to_string(),
            role: Role::Agent,
            parts: vec![Part::text(text)],
            context_id: Some(context_id.to_string()),
            task_id: Some(task_id.to_string()),
        }
    }
}

/// Task lifecycle state. Serializes as ProtoJSON `TASK_STATE_*`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<serde_json::Value>,
}

// ─────────────────────────── JSON-RPC envelope ───────────────────────────

/// Params for the `message/send` method.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendParams {
    pub message: Message,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub configuration: serde_json::Value,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

/// Params for the `tasks/get` method.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskGetParams {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
}

/// A minimal JSON-RPC 2.0 request envelope. `id` and `params` stay as raw JSON so
/// the handler can dispatch on `method` before typing the params.
#[derive(Deserialize, Clone, Debug)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Standard JSON-RPC error codes we use.
pub mod error {
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
}

/// Build a JSON-RPC success response.
pub fn rpc_result(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC error response.
pub fn rpc_error(id: &serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_card_serializes_protojson_camelcase() {
        let card = agent_card("0.8.0", "http://127.0.0.1:41041/a2a");
        let v = serde_json::to_value(&card).unwrap();
        assert_eq!(v["protocolVersion"], "1.0");
        assert_eq!(v["name"], "Lectern");
        assert_eq!(v["version"], "0.8.0");
        assert_eq!(v["url"], "http://127.0.0.1:41041/a2a");
        // camelCase keys present (not snake_case)
        assert!(v.get("defaultInputModes").is_some());
        assert!(v.get("default_input_modes").is_none());
        assert_eq!(v["skills"][0]["id"], "run");
        assert_eq!(v["preferredTransport"], "JSONRPC");
        assert_eq!(v["capabilities"]["streaming"], false);
        // round-trips
        let back: AgentCard = serde_json::from_value(v).unwrap();
        assert_eq!(back, card);
    }

    #[test]
    fn enums_use_screaming_snake_case() {
        assert_eq!(serde_json::to_value(Role::User).unwrap(), "ROLE_USER");
        assert_eq!(serde_json::to_value(Role::Agent).unwrap(), "ROLE_AGENT");
        assert_eq!(
            serde_json::to_value(TaskState::Completed).unwrap(),
            "TASK_STATE_COMPLETED"
        );
        assert_eq!(
            serde_json::to_value(TaskState::Working).unwrap(),
            "TASK_STATE_WORKING"
        );
    }

    #[test]
    fn text_part_is_flat_oneof_no_kind() {
        let v = serde_json::to_value(Part::text("hello")).unwrap();
        assert_eq!(v, serde_json::json!({ "text": "hello" }));
        assert!(v.get("kind").is_none());
        // and deserializes back
        let back: Part = serde_json::from_value(serde_json::json!({ "text": "hi" })).unwrap();
        assert_eq!(back, Part::text("hi"));
    }

    #[test]
    fn message_send_params_deserialize_from_v1_wire() {
        // The exact shape a v1.0 A2A client sends.
        let wire = serde_json::json!({
            "message": {
                "messageId": "m-1",
                "role": "ROLE_USER",
                "parts": [ { "text": "add a settings page" } ]
            }
        });
        let params: MessageSendParams = serde_json::from_value(wire).unwrap();
        assert_eq!(params.message.role, Role::User);
        assert_eq!(params.message.message_id, "m-1");
        assert_eq!(
            Part::joined_text(&params.message.parts),
            "add a settings page"
        );
    }

    #[test]
    fn task_round_trips() {
        let task = Task {
            id: "t-1".to_string(),
            context_id: "c-1".to_string(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: Some(Message::agent_text("done", "t-1", "c-1")),
                timestamp: Some("2026-07-11T00:00:00Z".to_string()),
            },
            history: vec![Message::user_text("go")],
            artifacts: vec![],
        };
        let v = serde_json::to_value(&task).unwrap();
        assert_eq!(v["contextId"], "c-1");
        assert_eq!(v["status"]["state"], "TASK_STATE_COMPLETED");
        assert!(v.get("artifacts").is_none()); // empty vec skipped
        let back: Task = serde_json::from_value(v).unwrap();
        assert_eq!(back, task);
    }

    #[test]
    fn jsonrpc_request_parses_and_responses_build() {
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":7,"method":"tasks/get","params":{"id":"t-1"}}"#,
        )
        .unwrap();
        assert_eq!(req.method, "tasks/get");
        let params: TaskGetParams = serde_json::from_value(req.params).unwrap();
        assert_eq!(params.id, "t-1");

        let ok = rpc_result(&req.id, serde_json::json!({ "state": "ok" }));
        assert_eq!(ok["result"]["state"], "ok");
        let err = rpc_error(&req.id, error::METHOD_NOT_FOUND, "nope");
        assert_eq!(err["error"]["code"], error::METHOD_NOT_FOUND);
    }
}
