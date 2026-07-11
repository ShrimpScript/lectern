//! `lecternd` — the engine daemon. Opens a Unix domain socket and speaks
//! line-delimited JSON-RPC: `status`/`ping` for discovery, and `run`/`cancel`
//! for full agent sessions with the AgentEvent stream as notifications — the
//! IPC the TUI (and any future client) drives. See
//! Lectern-Brain/03-Architecture/{Engine Daemon (lecternd),Local API & IPC}.md.
use anyhow::Result;
mod transport {
    //! Mission ports P2a: one daemon protocol, per-OS transport.
    //! unix → the existing XDG unix socket (permissions do the auth).
    //! windows → 127.0.0.1 TCP with an ephemeral port + a 0600 token file in
    //! ~/.lectern (std timeouts everywhere; localhost-only bind; every request
    //! must carry the token — checked in handle()).
    use std::io::{Read, Write};
    use std::time::Duration;

    pub trait Duplex: Read + Write + Send {
        fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()>;
        fn try_clone_box(&self) -> std::io::Result<Box<dyn Duplex>>;
    }

    #[cfg(unix)]
    impl Duplex for std::os::unix::net::UnixStream {
        fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
            std::os::unix::net::UnixStream::set_read_timeout(self, d)
        }
        fn try_clone_box(&self) -> std::io::Result<Box<dyn Duplex>> {
            Ok(Box::new(self.try_clone()?))
        }
    }

    impl Duplex for std::net::TcpStream {
        fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
            std::net::TcpStream::set_read_timeout(self, d)
        }
        fn try_clone_box(&self) -> std::io::Result<Box<dyn Duplex>> {
            Ok(Box::new(self.try_clone()?))
        }
    }

    #[cfg_attr(unix, allow(dead_code))] // Tcp arm is Windows-only at runtime
    pub enum AnyListener {
        #[cfg(unix)]
        Unix(std::os::unix::net::UnixListener),
        Tcp(std::net::TcpListener),
    }

    impl AnyListener {
        pub fn accept_boxed(&self) -> std::io::Result<Box<dyn Duplex>> {
            match self {
                #[cfg(unix)]
                AnyListener::Unix(l) => l.accept().map(|(s, _)| Box::new(s) as Box<dyn Duplex>),
                AnyListener::Tcp(l) => l.accept().map(|(s, _)| Box::new(s) as Box<dyn Duplex>),
            }
        }
    }

    /// Windows sidecar files: where clients find the port + the shared secret.
    #[cfg_attr(unix, allow(dead_code))]
    pub fn tcp_meta_paths() -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = lectern_engine::data_dir();
        (dir.join("lecternd.port"), dir.join("lecternd.token"))
    }

    #[cfg_attr(unix, allow(dead_code))]
    pub fn fresh_token() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        // Not a KDF target — a per-boot shared secret gating localhost TCP.
        let seed = format!(
            "{}-{}-{:?}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            std::thread::current().id()
        );
        lectern_engine::securebundle::seal(&seed, "lectern-token-pad")
            .map(|s| {
                s.lines()
                    .last()
                    .unwrap_or_default()
                    .chars()
                    .rev()
                    .take(40)
                    .collect::<String>()
            })
            .unwrap_or(seed)
    }
}
use lectern_engine::backend::Backend;
use lectern_engine::{
    AntigravityBackend, ClaudeCodeBackend, Engine, MockBackend, OpenCodeBackend, RunOptions,
};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use transport::{AnyListener, Duplex};

