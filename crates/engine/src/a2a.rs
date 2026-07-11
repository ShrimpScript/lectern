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

/// A2A `TaskNotFoundError` code (the spec's `-32001`).
pub const TASK_NOT_FOUND: i64 = -32001;

// ─────────────────────────── Inbound service ───────────────────────────

/// Turns a prompt into an agent reply. The daemon supplies one that runs a real
/// Lectern turn; tests supply a mock.
pub type Runner = Box<dyn Fn(&str) -> anyhow::Result<String> + Send + Sync>;

/// A minimal in-memory task store + JSON-RPC dispatcher for inbound A2A. Keeping
/// the dispatch logic transport- and backend-agnostic (via the injected
/// [`Runner`]) makes it unit-testable without a socket or a real backend.
pub struct A2aService {
    tasks: std::sync::Mutex<std::collections::HashMap<String, Task>>,
    runner: Runner,
}

impl A2aService {
    pub fn new<F>(runner: F) -> Self
    where
        F: Fn(&str) -> anyhow::Result<String> + Send + Sync + 'static,
    {
        A2aService {
            tasks: std::sync::Mutex::new(std::collections::HashMap::new()),
            runner: Box::new(runner),
        }
    }

    /// Handle one JSON-RPC request body, returning the JSON-RPC response value.
    pub fn handle(&self, body: &str) -> serde_json::Value {
        let req: JsonRpcRequest = match serde_json::from_str(body) {
            Ok(r) => r,
            Err(e) => {
                return rpc_error(
                    &serde_json::Value::Null,
                    error::INVALID_REQUEST,
                    &format!("invalid JSON-RPC request: {e}"),
                );
            }
        };
        if req.jsonrpc != "2.0" {
            return rpc_error(&req.id, error::INVALID_REQUEST, "jsonrpc must be \"2.0\"");
        }
        match req.method.as_str() {
            "message/send" => self.message_send(&req),
            "tasks/get" => self.tasks_get(&req),
            other => rpc_error(
                &req.id,
                error::METHOD_NOT_FOUND,
                &format!("unknown method: {other}"),
            ),
        }
    }

    fn message_send(&self, req: &JsonRpcRequest) -> serde_json::Value {
        let params: MessageSendParams = match serde_json::from_value(req.params.clone()) {
            Ok(p) => p,
            Err(e) => {
                return rpc_error(
                    &req.id,
                    error::INVALID_PARAMS,
                    &format!("invalid message/send params: {e}"),
                );
            }
        };
        let prompt = Part::joined_text(&params.message.parts);
        if prompt.trim().is_empty() {
            return rpc_error(
                &req.id,
                error::INVALID_PARAMS,
                "message has no text content to run",
            );
        }
        let task_id = uuid::Uuid::new_v4().to_string();
        let context_id = params
            .message
            .context_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut user_msg = params.message.clone();
        user_msg.task_id = Some(task_id.clone());
        user_msg.context_id = Some(context_id.clone());

        // This slice runs synchronously: the turn completes before we answer, so
        // the returned Task is already terminal. Streaming / async lifecycle is a
        // later slice. Never auto-applies to disk — an inbound peer's task returns
        // proposed changes, it does not silently write the workspace.
        let task = match (self.runner)(&prompt) {
            Ok(reply) => {
                let agent_msg = Message::agent_text(reply, &task_id, &context_id);
                Task {
                    id: task_id.clone(),
                    context_id: context_id.clone(),
                    status: TaskStatus {
                        state: TaskState::Completed,
                        message: Some(agent_msg.clone()),
                        timestamp: Some(now_rfc3339()),
                    },
                    history: vec![user_msg, agent_msg],
                    artifacts: vec![],
                }
            }
            Err(e) => {
                let agent_msg =
                    Message::agent_text(format!("run failed: {e}"), &task_id, &context_id);
                Task {
                    id: task_id.clone(),
                    context_id: context_id.clone(),
                    status: TaskStatus {
                        state: TaskState::Failed,
                        message: Some(agent_msg),
                        timestamp: Some(now_rfc3339()),
                    },
                    history: vec![user_msg],
                    artifacts: vec![],
                }
            }
        };
        if let Ok(mut store) = self.tasks.lock() {
            store.insert(task_id, task.clone());
        }
        rpc_result(
            &req.id,
            serde_json::to_value(&task).unwrap_or(serde_json::Value::Null),
        )
    }

