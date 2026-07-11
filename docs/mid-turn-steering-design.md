# Mid-turn steering — design

Status: design (mission slice S1). No code yet; this pins the mechanism, an honest backend
matrix, and the deliberately-scoped path forward.

## Why, and what already exists

You often realise mid-run that the agent is heading the wrong way. Lectern already ships
**queue-for-next** (a follow-up message you compose during a run that is promoted into the composer
when the turn ends). This mission is the harder thing: getting a steering message to the agent
**while the current turn is still running**, so it can course-correct without you waiting for it to
finish.

This is **backend-protocol-dependent** and honestly uncertain: today every backend is spawned
one-shot (`claude -p <prompt> --output-format stream-json --verbose …`, `stdin` set to null), reads
its own output stream, and exits. There is no live input channel. True injection needs each CLI to
accept input on a live session — which not all do, and which we cannot verify end-to-end here
because running a real backend spends the user's tokens.

## Backend matrix (honest)

| Backend | Live mid-turn input? | How | Confidence |
|---|---|---|---|
| **Claude Code** | Yes (mechanism exists) | `--input-format stream-json` accepts NDJSON messages on stdin during a session; pairs with the `--output-format stream-json --verbose` Lectern already passes. `--replay-user-messages` echoes sent messages back on stdout. | Mechanism confirmed; **exact stdin user-message envelope is a documented gap** (anthropics/claude-code #24594) — must be validated against the CLI/SDK before relying on it. |
| **OpenCode** | Not in Lectern's current invocation | Lectern drives it one-shot; a live session would need OpenCode's server/session mode, out of scope here. | Treated as **not supported** for mid-turn until proven otherwise. |
| **Antigravity** | No | One-shot invocation, no live-input mode used. | **Not supported.** |
| **Mock** | Yes (by construction) | The mock is the deterministic proof vehicle: it drains the steering channel at a boundary and reflects the injected message in its event stream — no process, no tokens. | Fully verifiable. |

The upshot: the **engine mechanism** is fully deliverable and provable (via the mock); the
**Claude Code live path** is buildable but only compile-/unit-testable here (message formatting +
opt-in gating), with live end-to-end verification deferred. Where a backend can't do it, we say so.

## Engine steering channel

Mirror the existing cancellation seam. Today each backend carries
`cancel: Option<Arc<AtomicBool>>`; a watcher thread polls it and kills the process on Stop. Add a
symmetric, optional **steering handle**:

```rust
/// A thread-safe queue of steering messages the caller pushes and a running turn drains.
pub type Steer = std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>;
```

- The caller (the engine's run path, via `build_backend`) creates the `Steer`, keeps a clone to
  **push** steering messages onto, and passes a clone to the backend — exactly how `cancel` is
  threaded today.
- The backend **drains** it at a *safe boundary*: for the mock, between its scripted steps; for
  Claude, between output lines in the stdout read loop (`for line in reader.lines()`), writing any
  drained message to the child's stdin.
- Off by default: `steer: None` (as `cancel: None` today) means byte-identical behaviour to now.

An `mpsc::Receiver<String>` is the alternative, but `Arc<Mutex<VecDeque>>` matches the existing
`Arc<AtomicBool>` idiom, is trivially cloneable for the multi-thread drain, and lets the caller push
from anywhere without owning a `Sender`.

### Mock demonstration (the provable core)

`MockBackend::run_turn` gains: at one of its boundaries, drain the steer queue; for each drained
message, emit an `AgentEvent::Message { text: format!("steering: {msg}") }` (or fold it into its
plan). Unit test: construct a mock with a `Steer` pre-loaded with `"focus on tests"`, run it, assert
the event stream contains the steer; with an empty/None steer, the output is unchanged. Deterministic,
no tokens — this is criterion 1.

## Claude Code wiring (S3 — additive + opt-in)

Behind an explicit opt-in (`LECTERN_STEER=1` or a backend flag), and **only** then:

1. Spawn with `stdin(Stdio::piped())` and add `--input-format stream-json` (keep the existing
   `--output-format stream-json --verbose`; consider `--replay-user-messages`). Send the initial
   task as the first NDJSON user message instead of `-p`.
2. A small writer thread drains the `Steer` queue and writes each message as an NDJSON user
   message to the child's stdin (one JSON object per line, newline + flush), until the turn ends.
3. **Default path untouched:** when the opt-in is off, spawn exactly as today (`-p <prompt>`,
   `stdin` null). No regression to the working headless path.

The **exact user-message envelope** (`{"type":"user","message":{"role":"user","content":[{"type":
"text","text":"…"}]}}` is the SDK-standard shape) is a documented gap, so S3 validates it against the
CLI/SDK before shipping, and its behaviour is unit-tested at the message-construction level only —
**live verification is explicitly deferred** because it requires spending tokens on a real model.

## Deliberate scope + honesty

- Criteria 1, 3, 4, 5 (engine channel + mock proof, backend matrix, non-invasive, hygiene) are fully
  deliverable and verifiable here.
- Criterion 2 (the Claude live path) is delivered as **compile- and unit-tested, opt-in plumbing**
  with a labelled deferral for live verification. If rewiring Claude's I/O proves too risky to land
  without breaking the working `-p` path, the fallback is to ship the engine channel + mock + this
  design and leave the Claude live path as a ready-to-build spec — an honest partial, not a blind
  rewrite.

## Slice map (mirrors MISSION.md)

S2 engine `Steer` channel + mock demonstration (provable core) · S3 Claude Code stream-json input
plumbing (opt-in, additive, unit-tested) · S4 a way to send a steer + docs/CHANGELOG + audit.

## Sources

- Run Claude Code programmatically (headless) — https://code.claude.com/docs/en/headless
- `--input-format stream-json` is under-documented (the gap) — https://github.com/anthropics/claude-code/issues/24594
- Reverse-engineered CLI protocol — https://github.com/Roasbeef/claude-agent-sdk-go/blob/main/docs/cli-protocol.md
- Mid-turn steering as a 2026 norm — https://kiro.dev/docs/cli/chat/queue-steering/
