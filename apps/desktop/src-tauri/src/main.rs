//! Lectern desktop (Tauri v2). The Rust host embeds the Lectern engine and exposes
//! it to the React UI. Sessions stream their normalized [`AgentEvent`]s to the
//! window live via an IPC channel, so the Claude Code backend's thinking, tool
//! calls, terminal output, and file edits appear as they happen.
//! See Lectern-Brain/03-Architecture/Desktop App Stack.md.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lectern_engine::backend::Backend;
use lectern_engine::{AntigravityBackend, ClaudeCodeBackend, Engine, MockBackend, OpenCodeBackend, RunOptions};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::Channel;

/// Per-session cancel flags for in-flight runs (set true by `cancel_session`).
fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Backends the UI can offer, with live availability for this machine.
#[derive(serde::Serialize)]
struct BackendInfo {
    id: String,
    label: String,
    available: bool,
    detail: String,
}

/// Setup status for the UI's doctor/banner.
#[derive(serde::Serialize)]
struct Doctor {
    claude_available: bool,
    claude_version: Option<String>,
    default_dir: String,
}

/// Summary returned when a streamed session finishes.
#[derive(serde::Serialize)]
struct RunSummary {
    session_id: String,
    changes: usize,
    applied: bool,
    limit_hit: bool,
    input_tokens: u64,
    output_tokens: u64,
}

/// A top-level entry in the workspace file tree.
#[derive(serde::Serialize)]
struct FileEntry {
    name: String,
    dir: bool,
}

/// A learned skill for the workspace (powers the Marketplace + Settings screens).
#[derive(serde::Serialize)]
struct SkillInfo {
    name: String,
    description: String,
    triggers: Vec<String>,
    uses: i64,
    steps: Vec<String>,
    rules: Vec<String>,
    /// True for recorded GUI macros — these replay deterministically (no agent).
    gui: bool,
    /// Outcome stats (zero-token self-regulation): runs where this skill was
    /// auto-applied that ended clean vs. errored, and whether it paused itself.
    ok: u32,
    err: u32,
    paused: bool,
}

/// Cloud account status for the Profile screen.
#[derive(serde::Serialize)]
struct AccountInfo {
    signed_in: bool,
    base_url: Option<String>,
    plan: Option<String>,
}

/// A scheduled agent run (for the Schedule screen).
#[derive(serde::Serialize)]
struct ScheduleInfo {
    id: String,
    prompt: String,
    backend: String,
    apply: bool,
    run_at: i64,
    reason: String,
    status: String,
}

/// Persisted desktop preferences (theme + session defaults), saved to XDG config.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Prefs {
    theme: String,
    default_backend: String,
    default_model: String,
    default_apply: bool,
    /// Whether the user has completed (or skipped) first-run onboarding. `#[serde(default)]`
    /// so existing prefs files load with `false` and show onboarding once.
    #[serde(default)]
    onboarded: bool,
    /// Clean output by default: machinery rows (thoughts/tools/routing) collapse
    /// into one expandable strip per turn.
    #[serde(default)]
    clean_output: bool,
    /// Active custom theme file name under ~/.lectern/themes.
    /// None/invalid = the immutable built-in Light/Dark.
    #[serde(default)]
    custom_theme: Option<String>,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            default_backend: "auto".into(),
            default_model: String::new(),
            default_apply: false,
            onboarded: false,
            clean_output: false,
            custom_theme: None,
        }
    }
}

fn prefs_path() -> std::path::PathBuf {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(lectern_engine::home_dir()).join(".config")
        });
    dir.join("lectern").join("desktop.json")
}

fn build_backend(
    name: &str,
    skip_permissions: bool,
    model: Option<String>,
    cancel: Arc<AtomicBool>,
) -> Box<dyn Backend> {
    let claude = |model: Option<String>| -> Box<dyn Backend> {
        Box::new(ClaudeCodeBackend {
            model,
            skip_permissions,
            cancel: Some(cancel.clone()),
            ..ClaudeCodeBackend::new()
        })
    };
    match name {
        "claude-code" | "claude" => claude(model),
        "antigravity" | "gemini" => Box::new(AntigravityBackend {
            model,
            skip_permissions,
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
            // "auto": Claude Code when present, else the mock pipeline.
            if ClaudeCodeBackend::new().available() {
                claude(model)
            } else {
                Box::new(MockBackend { fast: true })
            }
        }
    }
}

#[tauri::command]
async fn engine_backends()-> Vec<BackendInfo> {
    tauri::async_runtime::spawn_blocking(move || {
    // Probe the three real harnesses in parallel — serially this was
    // claude+agy+opencode ≈ 1.5s of boot latency; now it's max(single probe).
    let cc_t = std::thread::spawn(|| {
        let cc = ClaudeCodeBackend::new();
        (cc.available(), cc.version())
    });
    let agy_t = std::thread::spawn(|| AntigravityBackend::new().available());
    let oc_t = std::thread::spawn(|| {
        let oc = OpenCodeBackend::new();
        (oc.available(), oc.version())
    });
    let (claude_ok, claude_ver) = cc_t.join().unwrap_or((false, None));
    let agy_ok = agy_t.join().unwrap_or(false);
    let (oc_ok, oc_ver) = oc_t.join().unwrap_or((false, None));
    vec![
        BackendInfo {
            id: "auto".into(),
            label: "Auto".into(),
            available: true,
            detail: if claude_ok {
                "routes each task to the best model".into()
            } else {
                "connect a provider to enable routing".into()
            },
        },
        BackendInfo {
            id: "claude-code".into(),
            label: "Claude Code".into(),
            available: claude_ok,
            detail: claude_ver.unwrap_or_else(|| "not installed".into()),
        },
        BackendInfo {
            id: "antigravity".into(),
            label: "Antigravity (Gemini)".into(),
            available: agy_ok,
            detail: "Gemini via Antigravity CLI".into(),
        },
        BackendInfo {
            id: "opencode".into(),
            label: "OpenCode (many providers)".into(),
            available: oc_ok,
            detail: oc_ver
                .map(|v| format!("v{v} · OpenRouter + free models"))
                .unwrap_or_else(|| "install from opencode.ai".into()),
        },
        BackendInfo {
            id: "openrouter".into(),
            label: "OpenRouter".into(),
            available: oc_ok && !lectern_engine::backend::discover_openrouter_models().is_empty(),
            detail: if oc_ok {
                "via opencode — connect with `opencode auth login` (free models available)".into()
            } else {
                "install opencode first".into()
            },
        },
        {
            let ollama = lectern_engine::backend::discover_ollama_models();
            BackendInfo {
                id: "ollama".into(),
                label: "Ollama (local)".into(),
                available: !ollama.is_empty(),
                detail: if ollama.is_empty() {
                    "not running — start Ollama (ollama.com), then `ollama pull llama3`".into()
                } else {
                    format!("local · {} models detected", ollama.len())
                },
            }
        },
        BackendInfo {
            id: "mock".into(),
            label: "Mock".into(),
            available: true,
            detail: "offline demo pipeline".into(),
        },
    ]
})
    .await
        .unwrap_or_default()
}

#[derive(serde::Serialize)]
struct ModelEntry {
    id: String,
    label: String,
}

/// Claude models this account has actually used (from ~/.claude.json) — keeps the
/// model picker current when new models ship, with zero token spend.
#[tauri::command]
async fn claude_models()-> Vec<ModelEntry> {
    tauri::async_runtime::spawn_blocking(move || {
    lectern_engine::backend::discover_claude_models()
        .into_iter()
        .map(|(id, label)| ModelEntry { id, label })
        .collect()
})
    .await
        .unwrap()
}

/// OpenCode's built-in free models (from `opencode models`, no inference) —
/// zero-config entries for the model menu; more providers unlock via
/// `opencode auth login`.
#[tauri::command]
async fn opencode_models()-> Vec<ModelEntry> {
    tauri::async_runtime::spawn_blocking(move || {
    lectern_engine::backend::discover_opencode_models()
        .into_iter()
        .map(|(id, label)| ModelEntry { id, label })
        .collect()
})
    .await
        .unwrap()
}

#[tauri::command]
async fn doctor()-> Doctor {
    tauri::async_runtime::spawn_blocking(move || {
    let cc = ClaudeCodeBackend::new();
    Doctor {
        claude_available: cc.available(),
        claude_version: cc.version(),
        // Default to the user's home so the agent is usable immediately. The home dir
        // isn't auto-indexed (the engine skips it); the global brain still applies.
        default_dir: lectern_engine::home_dir(),
    }
})
    .await
        .unwrap()
}

/// Top-level files/folders of the workspace (heavy/hidden dirs filtered) — for the file tree.
#[tauri::command]
async fn list_dir(path: String)-> Vec<FileEntry> {
    tauri::async_runtime::spawn_blocking(move || {
    const IGNORE: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        ".next",
        ".data",
        "dist",
        "build",
        ".lectern",
    ];
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if IGNORE.contains(&name.as_str()) {
                continue;
            }
            let dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if name.starts_with('.') && dir {
                continue;
            }
            out.push(FileEntry { name, dir });
        }
    }
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out.truncate(500);
    out
})
    .await
        .unwrap()
}

