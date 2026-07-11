# A2A (Agent2Agent) — local interop design

Status: design (mission slice S1). No code yet; this pins the surface and the decisions the
implementation slices build against.

## Why

A2A reached v1.0 in April 2026 and is governed under the Linux Foundation. It is the
de-facto standard for agents to *collaborate*, and it complements MCP rather than competing
with it: MCP equips one agent with tools; A2A lets two already-equipped agents hand work to
each other. Lectern already speaks MCP (client and a harness server). Adding A2A lets other
local A2A agents delegate work to Lectern, and lets Lectern's Conductor delegate a plan step
out to another local agent — without giving up the local-first posture.

Scope is deliberately **local**: everything binds loopback, is off by default, and adds no
cloud dependency. Exposing A2A beyond `127.0.0.1` is explicitly out of scope here and would
be a separate, separately-gated decision.

## The subset we implement

A2A v1.0 defines a large surface (SendMessage, SendStreamingMessage, GetTask, ListTasks,
CancelTask, SubscribeToTask, push-notification config, GetExtendedAgentCard) across three
transports (JSON-RPC over HTTP, gRPC, HTTP+JSON/REST). We implement the **minimum coherent
core over the JSON-RPC/HTTP transport**:

| Surface | v1.0 method | We implement | Notes |
|---|---|---|---|
| Discovery | `GET /.well-known/agent-card.json` | ✅ | static card, describes Lectern's skills |
| Send a message | `message/send` | ✅ | runs a Lectern turn, returns a Task |
| Poll a task | `tasks/get` | ✅ | lifecycle state + result |
| Streaming send | `message/sendStreaming` (SSE) | deferred | needs an SSE story; note below |
| List/cancel/subscribe/push | `tasks/list` · `tasks/cancel` · `tasks/subscribe` · push config | deferred | not needed for the first useful hop |

`tasks/cancel` is a strong candidate to add in S4 alongside the lifecycle work, since Lectern
runs can already be cancelled — it is listed as a stretch there, not a separate slice.

## Wire format (v1.0 — ProtoJSON, not the v0.x form)

v1.0's normative source is the protobuf model (`specification/a2a.proto`), and the JSON-RPC
binding serializes it as **ProtoJSON**. Two consequences that differ from pre-1.0 examples
still floating around the web, and that we must get right:

- **Field names are `camelCase`** (`messageId`, `contextId`, `protocolVersion`, `taskId`).
- **Enum values are `SCREAMING_SNAKE_CASE`** — `TASK_STATE_COMPLETED`, `ROLE_USER` — *not*
  the older lowercase `"completed"` / `"user"`, and Parts are a protobuf `oneof` serialized
  by field name (`text` / `raw` / `url` / `data`), *not* the older `{"kind":"text",…}` form.

Because of this, **S2's first job is to validate exact serialization against the canonical
generated JSON Schema (`specification/json/a2a.json`) and round-trip our types against a
reference client** (a2a-python / a2a-inspector) before building on them. The shapes below are
the target; the schema is the authority if they disagree.

### AgentCard (served at `/.well-known/agent-card.json`)

Required: `name`, `version`, `url`, `protocolVersion` (`"1.0"`), `provider`, `capabilities`,
`defaultInputModes`, `defaultOutputModes`; plus `skills`.

```json
{
  "name": "Lectern",
  "version": "0.8.0",
  "url": "http://127.0.0.1:41041/a2a",
  "protocolVersion": "1.0",
  "provider": { "name": "Lectern", "url": "https://github.com/ShrimpScript/lectern" },
  "capabilities": { "streaming": false, "pushNotifications": false, "extendedAgentCard": false },
  "defaultInputModes": ["text/plain"],
  "defaultOutputModes": ["text/plain"],
  "skills": [
    { "id": "run", "name": "Run a coding task",
      "description": "Execute a software task in a Lectern workspace and return the result." }
  ]
}
```

`version` and `url` are filled at runtime (workspace version, the loopback address the
listener actually bound). `skills` starts with a single "run" skill; more can be surfaced
later (e.g. `conduct`).

### message/send

```json
{ "jsonrpc": "2.0", "id": 1, "method": "message/send",
  "params": { "message": {
    "messageId": "…", "role": "ROLE_USER",
    "parts": [ { "text": "add a settings page" } ] } } }
```

Result is a `Task` (Lectern always returns a Task, not a bare Message, so the caller gets an
id to poll):

```json
{ "jsonrpc": "2.0", "id": 1, "result": {
  "id": "…", "contextId": "…",
  "status": { "state": "TASK_STATE_COMPLETED", "timestamp": "2026-07-11T…Z" },
  "history": [ /* the user message + the agent reply message */ ],
  "artifacts": [] } }
```

### tasks/get