    fn tasks_get(&self, req: &JsonRpcRequest) -> serde_json::Value {
        let params: TaskGetParams = match serde_json::from_value(req.params.clone()) {
            Ok(p) => p,
            Err(e) => {
                return rpc_error(
                    &req.id,
                    error::INVALID_PARAMS,
                    &format!("invalid tasks/get params: {e}"),
                );
            }
        };
        let found = self
            .tasks
            .lock()
            .ok()
            .and_then(|s| s.get(&params.id).cloned());
        match found {
            Some(task) => rpc_result(
                &req.id,
                serde_json::to_value(&task).unwrap_or(serde_json::Value::Null),
            ),
            None => rpc_error(
                &req.id,
                TASK_NOT_FOUND,
                &format!("task not found: {}", params.id),
            ),
        }
    }
}

/// Current UTC time as an RFC 3339 / ISO 8601 string (`YYYY-MM-DDThh:mm:ssZ`).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_utc(secs)
}

/// Format a Unix timestamp (seconds) as UTC RFC 3339 without a date-library
/// dependency (Howard Hinnant's `civil_from_days`).
fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as i64;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
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

    #[test]
    fn message_send_runs_and_tasks_get_returns_it() {
        let svc = A2aService::new(|prompt: &str| Ok(format!("echo: {prompt}")));
        let resp = svc.handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"message/send","params":{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello world"}]}}}"#,
        );
        assert_eq!(resp["id"], 1);
        let task = &resp["result"];
        assert_eq!(task["status"]["state"], "TASK_STATE_COMPLETED");
        assert_eq!(task["status"]["message"]["role"], "ROLE_AGENT");
        assert!(task["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("echo: hello world"));
        // history carries the user turn + the agent reply
        assert_eq!(task["history"].as_array().unwrap().len(), 2);
        let tid = task["id"].as_str().unwrap().to_string();

        // tasks/get returns the same terminal task
        let got = svc.handle(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tasks/get","params":{{"id":"{tid}"}}}}"#
        ));
        assert_eq!(got["result"]["id"], tid);
        assert_eq!(got["result"]["status"]["state"], "TASK_STATE_COMPLETED");
    }

    #[test]
    fn runner_failure_yields_failed_task() {
        let svc = A2aService::new(|_| Err(anyhow::anyhow!("boom")));
        let resp = svc.handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"message/send","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"x"}]}}}"#,
        );
        assert_eq!(resp["result"]["status"]["state"], "TASK_STATE_FAILED");
        assert!(resp["result"]["status"]["message"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("boom"));
    }

    #[test]
    fn error_paths_are_well_formed_jsonrpc() {
        let svc = A2aService::new(|_| Ok("x".to_string()));
        // empty text
        let r1 = svc.handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"message/send","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[]}}}"#,
        );
        assert_eq!(r1["error"]["code"], error::INVALID_PARAMS);
        // unknown method
        let r2 = svc.handle(r#"{"jsonrpc":"2.0","id":2,"method":"foo/bar","params":{}}"#);
        assert_eq!(r2["error"]["code"], error::METHOD_NOT_FOUND);
        // unknown task
        let r3 =
            svc.handle(r#"{"jsonrpc":"2.0","id":3,"method":"tasks/get","params":{"id":"nope"}}"#);
        assert_eq!(r3["error"]["code"], TASK_NOT_FOUND);
        // not JSON at all
        let r4 = svc.handle("not json");
        assert_eq!(r4["error"]["code"], error::INVALID_REQUEST);
    }

    #[test]
    fn rfc3339_utc_known_epochs() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
    }
}