/// The workspace's learned skills (subject-keyed memory). Empty until any are recorded.
#[tauri::command]
async fn skills(path: String)-> Vec<SkillInfo> {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(engine) = Engine::open_default() else {
        return vec![];
    };
    let Ok(ws) = engine.open_workspace(Path::new(&path)) else {
        return vec![];
    };
    let stats = lectern_engine::skillstats::load();
    engine
        .list_skills(&ws)
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            let st = stats.get(&s.name).cloned().unwrap_or_default();
            SkillInfo {
                gui: lectern_engine::gui_replay_steps(&s.body.steps).is_some(),
                ok: st.ok,
                err: st.err,
                paused: lectern_engine::skillstats::is_paused(&st),
                name: s.name,
                description: s.description,
                triggers: s.triggers,
                uses: s.uses,
                rules: s.body.rules,
                steps: s.body.steps,
            }
        })
        .collect()
})
    .await
        .unwrap()
}

/// Re-enable a paused skill: clears its outcome record (fresh start).
#[tauri::command]
async fn reset_skill_stats(name: String) {
    tauri::async_runtime::spawn_blocking(move || {
    lectern_engine::skillstats::reset(&name);
})
    .await
        .ok();
}

/// Routing-config snapshot for the Settings card (file self-writes defaults on first read).
#[tauri::command]
async fn routing_summary()-> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
    serde_json::to_value(lectern_engine::route::routing_summary()).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Open a Lectern-owned config file in the system editor (xdg-open).
#[tauri::command]
async fn open_config_file(path: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let allowed = path.starts_with(&format!(
        "{}/.lectern/",
        lectern_engine::home_dir()
    ));
    if !allowed {
        return Err("only ~/.lectern config files can be opened".into());
    }
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Remote-access channels: read-only status of Claude Code's
/// channels (~/.claude/channels/<name>/access.json). Counts + policy only —
/// Lectern NEVER edits access or approves pairings (that stays in the CLI,
/// deliberately: inbound remote messages are a prompt-injection surface).
#[derive(serde::Serialize)]
struct ChannelStatus {
    name: String,
    configured: bool,
    allowed: usize,
    pending: usize,
    dm_policy: String,
}

#[tauri::command]
async fn channels_status()-> Vec<ChannelStatus> {
    tauri::async_runtime::spawn_blocking(move || {
    let home = lectern_engine::home_dir();
    let dir = std::path::PathBuf::from(home).join(".claude").join("channels");
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let access = e.path().join("access.json");
            let parsed: Option<serde_json::Value> = std::fs::read_to_string(&access)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok());
            match parsed {
                Some(v) => out.push(ChannelStatus {
                    name,
                    configured: true,
                    allowed: v
                        .get("allowFrom")
                        .and_then(|a| a.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0),
                    pending: v
                        .get("pending")
                        .and_then(|p| p.as_object())
                        .map(|p| p.len())
                        .unwrap_or(0),
                    dm_policy: v
                        .get("dmPolicy")
                        .and_then(|d| d.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                }),
                None => out.push(ChannelStatus {
                    name,
                    configured: false,
                    allowed: 0,
                    pending: 0,
                    dm_policy: "not set up".into(),
                }),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
})
    .await
        .unwrap()
}

/// Write a chat export into the user's Downloads dir (no dialog dependency);
/// returns the full path for the UI toast. Filename is sanitized here.
fn themes_dir() -> Result<std::path::PathBuf, String> {
    let home = lectern_engine::home_dir();
    let dir = std::path::PathBuf::from(home).join(".lectern").join("themes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn safe_theme_name(filename: &str) -> Result<String, String> {
    let safe: String = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    if !safe.ends_with(".json") || safe.starts_with('.') {
        return Err("theme files are <name>.json".into());
    }
    Ok(safe)
}

/// List custom themes: name/base parsed out of each JSON.
#[tauri::command]
async fn list_themes()-> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(dir) = themes_dir() else { return vec![] };
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if !file.ends_with(".json") {
                continue;
            }
            let parsed: Option<serde_json::Value> = std::fs::read_to_string(e.path())
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok());
            let (name, base, valid) = match &parsed {
                Some(v) if v.get("lectern_theme").and_then(|x| x.as_i64()) == Some(1) => (
                    v.get("name").and_then(|n| n.as_str()).unwrap_or(&file).to_string(),
                    v.get("base").and_then(|b| b.as_str()).unwrap_or("light").to_string(),
                    true,
                ),
                _ => (file.clone(), "light".into(), false),
            };
            out.push(serde_json::json!({ "file": file, "name": name, "base": base, "valid": valid }));
        }
    }
    out.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    out
})
    .await
        .unwrap()
}

#[tauri::command]
async fn read_theme(file: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let dir = themes_dir()?;
    std::fs::read_to_string(dir.join(safe_theme_name(&file)?)).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Create/import a theme file. Refuses to overwrite unless `overwrite`.
#[tauri::command]
async fn save_theme_file(file: String, content: String, overwrite: bool)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let dir = themes_dir()?;
    let path = dir.join(safe_theme_name(&file)?);
    if path.exists() && !overwrite {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn save_chat_export(filename: String, content: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let safe: String = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    let home = lectern_engine::home_dir();
    let dir = std::path::PathBuf::from(&home).join("Downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(safe);
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Usage page data.
#[tauri::command]
async fn usage_stats()-> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    engine.usage_stats().map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/* Session-unification groundwork (Hermes-teardown top rec): the desktop can
   now read/write ENGINE-STORE sessions. Phase 1 = surface only; the UI still
   runs on its JSON list until the switchover slice. */
#[tauri::command]
async fn store_sessions(path: String, limit: i64)-> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
    let ws = eng.open_workspace(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    eng.sessions_with_meta(&ws, limit).map(serde_json::Value::Array).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_set_session_meta(session_id: String, meta_json: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
    eng.set_session_meta(&session_id, &meta_json).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_rename_session(session_id: String, title: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
    eng.rename_session(&session_id, &title).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_session_events(session_id: String)-> Result<Vec<serde_json::Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
    let payloads = eng.session_events(&session_id).map_err(|e| e.to_string())?;
    Ok(payloads.iter().filter_map(|t| serde_json::from_str(t).ok()).collect())
})
    .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_pin_session(session_id: String, pinned: bool)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
    eng.set_session_pinned(&session_id, pinned).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Publish-time audit: static red-flag rules + a $0 OpenCode
/// free-model second opinion. Gates the hub publish flow only — manual local
/// installs bypass it by design.
#[tauri::command]
async fn audit_skill(path: String, name: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let ws = engine
            .open_workspace(Path::new(&path))
            .map_err(|e| e.to_string())?;
        let bundle = engine.export_skill(&ws, &name).map_err(|e| e.to_string())?;
        serde_json::to_value(lectern_engine::audit::audit_bundle(&bundle)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Embedded terminal (Hermes-inspired): a real PTY per chat session ─────────
// Local backend v1. Output streams over a tauri CHANNEL — the same proven
// mechanism as run_session's event stream (the first version used emit/listen,
// the app's only listen() user, and the events never arrived in release —
// found by driving the real app). Reattach swaps the live channel.
struct PtyHandle {
    writer: Box<dyn std::io::Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    out: std::sync::Arc<std::sync::Mutex<tauri::ipc::Channel<serde_json::Value>>>,
}

fn ptys() -> &'static std::sync::Mutex<std::collections::HashMap<String, PtyHandle>> {
    static P: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, PtyHandle>>,
    > = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

/* Terminal backends (design: docs/terminal-backends-design.md). One spec,
   three spawn paths; the picker only ever shows engines DETECTED on this
   machine, so absent docker/sshd means no UI change at all. Lectern never
   touches credentials — docker/ssh use the user's own CLI auth. */
#[derive(Clone, Debug, PartialEq)]
enum TermEngine {
    Local,
    Docker(String),
    Ssh(String),
}

fn parse_term_engine(spec: &str) -> TermEngine {
    match spec.split_once(':') {
        Some(("docker", c)) if !c.is_empty() => TermEngine::Docker(c.to_string()),
        Some(("ssh", h)) if !h.is_empty() => TermEngine::Ssh(h.to_string()),
        _ => TermEngine::Local,
    }
}

/// Host aliases from an ssh config body (pure — unit-tested). Skips wildcard
/// and negated patterns; first token after `Host` per stanza, all stanzas.
fn ssh_config_hosts(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Host ").or_else(|| t.strip_prefix("Host\t")) {
            for h in rest.split_whitespace() {
                if !h.contains('*') && !h.contains('?') && !h.starts_with('!') && !out.contains(&h.to_string()) {
                    out.push(h.to_string());
                }
            }
        }
    }
    out
}

fn term_command_for(engine: &TermEngine, dir: &str) -> portable_pty::CommandBuilder {
    use portable_pty::CommandBuilder;
    let mut cmd = match engine {
        TermEngine::Local => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
            let mut c = CommandBuilder::new(shell);
            c.cwd(dir);
            c
        }
        TermEngine::Docker(container) => {
            // interactive shell in the container; fall back through sh
            let mut c = CommandBuilder::new("docker");
            c.args(["exec", "-it", container, "sh", "-c", "exec bash 2>/dev/null || exec sh"]);
            c
        }
        TermEngine::Ssh(host) => {
            let mut c = CommandBuilder::new("ssh");
            c.args(["-tt", host]);
            c
        }
    };
    cmd.env("TERM", "xterm-256color");
    cmd
}

/// Engines available on THIS machine: local always; docker containers when the
/// docker CLI answers quickly; ssh hosts from ~/.ssh/config. All probes are
/// time-capped so the drawer never stalls.
#[tauri::command]
async fn term_engines() -> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = vec![serde_json::json!({ "id": "local", "label": "This computer" })];
        if let Ok(o) = std::process::Command::new("timeout")
            .args(["3", "docker", "ps", "--format", "{{.Names}}"])
            .output()
        {
            if o.status.success() {
                for name in String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()) {
                    out.push(serde_json::json!({ "id": format!("docker:{name}"), "label": format!("Docker · {name}") }));
                }
            }
        }
        let ssh_cfg = std::path::PathBuf::from(lectern_engine::home_dir()).join(".ssh/config");
        if let Ok(body) = std::fs::read_to_string(ssh_cfg) {
            for h in ssh_config_hosts(&body) {
                out.push(serde_json::json!({ "id": format!("ssh:{h}"), "label": format!("SSH · {h}") }));
            }
        }
        out
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn term_open(
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
    engine: Option<String>,
    on_out: tauri::ipc::Channel<serde_json::Value>,
) -> Result<(), String> {
    use portable_pty::PtySize;
    {
        // Reattach: swap the drawer's fresh channel into the live reader.
        let map = ptys().lock().map_err(|e| e.to_string())?;
        if let Some(h) = map.get(&id) {
            if let Ok(mut ch) = h.out.lock() {
                *ch = on_out;
            }
            return Ok(());
        }
    }
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())?;
    let dir = if std::path::Path::new(&cwd).is_dir() {
        cwd
    } else {
        lectern_engine::home_dir()
    };
    let spec = parse_term_engine(engine.as_deref().unwrap_or("local"));
    let cmd = term_command_for(&spec, &dir);
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let out = std::sync::Arc::new(std::sync::Mutex::new(on_out));
    let out2 = out.clone();
    let id2 = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Ok(ch) = out2.lock() {
                        let _ = ch.send(serde_json::json!({ "kind": "out", "data": text }));
                    }
                }
            }
        }
        if let Ok(ch) = out2.lock() {
            let _ = ch.send(serde_json::json!({ "kind": "exit" }));
        }
        if let Ok(mut m) = ptys().lock() {
            m.remove(&id2);
        }
    });
    ptys().lock().map_err(|e| e.to_string())?.insert(
        id,
        PtyHandle { writer, master: pair.master, child, out },
    );
    Ok(())
}