/// Live runs by id — lets a second connection cancel a streaming run.
fn runs() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Mirror of the desktop's backend builder: id → configured backend with the
/// run's cancel flag attached. "auto" = Claude Code when present, else mock.
fn build_backend(
    name: &str,
    yolo: bool,
    model: Option<String>,
    cancel: Arc<AtomicBool>,
) -> Box<dyn Backend> {
    let claude = |model: Option<String>| -> Box<dyn Backend> {
        Box::new(ClaudeCodeBackend {
            model,
            skip_permissions: yolo,
            cancel: Some(cancel.clone()),
            ..ClaudeCodeBackend::new()
        })
    };
    match name {
        "claude-code" | "claude" => claude(model),
        "antigravity" | "gemini" => Box::new(AntigravityBackend {
            model,
            skip_permissions: yolo,
            cancel: Some(cancel.clone()),
            ..AntigravityBackend::new()
        }),
        "opencode" | "openrouter" | "ollama" => Box::new(OpenCodeBackend {
            model,
            cancel: Some(cancel.clone()),
            ..OpenCodeBackend::new()
        }),
        "mock" => Box::new(MockBackend { fast: true }),
        _ => {
            if ClaudeCodeBackend::new().available() {
                claude(model)
            } else {
                Box::new(MockBackend { fast: true })
            }
        }
    }
}

#[cfg(unix)]
fn socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("lectern").join("lecternd.sock")
}

/// Non-unix: "the socket" is the TCP meta pair; this path is only used for the
/// stale-file cleanup + log line, so point it at the port file.
#[cfg(not(unix))]
fn socket_path() -> PathBuf {
    transport::tcp_meta_paths().0
}

/// True when a live daemon already answers on the socket — probed with a short
/// timeout so a dead socket file doesn't hang startup.
#[cfg(unix)]
fn daemon_alive(sock: &Path) -> bool {
    let Ok(stream) = UnixStream::connect(sock) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let Ok(mut writer) = stream.try_clone() else {
        return false;
    };
    if writeln!(writer, r#"{{"jsonrpc":"2.0","id":0,"method":"ping"}}"#).is_err() {
        return false;
    }
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    matches!(reader.read_line(&mut line), Ok(n) if n > 0 && line.contains("pong"))
}

#[cfg(not(unix))]
fn daemon_alive(_sock: &Path) -> bool {
    let (port_path, token_path) = transport::tcp_meta_paths();
    let (Ok(port), Ok(token)) = (
        std::fs::read_to_string(&port_path),
        std::fs::read_to_string(&token_path),
    ) else {
        return false;
    };
    let Ok(port) = port.trim().parse::<u16>() else {
        return false;
    };
    let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let Ok(mut writer) = stream.try_clone() else {
        return false;
    };
    if writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":0,"method":"ping","token":"{}"}}"#,
        token.trim()
    )
    .is_err()
    {
        return false;
    }
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    matches!(reader.read_line(&mut line), Ok(n) if n > 0 && line.contains("pong"))
}