```json
{ "jsonrpc": "2.0", "id": 2, "method": "tasks/get", "params": { "id": "…" } }
```

Result is the same `Task` object, reflecting current `status.state`. TaskState values we use:
`TASK_STATE_SUBMITTED` → `TASK_STATE_WORKING` → `TASK_STATE_COMPLETED` | `TASK_STATE_FAILED`
(+ `TASK_STATE_CANCELED` if cancel lands).

Errors use standard JSON-RPC error objects (`-32600` invalid request, `-32601` method not
found, `-32602` invalid params, `-32603` internal), plus A2A's task-not-found range where it
applies.

## Implementation decisions

### HTTP stack — `tiny_http`, not axum, not hand-rolled

The engine has **zero HTTP dependencies** today, and `lecternd` is a *synchronous,
thread-per-connection* Unix-socket server (no async runtime). Three options weighed:

- **axum + tokio/hyper** — robust and SSE-ready, but drags an async runtime into a sync
  daemon: a large dependency tree and an architectural shift for a loopback endpoint that
  handles small JSON. Rejected for now; revisit only if/when streaming (SSE) is worth it.
- **Hand-rolled `std::net::TcpListener`** — zero new deps, but re-implements HTTP/1.1 parsing
  (Content-Length, keep-alive, malformed-request handling) — parser bugs on a network-facing
  surface are exactly what we don't want.
- **`tiny_http`** ✅ — a small, **synchronous** HTTP/1.1 server crate with a light dependency
  set and no async runtime. It fits `lecternd`'s existing threaded model, handles request
  parsing/Content-Length correctly, and keeps the footprint auditable. This is the pick.

Only two routes are needed: `GET /.well-known/agent-card.json` and `POST /a2a`.

### Where it lives

- **`engine::a2a`** — the protocol types (serde), the request handler
  (`handle_a2a_request(&Engine, HttpReqParts) -> HttpRespParts`-style, transport-agnostic and
  unit-testable), the in-process task store, and the **client** (so the Conductor, which lives
  in the engine, can call peers). Keeping the logic in the engine means it is tested without a
  socket and without `lecternd`.
- **`lecternd`** — owns the `tiny_http` listener thread (spawned at daemon start *only when
  enabled*), reads each request, and dispatches to `engine::a2a`. Thin transport glue.

### Opt-in + security

The inbound endpoint runs agent turns, so it is a real new surface and is treated as such:

- **Off by default.** Enabled explicitly via `lectern daemon --a2a` (and/or env
  `LECTERN_A2A=1`); the bind address defaults to `127.0.0.1:41041` and can be set with
  `LECTERN_A2A_ADDR` — but the code **refuses any non-loopback address** unless a separate,
  deliberately-named override is set (not in scope here). No `0.0.0.0`, ever, by accident.
- **Optional bearer token.** If `LECTERN_A2A_TOKEN` is set, `message/send` requires
  `Authorization: Bearer <token>`; the agent card stays readable (it is just capability
  metadata). Recommended even on loopback on a shared machine.
- **No cloud, no telemetry.** Nothing about A2A phones home; peers are explicitly configured.
- **Body cap.** Requests are size-capped; oversized or malformed bodies get a clean 400.

### Outbound (client) config

The Conductor delegates only to **explicitly configured** local peers — never auto-discovered
off the network. A peer list (name → base URL, loopback/LAN, optional token) lives in config
(`~/.lectern/a2a-peers.json` or a section of the existing config). The client fetches a peer's
card, calls `message/send`, and polls `tasks/get` until terminal. If no peers are configured,
Conductor behaviour is exactly as today.

## Deferred / follow-ups (tracked, not lost)

- **Streaming** (`message/sendStreaming` via SSE) — the main reason someone would want axum;
  revisit as a unit if there's demand. Card advertises `streaming: false` until then.
- `tasks/list`, `tasks/subscribe`, push-notification config, `GetExtendedAgentCard`.
- Exposing A2A beyond loopback (auth hardening, a real threat model) — separate, gated.
- Surfacing inbound A2A tasks in the desktop/TUI as first-class sessions.

## Slice map (mirrors MISSION.md)

S2 types + card served · S3 inbound `message/send` → turn · S4 lifecycle (`tasks/get`,
stretch `tasks/cancel`) · S5 client · S6 Conductor delegation · S7 surface (`doctor`/daemon
status) + docs + audit.

## Sources

- Agent2Agent Protocol v1.0 specification — https://a2a-protocol.org/v1.0.0/specification/
- Canonical spec (protobuf-normative) — https://github.com/a2aproject/A2A/blob/main/docs/specification.md
- A2A vs MCP (positioning) — https://beam.ai/agentic-insights/agent2agent-vs-mcp-2026-ai-agent-stack