#[tauri::command]
fn term_write(id: String, data: String) -> Result<(), String> {
    let mut map = ptys().lock().map_err(|e| e.to_string())?;
    let h = map.get_mut(&id).ok_or("no terminal")?;
    std::io::Write::write_all(&mut h.writer, data.as_bytes()).map_err(|e| e.to_string())?;
    std::io::Write::flush(&mut h.writer).map_err(|e| e.to_string())
}

#[tauri::command]
fn term_resize(id: String, cols: u16, rows: u16) -> Result<(), String> {
    let map = ptys().lock().map_err(|e| e.to_string())?;
    let h = map.get(&id).ok_or("no terminal")?;
    h.master
        .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn term_kill(id: String) {
    if let Ok(mut m) = ptys().lock() {
        if let Some(mut h) = m.remove(&id) {
            let _ = h.child.kill();
        }
    }
}

#[tauri::command]
async fn get_user_profile()-> String {
    tauri::async_runtime::spawn_blocking(move || {
    std::fs::read_to_string(lectern_engine::user_profile_path()).unwrap_or_default()
})
    .await
        .unwrap()
}

#[tauri::command]
async fn set_user_profile(content: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let path = lectern_engine::user_profile_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Create or replace a hand-authored skill (Marketplace "New skill" / edit).
#[tauri::command]
async fn create_skill(
    name: String,
    description: String,
    triggers: Vec<String>,
    rules: Vec<String>,
    steps: Vec<String>,
)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    engine
        .upsert_skill(&name, &description, triggers, rules, steps)
        .map(|_| ())
        .map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Export a skill as a portable JSON bundle.
#[tauri::command]
async fn export_skill(path: String, name: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    engine.export_skill(&ws, &name).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Save an exported skill bundle to a file the user picks. Returns the path, or None if cancelled.
#[tauri::command]
async fn save_skill_file(name: String, content: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let slug: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect();
        let out = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--save",
                "--confirm-overwrite",
                &format!("--filename={slug}.json"),
                "--title=Export skill",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Ok(None);
        }
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if p.is_empty() {
            return Ok(None);
        }
        std::fs::write(&p, content).map_err(|e| e.to_string())?;
        Ok(Some(p))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pick a `.json` skill bundle and import it. Returns the imported skill name, or None.
#[tauri::command]
async fn import_skill_file() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--file-filter=Skill bundles (*.json) | *.json",
                "--title=Import a skill",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Ok(None);
        }
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if p.is_empty() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let s = engine.import_skill(&json).map_err(|e| e.to_string())?;
        Ok(Some(s.name))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Browse the community hub (read-only, no auth). Returns the index entries.
#[tauri::command]
async fn browse_registry() -> Result<Vec<lectern_engine::registry::RegistryEntry>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        engine.browse_registry().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Fetch one community skill's full bundle so the UI can SHOW its exact
/// rules/steps before the user confirms (review-before-install). Download only —
/// nothing is imported or run.
#[tauri::command]
async fn fetch_registry_skill(
    id: String,
    sha256: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let (bundle, verified) = engine
            .fetch_registry_skill_verified(&id, sha256.as_deref())
            .map_err(|e| e.to_string())?;
        serde_json::to_value(serde_json::json!({ "bundle": bundle, "verified": verified }))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install a community skill by id (re-fetches from the hub, imports it, and
/// records the version). Call ONLY after the user has reviewed and confirmed.
#[tauri::command]
async fn install_registry_skill(id: String, sha256: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        engine
            .install_registry_skill(&id, sha256.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Map of hub skill id -> installed version, for "update available" badges.
#[tauri::command]
async fn registry_installed()-> std::collections::HashMap<String, u32> {
    tauri::async_runtime::spawn_blocking(move || {
    Engine::open_default()
        .map(|e| e.installed_registry_versions())
        .unwrap_or_default()
})
    .await
        .unwrap()
}

/// Publish one of the user's skills to the hub: build GitHub's prefilled
/// "propose new file" URL and open it in the browser (browser-PR — no token in
/// the app). Returns the URL (also opened) so the UI can offer a copy link.
#[tauri::command]
async fn publish_skill(path: String, name: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    let url = engine.publish_url(&ws, &name).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    Ok(url)
})
    .await
        .map_err(|e| e.to_string())?
}

/// The community hub repo page (for a "view on GitHub" link).
#[tauri::command]
async fn registry_repo_url()-> String {
    tauri::async_runtime::spawn_blocking(move || {
    Engine::open_default()
        .map(|e| e.registry_repo_url())
        .unwrap_or_default()
})
    .await
        .unwrap()
}

/// Materialize this workspace's learned skills as native Claude Code skills
/// (.claude/skills/lectern-*). Returns how many were written.
#[tauri::command]
async fn sync_skills(path: String)-> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    engine
        .sync_skills_to_claude(&ws, Path::new(&path))
        .map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Distill a reusable skill from this workspace's most recent session.
#[tauri::command]
async fn record_skill(path: String, name: Option<String>)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    let skill = engine
        .record_skill(&ws, None, name.as_deref())
        .map_err(|e| e.to_string())?;
    // Immediately make it available to Claude Code.
    let _ = engine.sync_skills_to_claude(&ws, Path::new(&path));
    Ok(skill.name)
})
    .await
        .map_err(|e| e.to_string())?
}

/// Delete a learned skill (from the brain) and re-sync so its .claude/skills file is removed.
#[tauri::command]
async fn delete_skill(path: String, name: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    engine.delete_skill(&name).map_err(|e| e.to_string())?;
    if let Ok(ws) = engine.open_workspace(Path::new(&path)) {
        let _ = engine.sync_skills_to_claude(&ws, Path::new(&path));
    }
    Ok(())
})
    .await
        .map_err(|e| e.to_string())?
}

/// A native skill the agent (Claude Code) already has, read from ~/.claude/skills.
#[derive(serde::Serialize)]
struct AgentSkill {
    name: String,
    description: String,
}