fn main() -> Result<()> {
    let sock = socket_path();
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Single-instance: probe before touching the socket. Unconditionally deleting
    // it would silently orphan a live daemon and double the scheduler.
    if daemon_alive(&sock) {
        println!("lecternd already running on {}", sock.display());
        return Ok(());
    }
    // No live daemon — clear a stale socket file.
    let _ = std::fs::remove_file(&sock);

    #[cfg(unix)]
    let listener = AnyListener::Unix(UnixListener::bind(&sock)?);
    #[cfg(not(unix))]
    let listener = {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = l.local_addr()?.port();
        let (port_path, token_path) = transport::tcp_meta_paths();
        std::fs::create_dir_all(port_path.parent().unwrap())?;
        std::fs::write(&port_path, port.to_string())?;
        std::fs::write(&token_path, transport::fresh_token())?;
        AnyListener::Tcp(l)
    };
    println!("lecternd v{} — engine daemon", env!("CARGO_PKG_VERSION"));
    println!("listening on {}", sock.display());
    // Background scheduler: runs due / auto-continue tasks on a loop.
    std::thread::spawn(scheduler_loop);
    println!("scheduler: checking for due tasks every 30s");
    // Optional, opt-in, loopback-only A2A (Agent2Agent) endpoint. Off unless the
    // user asks for it (LECTERN_A2A / LECTERN_A2A_ADDR). See docs/a2a-design.md.
    if let Some(addr) = a2a::configured_addr() {
        std::thread::spawn(move || a2a::serve(addr));
    }
    println!("(Ctrl-C to stop)");

    loop {
        match listener.accept_boxed() {
            // One thread per connection — a slow or silent client must never
            // block the accept loop (an idle read also times out).
            Ok(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(300)));
                std::thread::spawn(move || {
                    if let Err(e) = handle(s) {
                        eprintln!("connection error: {e:#}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e:#}"),
        }
    }
}

/// Periodically run due schedules (one-shot tasks + auto-continue retries).
/// Failures are logged, never swallowed — a broken engine open or a run error
/// would otherwise leave "why didn't my task fire?" undiagnosable.
fn scheduler_loop() {
    let interval = Duration::from_secs(30);
    loop {
        match lectern_engine::Engine::open_default() {
            Ok(engine) => match engine.run_due_schedules(3600, |_ev| {}) {
                Ok(ran) => {
                    if !ran.is_empty() {
                        eprintln!("[scheduler] ran {} due task(s)", ran.len());
                    }
                }
                Err(e) => eprintln!("[scheduler] running due tasks failed: {e:#}"),
            },
            Err(e) => eprintln!("[scheduler] engine open failed: {e:#}"),
        }
        std::thread::sleep(interval);
    }
}

/// Execute a full agent session on this connection: emit a `started`
/// notification (with the cancellable run_id), stream every AgentEvent as an
/// `event` notification, and return the summary as the request's result.
fn handle_run(req: &serde_json::Value, writer: &mut (dyn Duplex + '_)) -> serde_json::Value {
    let p = req.get("params").cloned().unwrap_or_default();
    let get = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let prompt = get("prompt");
    if prompt.trim().is_empty() {
        return serde_json::json!({ "error": "prompt required" });
    }
    let path = {
        let raw = get("path");
        if raw.trim().is_empty() {
            ".".into()
        } else {
            raw
        }
    };
    let backend_id = {
        let raw = get("backend");
        if raw.trim().is_empty() {
            "auto".into()
        } else {
            raw
        }
    };
    let model = p
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    let apply = p.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
    let yolo = p.get("yolo").and_then(|v| v.as_bool()).unwrap_or(false);
    let conduct = p.get("conduct").and_then(|v| v.as_bool()).unwrap_or(false);

    let run_id = format!(
        "{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut m) = runs().lock() {
        m.insert(run_id.clone(), cancel.clone());
    }
    let _ = writeln!(
        writer,
        "{}",
        serde_json::json!({ "jsonrpc": "2.0", "method": "started", "params": { "run_id": run_id } })
    );
    let _ = writer.flush();

    let finish = |m: serde_json::Value| {
        if let Ok(mut r) = runs().lock() {
            r.remove(&run_id);
        }
        m
    };
    let engine = match Engine::open_default() {
        Ok(e) => e,
        Err(e) => return finish(serde_json::json!({ "error": format!("engine: {e:#}") })),
    };
    let ws = match engine.open_workspace(Path::new(&path)) {
        Ok(w) => w,
        Err(e) => return finish(serde_json::json!({ "error": format!("workspace: {e:#}") })),
    };
    let mut sink = |ev: lectern_engine::event::AgentEvent| {
        if let Ok(v) = serde_json::to_value(&ev) {
            let _ = writeln!(
                writer,
                "{}",
                serde_json::json!({ "jsonrpc": "2.0", "method": "event", "params": v })
            );
            let _ = writer.flush();
        }
    };
    let outcome = if conduct {
        let cancel2 = cancel.clone();
        let yolo2 = yolo;
        let make = move |b: &str, m: Option<String>| build_backend(b, yolo2, m, cancel2.clone());
        engine.run_conductor(&ws, &prompt, &make, apply, &mut sink)
    } else {
        let backend = build_backend(&backend_id, yolo, model, cancel.clone());
        engine.run(
            &ws,
            &prompt,
            backend.as_ref(),
            RunOptions {
                apply,
                worktree: false,
            },
            &mut sink,
        )
    };
    match outcome {
        Ok(res) => finish(serde_json::json!({
            "run_id": run_id,
            "session_id": res.session_id,
            "changes": res.changes.len(),
            "applied": res.applied,
            "limit_hit": res.limit_hit,
            "input_tokens": res.usage.input_tokens,
            "output_tokens": res.usage.output_tokens,
        })),
        Err(e) => finish(serde_json::json!({ "run_id": run_id, "error": format!("{e:#}") })),
    }
}

fn handle(stream: Box<dyn Duplex>) -> Result<()> {
    let peer = stream.try_clone_box()?;
    let mut reader = BufReader::new(stream);
    let mut writer = peer;
    // Windows TCP transport: every request must carry the daemon token
    // (unix sockets skip this — filesystem permissions are the auth).
    #[cfg(not(unix))]
    let expected_token = {
        let (_, token_path) = transport::tcp_meta_paths();
        std::fs::read_to_string(token_path).unwrap_or_default()
    };
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let req: serde_json::Value =
            serde_json::from_str(line.trim()).unwrap_or(serde_json::json!({}));
        #[cfg(not(unix))]
        if req.get("token").and_then(|t| t.as_str()) != Some(expected_token.trim()) {
            writeln!(
                writer,
                r#"{{"jsonrpc":"2.0","id":null,"result":{{"error":"bad or missing daemon token"}}}}"#
            )?;
            writer.flush()?;
            return Ok(());
        }
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        // `run` streams AgentEvents as notifications on this connection, then
        // answers the request id with a summary — handled outside the match.
        if method == "run" {
            let result = handle_run(&req, &mut *writer);
            let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
            writeln!(writer, "{resp}")?;
            writer.flush()?;
            line.clear();
            continue;
        }
        let result = match method {
            "status" => serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "data_dir": lectern_engine::data_dir().to_string_lossy(),
            }),
            "ping" => serde_json::json!("pong"),
            // ── TUI IPC: read-only session + model surface ──────
            "sessions" => {
                let p = req.get("params").cloned().unwrap_or_default();
                let path = p
                    .get("path")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(".")
                    .to_string();
                let limit = p.get("limit").and_then(|v| v.as_i64()).unwrap_or(30);
                match Engine::open_default().and_then(|e| {
                    let ws = e.open_workspace(Path::new(&path))?;
                    e.sessions_with_meta(&ws, limit)
                }) {
                    // full objects incl. desktop-written meta (unification p3):
                    // clients render model/view/project chips from it
                    Ok(rows) => serde_json::Value::Array(rows),
                    Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                }
            }
            "history" => {
                let sid = req
                    .get("params")
                    .and_then(|p| p.get("session_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match Engine::open_default().and_then(|e| e.session_events(sid)) {
                    Ok(payloads) => {
                        let events: Vec<serde_json::Value> = payloads
                            .iter()
                            .filter_map(|t| serde_json::from_str(t).ok())
                            .collect();
                        serde_json::json!(events)
                    }
                    Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                }
            }
            "usage" => match Engine::open_default().and_then(|e| e.usage_stats()) {
                Ok(v) => v,
                Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
            },
            "mcp_overview" => lectern_engine::harness_mcp::harness_mcp_overview(),
            "session_rename" => {
                let p = req.get("params").cloned().unwrap_or_default();
                let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                let title = p
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if sid.is_empty() || title.is_empty() {
                    serde_json::json!({ "error": "session_id and title required" })
                } else {
                    match Engine::open_default().and_then(|e| e.rename_session(sid, &title)) {
                        Ok(()) => serde_json::json!({ "ok": true }),
                        Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                    }
                }
            }
            "session_pin" => {
                let p = req.get("params").cloned().unwrap_or_default();
                let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                let pinned = p.get("pinned").and_then(|v| v.as_bool()).unwrap_or(true);
                match Engine::open_default().and_then(|e| e.set_session_pinned(sid, pinned)) {
                    Ok(()) => serde_json::json!({ "ok": true, "pinned": pinned }),
                    Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                }
            }
            "skills" => {
                let path = req
                    .get("params")
                    .and_then(|p| p.get("path"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(".")
                    .to_string();
                match Engine::open_default().and_then(|e| {
                    let ws = e.open_workspace(Path::new(&path))?;
                    e.list_skills(&ws)
                }) {
                    Ok(skills) => serde_json::json!(skills
                        .into_iter()
                        .map(|sk| serde_json::json!({
                            "name": sk.name, "description": sk.description, "uses": sk.uses,
                            "triggers": sk.triggers,
                        }))
                        .collect::<Vec<_>>()),
                    Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                }
            }
            "brain" => {
                let path = req
                    .get("params")
                    .and_then(|p| p.get("path"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(".")
                    .to_string();
                match Engine::open_default().and_then(|e| {
                    let ws = e.open_workspace(Path::new(&path))?;
                    let sessions = e.recent_sessions(&ws, 1000)?.len();
                    let skills = e.list_skills(&ws)?.len();
                    Ok::<_, anyhow::Error>(serde_json::json!({
                        "sessions": sessions,
                        "skills": skills,
                        "graph": Path::new(&path).join("graphify-out").is_dir(),
                    }))
                }) {
                    Ok(v) => v,
                    Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
                }
            }
            "models" => {
                let claude: Vec<serde_json::Value> = lectern_engine::backend::discover_claude_models()
                    .into_iter()
                    .map(|(id, label)| serde_json::json!({ "id": id, "label": label, "backend": "claude-code" }))
                    .collect();
                let oc: Vec<serde_json::Value> = lectern_engine::backend::discover_opencode_models()
                    .into_iter()
                    .map(|(id, label)| serde_json::json!({ "id": id, "label": label, "backend": "opencode" }))
                    .collect();
                serde_json::json!({ "claude": claude, "opencode": oc })
            }
            "cancel" => {
                let rid = req
                    .get("params")
                    .and_then(|p| p.get("run_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match runs().lock().ok().and_then(|m| m.get(rid).cloned()) {
                    Some(flag) => {
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        serde_json::json!({ "cancelling": rid })
                    }
                    None => serde_json::json!({ "error": "unknown run_id" }),
                }
            }
            _ => serde_json::json!({ "error": format!("unknown method: {method}") }),
        };
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
        writeln!(writer, "{resp}")?;
        writer.flush()?;
        line.clear();
    }
    Ok(())
}

/// The opt-in, loopback-only A2A (Agent2Agent) endpoint. Off by default; enabling
/// is explicit and the bind is refused if it is not loopback. See docs/a2a-design.md.
mod a2a {
    use lectern_engine::a2a::A2aService;
    use std::io::Read;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tiny_http::{Method, Request, Response, StatusCode};

    /// Where to bind, if A2A is enabled. Enabled when `LECTERN_A2A` is truthy or
    /// `LECTERN_A2A_ADDR` is set; defaults to `127.0.0.1:41041`. Any non-loopback
    /// address is refused — A2A is loopback-only by design.
    pub fn configured_addr() -> Option<SocketAddr> {
        let enabled = std::env::var("LECTERN_A2A")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let addr_env = std::env::var("LECTERN_A2A_ADDR").ok();
        if !enabled && addr_env.is_none() {
            return None;
        }
        let addr_str = addr_env.unwrap_or_else(|| "127.0.0.1:41041".to_string());
        let addr: SocketAddr = match addr_str.trim().parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[a2a] invalid LECTERN_A2A_ADDR '{addr_str}': {e}; A2A disabled");
                return None;
            }
        };
        if !addr.ip().is_loopback() {
            eprintln!("[a2a] refusing non-loopback bind {addr}; A2A is loopback-only");
            return None;
        }
        Some(addr)
    }

    fn json_header() -> tiny_http::Header {
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("static content-type header is valid")
    }

    /// Largest inbound request body we read (JSON-RPC messages are small).
    const MAX_BODY: u64 = 256 * 1024;

    /// Run a Lectern turn for an inbound A2A message and return the agent's reply
    /// text. Uses the workspace at the daemon's cwd. Never auto-applies changes —
    /// a remote peer's task returns proposed changes, it does not write the disk.
    /// The backend is `auto` by default; `LECTERN_A2A_BACKEND=mock` forces the
    /// no-cost mock (used for end-to-end tests).
    fn run_turn(
        backend_id: &str,
        prompt: &str,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<String> {
        use lectern_engine::event::AgentEvent as E;
        use std::path::Path;

        let engine = super::Engine::open_default()?;
        let ws = engine.open_workspace(Path::new("."))?;
        let backend = super::build_backend(backend_id, false, None, cancel);
        let mut reply = String::new();
        let res = engine.run(
            &ws,
            prompt,
            backend.as_ref(),
            super::RunOptions {
                apply: false,
                worktree: false,
            },
            |ev| match ev {
                E::Message { text } => {
                    if !reply.is_empty() {
                        reply.push('\n');
                    }
                    reply.push_str(&text);
                }
                E::MessageDelta { text } => reply.push_str(&text),
                _ => {}
            },
        )?;
        if reply.trim().is_empty() {
            reply = format!("Completed. {} proposed change(s).", res.changes.len());
        }
        Ok(reply)
    }

    /// Dispatch one HTTP request: the agent card (GET), a JSON-RPC call (POST
    /// /a2a), or 404.
    fn dispatch(mut request: Request, service: &A2aService, card_json: &str) {
        let is_get = request.method() == &Method::Get;
        let is_post = request.method() == &Method::Post;
        let url = request.url().to_string();
        let resp = if is_get && url == "/.well-known/agent-card.json" {
            Response::from_string(card_json.to_string()).with_header(json_header())
        } else if is_post && url == "/a2a" {
            let mut body = String::new();
            let _ = request.as_reader().take(MAX_BODY).read_to_string(&mut body);
            let out = service.handle(&body).to_string();
            Response::from_string(out).with_header(json_header())
        } else {
            Response::from_string("not found").with_status_code(StatusCode(404))
        };
        let _ = request.respond(resp);
    }

    /// Serve the A2A endpoint until the process exits: the agent card plus the
    /// `message/send` / `tasks/get` JSON-RPC methods, each on its own thread so a
    /// long-running turn never blocks discovery.
    pub fn serve(addr: SocketAddr) {
        let server = match tiny_http::Server::http(addr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[a2a] failed to bind {addr}: {e}");
                return;
            }
        };
        let endpoint = format!("http://{addr}/a2a");
        let card = lectern_engine::a2a::agent_card(env!("CARGO_PKG_VERSION"), &endpoint);
        let card_json = serde_json::to_string(&card).unwrap_or_default();
        let backend_id =
            std::env::var("LECTERN_A2A_BACKEND").unwrap_or_else(|_| "auto".to_string());
        let service = Arc::new(A2aService::new(move |prompt: &str, cancel| {
            run_turn(&backend_id, prompt, cancel)
        }));
        println!("a2a: endpoint on http://{addr} (loopback-only, opt-in)");

        for request in server.incoming_requests() {
            let service = service.clone();
            let card_json = card_json.clone();
            std::thread::spawn(move || dispatch(request, &service, &card_json));
        }
    }
}