/// Read Claude Code's own user-level skills (~/.claude/skills/*/SKILL.md) so the UI
/// can surface them for tab-completion — distinct from Lectern's learned skills.
#[tauri::command]
async fn agent_skills()-> Vec<AgentSkill> {
    tauri::async_runtime::spawn_blocking(move || {
    let home = lectern_engine::home_dir();
    let dir = std::path::Path::new(&home).join(".claude").join("skills");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let Ok(content) = std::fs::read_to_string(e.path().join("SKILL.md")) else {
                continue;
            };
            let (mut name, mut desc, mut in_fm) = (String::new(), String::new(), false);
            for line in content.lines() {
                let t = line.trim();
                if t == "---" {
                    if in_fm {
                        break;
                    }
                    in_fm = true;
                    continue;
                }
                if in_fm {
                    if let Some(v) = t.strip_prefix("name:") {
                        name = v.trim().to_string();
                    } else if let Some(v) = t.strip_prefix("description:") {
                        desc = v.trim().to_string();
                    }
                }
            }
            if name.is_empty() {
                name = e.file_name().to_string_lossy().to_string();
            }
            out.push(AgentSkill {
                name,
                description: desc,
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.truncate(40);
    out
})
    .await
        .unwrap()
}

// ── MCP: surface + manage Claude Code's MCP servers from the Personal Agent ────
/// An MCP server Claude Code has configured (from `claude mcp list`).
#[derive(Clone, serde::Serialize)]
struct McpServer {
    name: String,
    detail: String,
    connected: bool,
    /// Truthful per-harness presence (C4b-4): read from each harness's own config.
    oc: bool,
    agy: bool,
}

/// Run `claude mcp <args>` with a resolved binary; returns stdout or the stderr error.
fn claude_mcp(args: &[&str]) -> Result<String, String> {
    let bin = lectern_engine::backend::resolve_claude("claude")
        .ok_or_else(|| "Claude Code not found".to_string())?;
    // Hard cap: `claude mcp list` pings every server and was measured at 37s
    // with two unreachable ones — cap the child so callers stay bounded.
    let out = std::process::Command::new("timeout")
        .arg("8")
        .arg(&bin)
        .arg("mcp")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        Err(if err.trim().is_empty() {
            String::from_utf8_lossy(&out.stdout).to_string()
        } else {
            err
        })
    }
}

/// The MCP servers Claude Code knows about (so the agent can use their tools natively).
/// Async + cached: the probe pings every server (slow, network-bound); we serve the
/// last good answer instantly and refresh it in the background, capped at 8s.
fn mcp_cache() -> &'static Mutex<Option<Vec<McpServer>>> {
    static C: OnceLock<Mutex<Option<Vec<McpServer>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn list_mcp_probe() -> Vec<McpServer> {
    let Ok(text) = claude_mcp(&["list"]) else {
        return vec![];
    };
    // Lines look like: "name: <command-or-url> - ✔ Connected" (or "✘ Failed to connect").
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Checking") {
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        let connected = rest.contains("Connected") && !rest.contains("Failed");
        let detail = rest
            .rsplit_once(" - ")
            .map(|(d, _)| d)
            .unwrap_or(rest)
            .trim()
            .to_string();
        let trimmed = name.trim().to_string();
        out.push(McpServer {
            oc: lectern_engine::harness_mcp::opencode_has(&trimmed),
            agy: lectern_engine::harness_mcp::antigravity_has(&trimmed),
            name: trimmed,
            detail,
            connected,
        });
    }
    out
}

#[tauri::command]
async fn list_mcp() -> Vec<McpServer> {
    let fresh = tauri::async_runtime::spawn_blocking(list_mcp_probe).await.unwrap_or_default();
    if !fresh.is_empty() {
        *mcp_cache().lock().unwrap() = Some(fresh.clone());
        return fresh;
    }
    // probe failed/timed out → last good beats empty
    mcp_cache().lock().unwrap().clone().unwrap_or_default()
}

/// Add an MCP server EVERYWHERE: Claude Code first (the primary —
/// its list drives the UI; a failure there aborts), then best-effort fan-out into
/// every other detected harness via the tested never-clobber writers. Returns a
/// per-harness map: "ok" | "not installed" | "skipped/error: reason".
#[tauri::command]
async fn add_mcp(
    name: String,
    command: String,
    env: Option<Vec<(String, String)>>,
)-> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let env_pairs = env.clone().unwrap_or_default();
    add_mcp_claude(&name, &command, env)?;
    let spec =
        lectern_engine::harness_mcp::McpSpec::parse(&name, &command, env_pairs);
    let opencode = if lectern_engine::harness_mcp::opencode_detected() {
        match lectern_engine::harness_mcp::merge_opencode(
            &lectern_engine::harness_mcp::opencode_config_path(),
            &spec,
        ) {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("error: {e}"),
        }
    } else {
        "not installed".to_string()
    };
    let antigravity = if lectern_engine::harness_mcp::antigravity_detected() {
        let paths = lectern_engine::harness_mcp::antigravity_config_paths();
        if paths.is_empty() {
            "no config dir".to_string()
        } else {
            let mut out = "ok".to_string();
            for p in paths {
                if let Err(e) = lectern_engine::harness_mcp::merge_antigravity(&p, &spec) {
                    out = format!("skipped: {e}");
                    break;
                }
            }
            out
        }
    } else {
        "not installed".to_string()
    };
    Ok(serde_json::json!({
        "claude": "ok",
        "opencode": opencode,
        "antigravity": antigravity,
    }))
})
    .await
        .map_err(|e| e.to_string())?
}

/// The Claude Code registration (primary harness) — extracted from the original add_mcp.
fn add_mcp_claude(name: &str, command: &str, env: Option<Vec<(String, String)>>) -> Result<(), String> {
    let (name, command) = (name.trim(), command.trim());
    if name.is_empty() || command.is_empty() {
        return Err("Name and command/URL are both required.".into());
    }
    let mut args: Vec<String> = vec!["add".into()];
    if command.starts_with("http://") || command.starts_with("https://") {
        // Remote transport takes no -e env flags. /sse endpoints (Asana,
        // Atlassian) use the sse transport; everything else streamable http.
        let transport = if command.trim_end_matches('/').ends_with("/sse") {
            "sse"
        } else {
            "http"
        };
        args.extend([
            "--transport".into(),
            transport.into(),
            name.into(),
            command.into(),
        ]);
    } else {
        for (k, v) in env.unwrap_or_default() {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() {
                args.push("-e".into());
                args.push(format!("{k}={v}"));
            }
        }
        args.push(name.into());
        args.push("--".into());
        args.extend(command.split_whitespace().map(str::to_string));
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    claude_mcp(&refs).map(|_| ())
}

/// Remove an MCP server by name — from Claude Code and, best-effort, from every
/// other harness config the fan-out may have written (C4b-3).
#[tauri::command]
async fn remove_mcp(name: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let name = name.trim();
    if name.is_empty() {
        return Err("No server name.".into());
    }
    let _ = lectern_engine::harness_mcp::remove_opencode(
        &lectern_engine::harness_mcp::opencode_config_path(),
        name,
    );
    for p in lectern_engine::harness_mcp::antigravity_config_paths() {
        let _ = lectern_engine::harness_mcp::remove_antigravity(&p, name);
    }
    claude_mcp(&["remove", name]).map(|_| ())
})
    .await
        .map_err(|e| e.to_string())?
}

/// One-click: register THIS app as an MCP server exposing Lectern's shared brain
/// (`<exe> mcp serve`), so Claude Code + the agent can query memory/skills via MCP.
#[tauri::command]
async fn connect_brain()-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    claude_mcp(&["add", "lectern-brain", "--", &exe, "mcp", "serve"]).map(|_| ())
})
    .await
        .map_err(|e| e.to_string())?
}

/// One-click: register graphify's code knowledge-graph as an MCP server for this
/// workspace, so the agent can query structure/dependencies (query_graph, get_node,
/// get_neighbors, shortest_path) instead of grepping. Requires the graph to be built.
#[tauri::command]
async fn connect_graphify(path: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let graph = std::path::Path::new(&path)
        .join("graphify-out")
        .join("graph.json");
    if !graph.exists() {
        return Err(
            "No code graph yet — build it first with `graphify extract .` in this workspace (or /graphify in Claude Code).".into(),
        );
    }
    // graphify-mcp is installed via pipx (~/.local/bin); fall back to PATH.
    let home = lectern_engine::home_dir();
    let local = format!("{home}/.local/bin/graphify-mcp");
    let bin = if std::path::Path::new(&local).exists() {
        local
    } else {
        "graphify-mcp".to_string()
    };
    let graph = graph.to_string_lossy().to_string();
    claude_mcp(&["add", "lectern-graphify", "--", &bin, &graph]).map(|_| ())
})
    .await
        .map_err(|e| e.to_string())?
}

/// A selectable model for the chat's model dropdown.
#[derive(serde::Serialize)]
struct ModelInfo {
    id: String,
    label: String,
}

/// Models available for the selected backend. Claude Code accepts these aliases;
/// an API-key backend would expose its own set (added when that backend lands).
#[tauri::command]
async fn models(backend: String)-> Vec<ModelInfo> {
    tauri::async_runtime::spawn_blocking(move || {
    // Antigravity harness — its exact CLI model strings (from `agy models`).
    if matches!(backend.as_str(), "antigravity" | "gemini") {
        return [
            ("", "Default"),
            ("Gemini 3.5 Flash (High)", "Gemini 3.5 Flash"),
            ("Gemini 3.1 Pro (High)", "Gemini 3.1 Pro"),
            ("GPT-OSS 120B (Medium)", "GPT-OSS 120B"),
        ]
        .into_iter()
        .map(|(id, label)| ModelInfo {
            id: id.into(),
            label: label.into(),
        })
        .collect();
    }
    if backend == "openrouter" {
        return lectern_engine::backend::discover_openrouter_models()
            .into_iter()
            .map(|(id, label)| ModelInfo { id, label })
            .collect();
    }
    if backend == "ollama" {
        return lectern_engine::backend::discover_ollama_models()
            .into_iter()
            .map(|(id, label)| ModelInfo { id, label })
            .collect();
    }
    let claude_ok = ClaudeCodeBackend::new().available();
    let uses_claude =
        matches!(backend.as_str(), "claude-code" | "claude") || (backend == "auto" && claude_ok);
    if !uses_claude {
        return vec![];
    }
    [
        ("auto", "Auto · smart routing"),
        ("", "Default"),
        ("opus", "Opus 4.8"),
        ("sonnet", "Sonnet 4.6"),
        ("haiku", "Haiku 4.5"),
    ]
    .into_iter()
    .map(|(id, label)| ModelInfo {
        id: id.into(),
        label: label.into(),
    })
    .collect()
})
    .await
        .unwrap_or_default()
}

/// Read a local image file as a data URL so the UI can display images the agent
/// produced/viewed (e.g. screenshots). Returns None for non-images or large/missing files.
#[tauri::command]
async fn read_image_b64(path: String)-> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", lectern_engine::home_dir(), rest)
    } else {
        path
    };
    let p = Path::new(&expanded);
    let mime = match p.extension()?.to_string_lossy().to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return None,
    };
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > 8_000_000 {
        return None;
    }
    let bytes = std::fs::read(p).ok()?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
})
    .await
        .unwrap()
}

// ── /record: capture an OS-level demonstration → distill into a skill ─────────
/// An in-flight recording: the xinput capture child + the steps captured so far.
struct Recording {
    child: std::process::Child,
    steps: Arc<Mutex<Vec<String>>>,
}
fn recorder() -> &'static Mutex<Option<Recording>> {
    static R: OnceLock<Mutex<Option<Recording>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

fn display() -> String {
    std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into())
}

/// Best-effort keycode→char map (lowercase) from `xmodmap -pke`, for typed text.
fn keymap() -> HashMap<u32, char> {
    let mut m = HashMap::new();
    let Ok(out) = std::process::Command::new("xmodmap")
        .arg("-pke")
        .env("DISPLAY", display())
        .output()
    else {
        return m;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() >= 4 && p[0] == "keycode" {
            if let Ok(code) = p[1].parse::<u32>() {
                if let Some(c) = keysym_char(p[3]) {
                    m.insert(code, c);
                }
            }
        }
    }
    m
}
fn keysym_char(ks: &str) -> Option<char> {
    match ks {
        "space" => Some(' '),
        "Return" | "KP_Enter" => Some('\n'),
        "period" => Some('.'),
        "comma" => Some(','),
        "minus" => Some('-'),
        "underscore" => Some('_'),
        "slash" => Some('/'),
        _ if ks.len() == 1 && ks.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) => {
            ks.chars().next()
        }
        _ => None,
    }
}
fn xdo(args: &[&str]) -> String {
    std::process::Command::new("xdotool")
        .args(args)
        .env("DISPLAY", display())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
fn active_window() -> String {
    xdo(&["getactivewindow", "getwindowname"])
}
fn mouse_xy() -> (i32, i32) {
    let s = xdo(&["getmouselocation"]);
    let pick = |key: &str| -> i32 {
        s.split(key)
            .nth(1)
            .and_then(|p| p.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (pick("x:"), pick("y:"))
}

/// Start capturing the user's input (clicks + typing + window switches) via xinput.
#[tauri::command]
fn start_recording() -> Result<(), String> {
    let mut guard = recorder().lock().unwrap();
    if guard.is_some() {
        return Err("Already recording.".into());
    }
    let mut child = std::process::Command::new("xinput")
        .args(["test-xi2", "--root"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env("DISPLAY", display())
        .spawn()
        .map_err(|e| format!("couldn't start capture (xinput): {e}"))?;
    let stdout = child.stdout.take().ok_or("no capture stream")?;
    let steps: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = steps.clone();
    std::thread::spawn(move || {
        let km = keymap();
        let mut cur: u32 = 0;
        let mut last_win = String::new();
        let mut typed = String::new();
        // Timing: capture the real delay before each action so replay matches the
        // user's pace (esp. the pause after a click before typing). Each step is
        // prefixed "[+Ns] " = seconds waited since the previous action.
        let mut last_at = std::time::Instant::now();
        let mut type_start: Option<std::time::Instant> = None;
        let gap = |from: std::time::Instant, to: std::time::Instant| -> f64 {
            to.saturating_duration_since(from)
                .as_secs_f64()
                .clamp(0.1, 10.0)
        };
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("EVENT type ") {
                cur = rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if let Some(d) = l.strip_prefix("detail: ") {
                let detail: u32 = d.trim().parse().unwrap_or(0);
                match cur {
                    15 => {
                        // RawButtonPress — flush any pending typing first (its delay is
                        // the wait before typing started: when the field focused).
                        let t = typed.trim();
                        if !t.is_empty() {
                            let g = gap(last_at, type_start.unwrap_or(last_at));
                            sink.lock().unwrap().push(format!(
                                "[+{:.1}s] Type \"{}\" in \"{}\"",
                                g,
                                t.replace('\n', "⏎"),
                                last_win
                            ));
                            last_at = std::time::Instant::now();
                        }
                        typed.clear();
                        type_start = None;
                        let now = std::time::Instant::now();
                        let win = active_window();
                        if !win.is_empty() && win != last_win {
                            sink.lock()
                                .unwrap()
                                .push(format!("[+{:.1}s] Switch to \"{win}\"", gap(last_at, now)));
                            last_at = now;
                            last_win = win.clone();
                        }
                        let (x, y) = mouse_xy();
                        let btn = if detail == 3 { "Right-click" } else { "Click" };
                        let now = std::time::Instant::now();
                        sink.lock().unwrap().push(format!(
                            "[+{:.1}s] {btn} at ({x}, {y}) in \"{win}\"",
                            gap(last_at, now)
                        ));
                        last_at = now;
                    }
                    13 => {
                        // RawKeyPress
                        if let Some(c) = km.get(&detail) {
                            if typed.is_empty() {
                                type_start = Some(std::time::Instant::now());
                            }
                            typed.push(*c);
                        }
                    }
                    _ => {}
                }
            }
        }
        let t = typed.trim();
        if !t.is_empty() {
            let g = gap(last_at, type_start.unwrap_or(last_at));
            sink.lock().unwrap().push(format!(
                "[+{:.1}s] Type \"{}\" in \"{}\"",
                g,
                t.replace('\n', "⏎"),
                last_win
            ));
        }
    });
    *guard = Some(Recording { child, steps });
    Ok(())
}

/// Whether a recording is currently in progress (for the UI indicator).
#[tauri::command]
fn recording_active() -> bool {
    recorder().lock().unwrap().is_some()
}

/// Stop the recording and return the captured steps.
#[tauri::command]
fn stop_recording() -> Result<Vec<String>, String> {
    let Some(mut rec) = recorder().lock().unwrap().take() else {
        return Err("Not recording.".into());
    };
    let _ = rec.child.kill();
    let _ = rec.child.wait();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut steps = rec.steps.lock().unwrap().clone();
    // The user moving to + clicking "Stop & save" lands on the Lectern window and gets
    // captured — drop those trailing actions so they aren't part of the skill.
    while steps
        .last()
        .is_some_and(|s| s.ends_with("in \"Lectern\"") || s.ends_with("Switch to \"Lectern\""))
    {
        steps.pop();
    }
    Ok(steps)
}

/// Save a recorded demonstration as a skill, then sync it to Claude Code.
#[tauri::command]
async fn save_recorded_skill(path: String, name: String, steps: Vec<String>)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    if steps.is_empty() {
        return Err("Nothing was captured.".into());
    }
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    // Auto-summary of what was captured (shown as the skill description / expand view).
    let clicks = steps.iter().filter(|s| s.contains("lick at (")).count();
    let types = steps.iter().filter(|s| s.contains("Type \"")).count();
    let mut windows: Vec<String> = steps
        .iter()
        .filter_map(|s| {
            s.rsplit("in \"")
                .next()
                .and_then(|w| w.strip_suffix('"'))
                .map(str::to_string)
        })
        .collect();
    windows.sort();
    windows.dedup();
    let where_ = if windows.is_empty() {
        "the desktop".to_string()
    } else {
        windows.join(", ")
    };
    let desc = format!(
        "Recorded GUI workflow — {} step(s) ({} click(s), {} text entr{}) in {}.",
        steps.len(),
        clicks,
        types,
        if types == 1 { "y" } else { "ies" },
        where_
    );
    let skill = engine
        .add_skill(&ws, &name, &desc, steps)
        .map_err(|e| e.to_string())?;
    let _ = engine.sync_skills_to_claude(&ws, Path::new(&path));
    Ok(skill.name)
})
    .await
        .map_err(|e| e.to_string())?
}

/// Cloud sign-in status + plan (best-effort; offline-tolerant). Runs off the UI thread.
#[tauri::command]
async fn account() -> AccountInfo {
    tauri::async_runtime::spawn_blocking(|| match lectern_engine::cloud::load_auth() {
        Some(auth) => {
            let plan = lectern_engine::cloud::get_entitlements(&auth)
                .ok()
                .and_then(|v| {
                    v.pointer("/token/plan")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string())
                });
            AccountInfo {
                signed_in: true,
                base_url: Some(auth.base_url),
                plan,
            }
        }
        None => AccountInfo {
            signed_in: false,
            base_url: None,
            plan: None,
        },
    })
    .await
    .unwrap_or(AccountInfo {
        signed_in: false,
        base_url: None,
        plan: None,
    })
}

/// Open a native multi-file picker (zenity, then kdialog) — for the 📎 attach button.
/// Returns absolute paths; images go to vision, other files are referenced by path.
#[tauri::command]
async fn pick_files() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = lectern_engine::home_dir();
        let split = |out: std::process::Output| -> Vec<String> {
            if !out.status.success() {
                return vec![];
            }
            String::from_utf8_lossy(&out.stdout)
                .split('\n')
                .flat_map(|l| l.split('|')) // zenity --multiple default separator is '|'
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        if let Ok(out) = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--multiple",
                "--title=Attach files",
                &format!("--filename={home}/"),
            ])
            .output()
        {
            return split(out);
        }
        if let Ok(out) = std::process::Command::new("kdialog")
            .args([
                "--getopenfilename",
                &home,
                "--multiple",
                "--separate-output",
            ])
            .output()
        {
            return split(out);
        }
        vec![]
    })
    .await
    .unwrap_or_default()
}

/// True if a file path looks like an image (so the UI shows a thumbnail + vision).
#[tauri::command]
fn is_image(path: String) -> bool {
    matches!(
        Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

// ── Voice dictation (functional, offline via faster-whisper) ──────────────────
/// The in-flight push-to-talk recording (a `parecord`/`arecord` child writing a wav).
fn dictation() -> &'static Mutex<Option<std::process::Child>> {
    static D: OnceLock<Mutex<Option<std::process::Child>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(None))
}
fn dictation_wav() -> String {
    format!(
        "{}/.cache/lectern/dictation.wav",
        lectern_engine::home_dir()
    )
}

/// Start recording from the default mic (16 kHz mono wav) for dictation.
#[tauri::command]
async fn start_dictation()-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let wav = dictation_wav();
    if let Some(parent) = Path::new(&wav).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut guard = dictation().lock().unwrap();
    if guard.is_some() {
        return Err("already recording".into());
    }
    // PulseAudio respects the system default input; fall back to ALSA arecord.
    let child = std::process::Command::new("parecord")
        .args([
            "--channels=1",
            "--rate=16000",
            "--format=s16le",
            "--file-format=wav",
            &wav,
        ])
        .spawn()
        .or_else(|_| {
            std::process::Command::new("arecord")
                .args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", &wav])
                .spawn()
        })
        .map_err(|e| format!("couldn't start recording (need parecord/arecord): {e}"))?;
    *guard = Some(child);
    Ok(())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Stop recording and transcribe the clip with faster-whisper → returns the text.
#[tauri::command]
async fn stop_dictation()-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let mut child = dictation()
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "not recording".to_string())?;
    // SIGTERM lets the recorder finalize the wav header (SIGKILL would corrupt it).
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let _ = child.wait();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let wav = dictation_wav();
    let py = format!(
        "{}/.cache/lectern-stt/bin/python",
        lectern_engine::home_dir()
    );
    if !Path::new(&py).exists() {
        return Err("Dictation engine not set up (faster-whisper venv missing).".into());
    }
    const SCRIPT: &str = "import sys\nfrom faster_whisper import WhisperModel\nm=WhisperModel('tiny.en',device='cpu',compute_type='int8')\nsegs,_=m.transcribe(sys.argv[1],beam_size=1)\nprint(' '.join(s.text.strip() for s in segs).strip())";
    let out = std::process::Command::new(&py)
        .arg("-c")
        .arg(SCRIPT)
        .arg(&wav)
        .output()
        .map_err(|e| format!("transcription failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Whether dictation is available on this machine (the STT venv exists).
#[tauri::command]
async fn dictation_available()-> bool {
    tauri::async_runtime::spawn_blocking(move || {
    let py = format!(
        "{}/.cache/lectern-stt/bin/python",
        lectern_engine::home_dir()
    );
    Path::new(&py).exists()
})
    .await
        .unwrap_or_default()
}

/// Open a native folder picker (zenity, then kdialog) and return the chosen path.
#[tauri::command]
async fn pick_folder() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = lectern_engine::home_dir();
        let take = |out: std::process::Output| -> Option<String> {
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .filter(|p| !p.is_empty())
        };
        // zenity (GTK), starting in the home folder; then kdialog (KDE).
        if let Ok(out) = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--directory",
                "--title=Open a project",
                &format!("--filename={home}/"),
            ])
            .output()
        {
            return take(out);
        }
        if let Ok(out) = std::process::Command::new("kdialog")
            .args(["--getexistingdirectory", &home])
            .output()
        {
            return take(out);
        }
        None
    })
    .await
    .ok()
    .flatten()
}

/// Load persisted desktop preferences (or defaults).
#[tauri::command]
fn get_prefs() -> Prefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist desktop preferences.
#[tauri::command]
fn set_prefs(prefs: Prefs) -> Result<(), String> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&prefs).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

fn sessions_path() -> std::path::PathBuf {
    prefs_path().with_file_name("sessions.json")
}

/// Load the persisted desktop session list (raw JSON; empty string if none yet).
#[tauri::command]
async fn get_sessions()-> String {
    tauri::async_runtime::spawn_blocking(move || {
    std::fs::read_to_string(sessions_path()).unwrap_or_default()
})
    .await
        .unwrap()
}

/// Persist the desktop session list (raw JSON serialized by the UI).
#[tauri::command]
async fn save_sessions(data: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let path = sessions_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, data).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

fn pastes_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(lectern_engine::home_dir())
        .join(".cache")
        .join("lectern")
        .join("pastes")
}

fn paste_ts() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Save a pasted image (raw base64, no data-URL prefix) to a cache dir and return
/// its absolute path, so it can be referenced in a prompt for Claude Code to read.
#[tauri::command]
async fn save_pasted_image(data: String, ext: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| e.to_string())?;
    let ext = if ext.is_empty() {
        "png".to_string()
    } else {
        ext
    };
    let dir = pastes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("paste-{}.{}", paste_ts(), ext));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// An image read from the OS clipboard: saved path (for Claude to read) + a data
/// URL (for the in-app preview).
#[derive(serde::Serialize)]
struct ClipImage {
    path: String,
    data_url: String,
}

/// Read raw PNG bytes from the OS clipboard via xclip (X11) then wl-paste (Wayland).
/// Needed because WebKitGTK's JS paste event doesn't expose clipboard images.
fn read_clip_png() -> Option<Vec<u8>> {
    if let Ok(t) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
        .output()
    {
        if String::from_utf8_lossy(&t.stdout).contains("image/png") {
            if let Ok(o) = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "image/png", "-o"])
                .output()
            {
                if o.status.success() && !o.stdout.is_empty() {
                    return Some(o.stdout);
                }
            }
        }
    }
    if let Ok(t) = std::process::Command::new("wl-paste")
        .arg("--list-types")
        .output()
    {
        if String::from_utf8_lossy(&t.stdout).contains("image/png") {
            if let Ok(o) = std::process::Command::new("wl-paste")
                .args(["--type", "image/png"])
                .output()
            {
                if o.status.success() && !o.stdout.is_empty() {
                    return Some(o.stdout);
                }
            }
        }
    }
    None
}

/// If the OS clipboard holds an image, save it and return its path + preview data URL.
#[tauri::command]
async fn read_clipboard_image()-> Option<ClipImage> {
    tauri::async_runtime::spawn_blocking(move || {
    use base64::Engine as _;
    let bytes = read_clip_png()?;
    let dir = pastes_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("paste-{}.png", paste_ts()));
    std::fs::write(&path, &bytes).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(ClipImage {
        path: path.to_string_lossy().to_string(),
        data_url: format!("data:image/png;base64,{b64}"),
    })
})
    .await
        .unwrap()
}

/// Scheduled agent runs for a workspace (soonest first).
#[tauri::command]
async fn list_schedules(path: String)-> Vec<ScheduleInfo> {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(engine) = Engine::open_default() else {
        return vec![];
    };
    let Ok(ws) = engine.open_workspace(Path::new(&path)) else {
        return vec![];
    };
    engine
        .list_schedules(&ws)
        .unwrap_or_default()
        .into_iter()
        .map(
            |(id, prompt, backend, apply, run_at, reason, status)| ScheduleInfo {
                id,
                prompt,
                backend,
                apply: apply != 0,
                run_at,
                reason,
                status,
            },
        )
        .collect()
})
    .await
        .unwrap()
}

/// All scheduled runs across every workspace (global Schedule view).
#[tauri::command]
fn app_meta() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "license": "Apache-2.0",
        "repo": "https://github.com/ShrimpScript/lectern",
    })
}

/// Open a URL in the system browser — restricted to Lectern's own public
/// pages so this can never become an arbitrary-open primitive.
#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    let allowed = url.starts_with("https://github.com/") || url.starts_with("https://getlectern.vercel.app/");
    if !allowed {
        return Err("URL not allowed".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" })
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clear_finished_schedules() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let eng = lectern_engine::Engine::open_default().map_err(|e| e.to_string())?;
        eng.clear_finished_schedules().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_all_schedules()-> Vec<ScheduleInfo> {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(engine) = Engine::open_default() else {
        return vec![];
    };
    engine
        .list_all_schedules()
        .unwrap_or_default()
        .into_iter()
        .map(
            |(id, prompt, backend, apply, run_at, reason, status)| ScheduleInfo {
                id,
                prompt,
                backend,
                apply: apply != 0,
                run_at,
                reason,
                status,
            },
        )
        .collect()
})
    .await
        .unwrap()
}

/// Schedule an agent run for `run_at` (unix seconds). Executed by the daemon when due.
#[tauri::command]
async fn schedule_add(
    path: String,
    prompt: String,
    backend: String,
    apply: bool,
    run_at: i64,
)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    let ws = engine
        .open_workspace(Path::new(&path))
        .map_err(|e| e.to_string())?;
    engine
        .schedule_add(
            &ws,
            &prompt,
            &backend,
            apply,
            run_at,
            "scheduled from desktop",
        )
        .map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Cancel a scheduled run by id.
#[tauri::command]
async fn cancel_schedule(id: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    let engine = Engine::open_default().map_err(|e| e.to_string())?;
    engine.cancel_schedule(&id).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// A node in the brain graph (kind: root | skill | trigger | memory | session).
#[derive(serde::Serialize)]
struct BrainNode {
    id: String,
    label: String,
    kind: String,
    weight: f64,
}
#[derive(serde::Serialize)]
struct BrainEdge {
    from: String,
    to: String,
}
#[derive(serde::Serialize)]
struct BrainGraph {
    nodes: Vec<BrainNode>,
    edges: Vec<BrainEdge>,
    skills: usize,
    memory: usize,
    sessions: usize,
}

/// Build the brain/memory graph for a workspace: skills + their shared trigger
/// keywords, indexed memory files, and recent sessions, all linked to a root node.
#[tauri::command]
async fn brain_graph(path: String)-> BrainGraph {
    tauri::async_runtime::spawn_blocking(move || {
    let root = Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".into());
    let mut nodes = vec![BrainNode {
        id: "root".into(),
        label: root,
        kind: "root".into(),
        weight: 3.0,
    }];
    let mut edges = Vec::new();
    let (mut n_skills, mut n_mem, mut n_sess) = (0usize, 0usize, 0usize);
    let blank = BrainGraph {
        nodes: vec![],
        edges: vec![],
        skills: 0,
        memory: 0,
        sessions: 0,
    };
    let Ok(engine) = Engine::open_default() else {
        return blank;
    };
    let Ok(ws) = engine.open_workspace(Path::new(&path)) else {
        return blank;
    };
    let mut trig_seen = std::collections::HashSet::new();
    for sk in engine.list_skills(&ws).unwrap_or_default() {
        n_skills += 1;
        let sid = format!("skill:{}", sk.name);
        nodes.push(BrainNode {
            id: sid.clone(),
            label: sk.name.clone(),
            kind: "skill".into(),
            weight: 1.2 + sk.uses as f64,
        });
        edges.push(BrainEdge {
            from: "root".into(),
            to: sid.clone(),
        });
        for t in sk.triggers.iter().take(6) {
            let tid = format!("trig:{t}");
            if trig_seen.insert(tid.clone()) {
                nodes.push(BrainNode {
                    id: tid.clone(),
                    label: t.clone(),
                    kind: "trigger".into(),
                    weight: 0.6,
                });
            }
            edges.push(BrainEdge {
                from: sid.clone(),
                to: tid,
            });
        }
    }
    for f in engine.memory_files(&ws, 24).unwrap_or_default() {
        n_mem += 1;
        let fid = format!("mem:{f}");
        let label = f.rsplit('/').next().unwrap_or(&f).to_string();
        nodes.push(BrainNode {
            id: fid.clone(),
            label,
            kind: "memory".into(),
            weight: 0.7,
        });
        edges.push(BrainEdge {
            from: "root".into(),
            to: fid,
        });
    }
    for (id, title, _backend, _created, _status) in
        engine.recent_sessions(&ws, 8).unwrap_or_default()
    {
        n_sess += 1;
        let sid = format!("sess:{id}");
        let label = if title.trim().is_empty() {
            "session".into()
        } else {
            title
        };
        nodes.push(BrainNode {
            id: sid.clone(),
            label,
            kind: "session".into(),
            weight: 0.8,
        });
        edges.push(BrainEdge {
            from: "root".into(),
            to: sid,
        });
    }
    BrainGraph {
        nodes,
        edges,
        skills: n_skills,
        memory: n_mem,
        sessions: n_sess,
    }
})
    .await
        .unwrap()
}

/// Run one agent session, streaming each normalized event to `on_event` as it
/// arrives. Runs on a blocking thread so the UI stays responsive and multiple
/// sessions can run concurrently.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn run_session(
    prompt: String,
    path: String,
    backend: String,
    apply: bool,
    skip_permissions: bool,
    model: Option<String>,
    session_id: String,
    on_event: Channel<serde_json::Value>,
) -> Result<RunSummary, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    running()
        .lock()
        .unwrap()
        .insert(session_id.clone(), cancel.clone());
    let sid = session_id.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let ws = engine
            .open_workspace(Path::new(&path))
            .map_err(|e| e.to_string())?;
        // Smart routing: "auto" picks the harness + model that excel at this task
        // (Claude or Gemini-via-Antigravity) and announces the choice.
        let (backend, model) = if model.as_deref() == Some("auto") {
            let mut r = lectern_engine::route::route_model(&prompt);
            // Availability fallback: routed Gemini but Antigravity not installed → Sonnet.
            if r.backend == "antigravity" && !AntigravityBackend::new().available() {
                r.reason = format!("{} (Gemini unavailable → Sonnet)", r.reason);
                r.backend = "claude-code".into();
                r.model = "sonnet".into();
                r.label = "Sonnet 4.6".into();
            }
            if let Ok(v) = serde_json::to_value(lectern_engine::event::AgentEvent::ModelRouted {
                model: r.label.clone(),
                reason: r.reason.clone(),
            }) {
                let _ = on_event.send(v);
            }
            (r.backend, Some(r.model))
        } else {
            (backend, model)
        };
        let be = build_backend(&backend, skip_permissions, model, cancel);
        let result = engine
            .run(
                &ws,
                &prompt,
                be.as_ref(),
                RunOptions {
                    apply,
                    worktree: false,
                },
                |ev| {
                    if let Ok(v) = serde_json::to_value(&ev) {
                        let _ = on_event.send(v);
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        // Best-effort content-free usage telemetry (counts only; no-op if signed out).
        engine.report_usage(
            be.id(),
            result.usage.input_tokens,
            result.usage.output_tokens,
        );
        Ok::<RunSummary, String>(RunSummary {
            session_id: result.session_id,
            changes: result.changes.len(),
            applied: result.applied,
            limit_hit: result.limit_hit,
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
        })
    })
    .await;
    running().lock().unwrap().remove(&sid);
    match out {
        Ok(inner) => inner,
        Err(e) => Err(e.to_string()),
    }
}

/// Run the Conductor on a session: plan the task, then hand each sub-task to its routed
/// model (Claude or Gemini), streaming the plan + per-step progress. Autonomous (steps
/// apply + skip permissions) so the orchestration actually executes.
#[tauri::command]
async fn run_conductor_session(
    prompt: String,
    path: String,
    session_id: String,
    on_event: Channel<serde_json::Value>,
) -> Result<RunSummary, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    running()
        .lock()
        .unwrap()
        .insert(session_id.clone(), cancel.clone());
    let sid = session_id.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let ws = engine
            .open_workspace(Path::new(&path))
            .map_err(|e| e.to_string())?;
        let cancel2 = cancel.clone();
        let make = move |b: &str, m: Option<String>| build_backend(b, true, m, cancel2.clone());
        let mut sink = |ev: lectern_engine::event::AgentEvent| {
            if let Ok(v) = serde_json::to_value(&ev) {
                let _ = on_event.send(v);
            }
        };
        let result = engine
            .run_conductor(&ws, &prompt, &make, true, &mut sink)
            .map_err(|e| e.to_string())?;
        engine.report_usage(
            "conductor",
            result.usage.input_tokens,
            result.usage.output_tokens,
        );
        Ok::<RunSummary, String>(RunSummary {
            session_id: result.session_id,
            changes: result.changes.len(),
            applied: result.applied,
            limit_hit: result.limit_hit,
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
        })
    })
    .await;
    running().lock().unwrap().remove(&sid);
    match out {
        Ok(inner) => inner,
        Err(e) => Err(e.to_string()),
    }
}

/// Status of the learned machine profile, for the Brain view.
#[derive(serde::Serialize)]
struct SystemProfileStatus {
    learned: bool,
    age_days: Option<u64>,
    preview: String,
}

#[tauri::command]
async fn system_profile_status()-> SystemProfileStatus {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(engine) = Engine::open_default() else {
        return SystemProfileStatus {
            learned: false,
            age_days: None,
            preview: String::new(),
        };
    };
    let prof = engine.system_profile();
    SystemProfileStatus {
        learned: prof.is_some(),
        age_days: engine.system_profile_age_days(),
        preview: prof
            .map(|p| p.chars().take(4000).collect())
            .unwrap_or_default(),
    }
})
    .await
        .unwrap()
}

/// Summary of this workspace's graphify code graph, for the Brain view.
#[tauri::command]
async fn code_graph(path: String)-> lectern_engine::codegraph::CodeGraphSummary {
    tauri::async_runtime::spawn_blocking(move || {
    let Ok(engine) = Engine::open_default() else {
        return Default::default();
    };
    let Ok(ws) = engine.open_workspace(Path::new(&path)) else {
        return Default::default();
    };
    engine.code_graph_summary(&ws)
})
    .await
        .unwrap()
}

/// Build/refresh this workspace's graphify code graph (`graphify update`, no LLM). Runs the
/// pipx CLI with the AppImage's Python env scrubbed so it uses the real interpreter.
#[tauri::command]
async fn build_code_graph(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bin = resolve_graphify()
            .ok_or_else(|| "graphify CLI not found — install it with: pipx install graphifyy".to_string())?;
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("update").arg(&path).current_dir(&path);
        lectern_engine::backend::scrub_appimage_env(&mut cmd);
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            Ok(s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Code graph built.").trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Locate the `graphify` CLI (PATH, then ~/.local/bin — where pipx installs it).
fn resolve_graphify() -> Option<String> {
    if let Ok(p) = std::env::var("PATH") {
        for d in p.split(':').filter(|d| !d.is_empty()) {
            let c = Path::new(d).join("graphify");
            if c.exists() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
    }
    let home = lectern_engine::home_dir();
    let c = format!("{home}/.local/bin/graphify");
    Path::new(&c).exists().then_some(c)
}

/// Whether the routing classifier is enabled (for the Settings toggle).
#[tauri::command]
async fn routing_classifier()-> bool {
    tauri::async_runtime::spawn_blocking(move || {
    lectern_engine::route::classifier_enabled()
})
    .await
        .unwrap()
}

/// Turn the routing classifier on/off (persisted to ~/.lectern/routing.json, live).
#[tauri::command]
async fn set_routing_classifier(on: bool)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    lectern_engine::route::set_classifier(on).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Read a text file for the built-in editor. Rejects oversized/binary files.
#[tauri::command]
async fn read_text_file(path: String)-> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > 2_000_000 {
        return Err("File is too large to open in the editor (>2 MB).".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.contains(&0) {
        return Err("This looks like a binary file.".into());
    }
    String::from_utf8(bytes).map_err(|_| "File isn't valid UTF-8 text.".to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Save the built-in editor's contents back to disk.
#[tauri::command]
async fn write_text_file(path: String, content: String)-> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
    std::fs::write(&path, content).map_err(|e| e.to_string())
})
    .await
        .map_err(|e| e.to_string())?
}

/// Learn the user's machine (routed agent probes it) and save the always-on profile,
/// streaming progress. Autonomous (the agent runs probe commands).
#[tauri::command]
async fn learn_system_session(
    session_id: String,
    on_event: Channel<serde_json::Value>,
) -> Result<String, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    running()
        .lock()
        .unwrap()
        .insert(session_id.clone(), cancel.clone());
    let sid = session_id.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let cancel2 = cancel.clone();
        let make = move |b: &str, m: Option<String>| build_backend(b, true, m, cancel2.clone());
        let mut sink = |ev: lectern_engine::event::AgentEvent| {
            if let Ok(v) = serde_json::to_value(&ev) {
                let _ = on_event.send(v);
            }
        };
        engine
            .learn_system(&make, &mut sink)
            .map_err(|e| e.to_string())
    })
    .await;
    running().lock().unwrap().remove(&sid);
    match out {
        Ok(inner) => inner,
        Err(e) => Err(e.to_string()),
    }
}

/// Deterministically replay a recorded GUI skill (no agent) — runs immediately, streaming
/// each action. Used for record/replay skills so a thinking model doesn't re-reason them.
#[tauri::command]
async fn replay_skill_session(
    name: String,
    path: String,
    on_event: Channel<serde_json::Value>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = Engine::open_default().map_err(|e| e.to_string())?;
        let ws = engine
            .open_workspace(Path::new(&path))
            .map_err(|e| e.to_string())?;
        let mut sink = |ev: lectern_engine::event::AgentEvent| {
            if let Ok(v) = serde_json::to_value(&ev) {
                let _ = on_event.send(v);
            }
        };
        engine.replay_skill(&ws, &name, &mut sink).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop an in-flight session — kills the supervised agent process (Stop / Esc).
#[tauri::command]
fn cancel_session(session_id: String) {
    if let Some(flag) = running().lock().unwrap().get(&session_id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod term_tests {
    use super::*;

    #[test]
    fn engine_spec_parses_and_defaults() {
        assert_eq!(parse_term_engine("local"), TermEngine::Local);
        assert_eq!(parse_term_engine("docker:web"), TermEngine::Docker("web".into()));
        assert_eq!(parse_term_engine("ssh:prod-box"), TermEngine::Ssh("prod-box".into()));
        assert_eq!(parse_term_engine("docker:"), TermEngine::Local);
        assert_eq!(parse_term_engine("junk"), TermEngine::Local);
    }

    #[test]
    fn ssh_hosts_skip_wildcards_and_dupes() {
        let cfg = "Host prod-box\n  User zeke\nHost *.internal !bad staging prod-box\nHost dev\n";
        assert_eq!(ssh_config_hosts(cfg), vec!["prod-box".to_string(), "staging".into(), "dev".into()]);
    }
}

fn main() {
    // Headless MCP-server mode: `lectern-desktop mcp serve [path]` exposes the brain
    // over stdio (no GUI). Lets the installed app double as the MCP server so the
    // one-click "Connect Lectern's brain" needs no separate CLI binary.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "mcp") && args.iter().any(|a| a == "serve") {
        let path = args
            .last()
            .filter(|a| !a.starts_with('-') && *a != "serve" && *a != "mcp")
            .map(String::as_str)
            .unwrap_or(".");
        let code = (|| -> Result<(), String> {
            let engine = Engine::open_default().map_err(|e| e.to_string())?;
            let ws = engine
                .open_workspace(Path::new(path))
                .map_err(|e| e.to_string())?;
            engine.mcp_serve(&ws).map_err(|e| e.to_string())
        })();
        if let Err(e) = code {
            eprintln!("lectern mcp: {e}");
            std::process::exit(1);
        }
        return;
    }
    tauri::Builder::default()
        // Ports P3a/P3b: LECTERN_SMOKE=1 → prove the window+webview actually
        // constructs on this OS (the risky part of a port), print the marker,
        // exit clean. CI runs this on Windows + macOS runners.
        .setup(|app| {
            if std::env::var("LECTERN_SMOKE").is_ok() {
                use tauri::Manager;
                let ok = app.get_webview_window("main").is_some();
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    // stderr + flush: tauri's exit can eat buffered stdout.
                    use std::io::Write;
                    let _ = writeln!(
                        std::io::stderr(),
                        "{}",
                        if ok { "LECTERN_SMOKE_OK" } else { "LECTERN_SMOKE_NO_WINDOW" }
                    );
                    let _ = std::io::stderr().flush();
                    handle.exit(if ok { 0 } else { 1 });
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_backends,
            claude_models,
            opencode_models,
            reset_skill_stats,
            get_user_profile,
            routing_summary,
            channels_status,
            save_chat_export,
            list_themes,
            usage_stats,
            store_sessions,
            store_set_session_meta,
            store_rename_session,
            store_pin_session,
            store_session_events,
            audit_skill,
            term_open,
            term_engines,
            term_write,
            term_resize,
            term_kill,
            read_theme,
            save_theme_file,
            open_config_file,
            set_user_profile,
            doctor,
            list_dir,
            skills,
            sync_skills,
            record_skill,
            delete_skill,
            create_skill,
            export_skill,
            save_skill_file,
            import_skill_file,
            browse_registry,
            fetch_registry_skill,
            install_registry_skill,
            registry_installed,
            publish_skill,
            registry_repo_url,
            agent_skills,
            list_mcp,
            add_mcp,
            remove_mcp,
            connect_brain,
            connect_graphify,
            replay_skill_session,
            models,
            read_image_b64,
            run_conductor_session,
            system_profile_status,
            learn_system_session,
            pick_files,
            is_image,
            start_dictation,
            stop_dictation,
            dictation_available,
            start_recording,
            stop_recording,
            recording_active,
            save_recorded_skill,
            account,
            pick_folder,
            get_prefs,
            set_prefs,
            get_sessions,
            save_sessions,
            save_pasted_image,
            read_clipboard_image,
            list_schedules,
            list_all_schedules,
            clear_finished_schedules,
            app_meta,
            open_url,
            schedule_add,
            cancel_schedule,
            brain_graph,
            code_graph,
            build_code_graph,
            routing_classifier,
            set_routing_classifier,
            read_text_file,
            write_text_file,
            run_session,
            cancel_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lectern");
}
