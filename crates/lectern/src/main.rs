//! `lectern` — the CLI client for the Lectern engine.
//! Open a repo, run an agent session, review the staged turn, apply changes.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lectern_engine::backend::Backend;
use lectern_engine::event::{AgentEvent, DiffKind};
use lectern_engine::{
    cloud, AntigravityBackend, ClaudeCodeBackend, Engine, LimitBackend, MockBackend,
    OpenCodeBackend, RunOptions,
};
use std::path::{Path, PathBuf};
use std::time::Instant;

// ── tiny ANSI helpers (no deps) ──────────────────────────────────────────────
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
fn dim(s: &str) -> String {
    format!("{DIM}{s}{RESET}")
}
fn bold(s: &str) -> String {
    format!("{BOLD}{s}{RESET}")
}

// ── benchmark instrumentation ────────────────────────────────────────────────
// A machine-readable report of one run, written when `--metrics-out` is set. It
// turns the event stream into verifiable numbers — token cost, tool-call count,
// and (for the Conductor) the actual per-step model routing and whether a
// cross-model review fired — so orchestration claims can be measured, not asserted.
#[derive(serde::Serialize)]
struct RouteRec {
    model: String,
    reason: String,
}

#[derive(serde::Serialize, Default)]
struct RunMetrics {
    mode: String,    // "run" | "conductor"
    backend: String, // requested backend
    success: bool,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    tool_calls: u32, // Terminal events (commands the agent ran)
    file_edits: u32, // FileEdit events
    plan_steps: u32, // steps in the Conductor's plan
    recalls: u32,    // brain signals: memory recalls + applied skills
    review_steps: u32,
    routes: Vec<RouteRec>, // per-step model routing decisions
    distinct_models: u32,  // how many different models actually ran
    changes: u32,
    limit_hit: bool,
    wall_ms: u128,
    error: Option<String>,
}

impl RunMetrics {
    fn new(mode: &str, backend: &str) -> Self {
        RunMetrics {
            mode: mode.into(),
            backend: backend.into(),
            ..Default::default()
        }
    }
    // Fold one event into the running tallies (called before the event is rendered).
    fn observe(&mut self, ev: &AgentEvent) {
        match ev {
            AgentEvent::Terminal { .. } => self.tool_calls += 1,
            AgentEvent::FileEdit { .. } => self.file_edits += 1,
            AgentEvent::Plan { steps } => self.plan_steps = steps.len() as u32,
            AgentEvent::ModelRouted { model, reason } => {
                if reason.to_lowercase().contains("review") {
                    self.review_steps += 1;
                }
                self.routes.push(RouteRec {
                    model: model.clone(),
                    reason: reason.clone(),
                });
            }
            AgentEvent::Thought { recalls, .. } => self.recalls += recalls.len() as u32,
            AgentEvent::SkillApplied { .. } => self.recalls += 1,
            // Fallback token totals if the run errors before returning a RunResult.
            AgentEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                self.input_tokens = *input_tokens;
                self.output_tokens = *output_tokens;
            }
            _ => {}
        }
    }
    // Fill in the authoritative totals from the final result and write the JSON.
    fn finalize(&mut self, elapsed: Instant) {
        self.wall_ms = elapsed.elapsed().as_millis();
        self.total_tokens = self.input_tokens + self.output_tokens;
        let mut models: Vec<&str> = self.routes.iter().map(|r| r.model.as_str()).collect();
        models.sort_unstable();
        models.dedup();
        self.distinct_models = models.len() as u32;
    }
    fn write(&self, path: &Path) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!(
                        "{}",
                        dim(&format!("metrics: could not write {path:?}: {e}"))
                    );
                }
            }
            Err(e) => eprintln!("{}", dim(&format!("metrics: serialize failed: {e}"))),
        }
    }
}

#[derive(Parser)]
#[command(
    name = "lectern",
    version,
    about = "An engine for your AI — local-first agent orchestration."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Launch the terminal UI (finds lectern-tui on PATH, next to this
    /// binary, or falls back to `bun run` in a dev checkout).
    Tui {
        /// Extra args passed through (e.g. --path, --backend).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Open (index) a repository as a workspace.
    Open {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Run an agent session against a workspace.
    Run {
        /// The task to perform.
        prompt: Vec<String>,
        /// Backend: auto (Claude Code if present, else mock), claude-code, or mock.
        #[arg(short, long, default_value = "auto")]
        backend: String,
        /// Workspace path.
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Apply proposed changes to disk (Claude Code edits in place; otherwise plan only).
        #[arg(long)]
        apply: bool,
        /// Run in an isolated git worktree/branch (safe parallel sessions).
        #[arg(long)]
        worktree: bool,
        /// Don't animate the mock backend.
        #[arg(long)]
        fast: bool,
        /// Model for Claude Code (an alias like "sonnet"/"opus" or a full model id).
        #[arg(long)]
        model: Option<String>,
        /// Fully autonomous Claude Code — skip permission prompts (also runs commands).
        #[arg(long)]
        yolo: bool,
        /// Claude Code fallback model when the primary is limited/unavailable.
        #[arg(long)]
        fallback_model: Option<String>,
        /// On a usage limit, auto-schedule a retry this many seconds later.
        #[arg(long, default_value = "3600")]
        retry_after: i64,
        /// Write a machine-readable run report (tokens, tool calls, routing) to this JSON file.
        #[arg(long)]
        metrics_out: Option<PathBuf>,
    },
    /// Show the adaptive context Lectern would send for a prompt (the "why" inspector).
    Context {
        prompt: Vec<String>,
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "8000")]
        budget: u64,
    },
    /// List recent sessions for a workspace.
    Sessions {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Export a session as an encrypted bundle (G3 — move sessions between machines).
    SessionExport {
        /// Session id (see `lectern sessions`).
        session_id: String,
        /// Output file (e.g. mysession.lectern-enc).
        out: PathBuf,
        /// Passphrase (min 8 chars); falls back to $LECTERN_PASSPHRASE.
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Import an encrypted session bundle into a workspace as a new session.
    SessionImport {
        /// Bundle file produced by session-export.
        file: PathBuf,
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Record, list, and manage learned skills.
    Skills {
        #[command(subcommand)]
        cmd: SkillsCmd,
    },
    /// List backends and their availability.
    Backends,
    /// Check your setup: engine, Claude Code, Antigravity, and cloud login.
    Doctor,
    /// Sign in to Lectern cloud (device authorization grant).
    Login {
        /// Override the cloud base URL (or set LECTERN_API_URL).
        #[arg(long)]
        url: Option<String>,
    },
    /// Sign out (clears the stored cloud token).
    Logout,
    /// Show the signed-in account, plan, and limits.
    Account,
    /// Sync learned skills/memory with the cloud (E2E-encrypted).
    Sync {
        #[command(subcommand)]
        cmd: SyncCmd,
    },
    /// Schedule a task for later, or run due tasks now.
    Schedule {
        #[command(subcommand)]
        cmd: ScheduleCmd,
    },
    /// Daemon controls.
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
    /// Model Context Protocol: expose Lectern's brain to MCP clients.
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
    /// Learn this machine and save the always-on system profile (~/.lectern/system.md).
    LearnSystem,
    /// Conductor: plan a task, then hand each sub-task to the model that excels at it.
    Conduct {
        /// The task to orchestrate.
        prompt: Vec<String>,
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Apply the steps' edits to disk (otherwise plan-only).
        #[arg(long)]
        apply: bool,
        /// Fully autonomous (skip permission prompts; lets steps run commands).
        #[arg(long)]
        yolo: bool,
        /// Backend for steps when routing is unavailable (default: auto).
        #[arg(short, long, default_value = "auto")]
        backend: String,
        /// Write a machine-readable run report (tokens, per-step routing, review) to this JSON file.
        #[arg(long)]
        metrics_out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum McpCmd {
    /// Run an MCP server over stdio that exposes the shared brain (recall_memory,
    /// list_skills, get_skill). Add to Claude Code with:
    /// `claude mcp add lectern-brain -- lectern mcp serve`.
    Serve {
        /// Workspace whose memory to serve (defaults to the current directory).
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ScheduleCmd {
    /// Schedule a one-shot task (--at +30m | +2h | +1d | now | <unix-ts>).
    Add {
        prompt: Vec<String>,
        #[arg(long, default_value = "+1h")]
        at: String,
        #[arg(short, long, default_value = "mock")]
        backend: String,
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// List schedules for a workspace.
    List {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Cancel a schedule by id.
    Cancel { id: String },
    /// Run all due schedules now (what the daemon does on a loop).
    RunDue {
        #[arg(long, default_value = "3600")]
        retry_after: i64,
    },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Push this workspace's skills to the cloud.
    Push {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Pull and import synced skills for this workspace.
    Pull {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Show sync status for this workspace.
    Status {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Show daemon status.
    Status,
}

#[derive(Subcommand)]
enum SkillsCmd {
    /// List learned skills for a workspace.
    List {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Record a skill by distilling a session (the in-app /record, headless).
    Record {
        /// Skill name (defaults to one derived from the session).
        #[arg(short, long)]
        name: Option<String>,
        /// Session id to record from (defaults to the most recent).
        #[arg(short, long)]
        session: Option<String>,
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{RED}error:{RESET} {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Open { path } => cmd_open(&path),
        Cmd::Run {
            prompt,
            backend,
            path,
            apply,
            worktree,
            fast,
            model,
            yolo,
            fallback_model,
            retry_after,
            metrics_out,
        } => cmd_run(
            prompt.join(" "),
            &backend,
            &path,
            apply,
            worktree,
            RunFlags {
                fast,
                model,
                yolo,
                fallback_model,
            },
            retry_after,
            metrics_out,
        ),
        Cmd::Context {
            prompt,
            path,
            budget,
        } => cmd_context(prompt.join(" "), &path, budget),
        Cmd::Sessions { path } => cmd_sessions(&path),
        Cmd::SessionExport {
            session_id,
            out,
            passphrase,
        } => {
            let pass = passphrase
                .or_else(|| std::env::var("LECTERN_PASSPHRASE").ok())
                .context("pass --passphrase or set LECTERN_PASSPHRASE")?;
            let engine = lectern_engine::Engine::open_default()?;
            let bundle = engine.export_session_encrypted(&session_id, &pass)?;
            std::fs::write(&out, bundle)?;
            println!("✓ encrypted bundle → {}", out.display());
            Ok(())
        }
        Cmd::SessionImport {
            file,
            path,
            passphrase,
        } => {
            let pass = passphrase
                .or_else(|| std::env::var("LECTERN_PASSPHRASE").ok())
                .context("pass --passphrase or set LECTERN_PASSPHRASE")?;
            let engine = lectern_engine::Engine::open_default()?;
            let ws = engine.open_workspace(&path)?;
            let text = std::fs::read_to_string(&file)?;
            let sid = engine.import_session_encrypted(&ws, &text, &pass)?;
            println!("✓ imported as session {sid}");
            Ok(())
        }
        Cmd::Backends => cmd_backends(),
        Cmd::Tui { args } => cmd_tui(&args),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Skills { cmd } => match cmd {
            SkillsCmd::List { path } => cmd_skills_list(&path),
            SkillsCmd::Record {
                name,
                session,
                path,
            } => cmd_skills_record(name, session, &path),
        },
        Cmd::Login { url } => cmd_login(url),
        Cmd::Logout => cmd_logout(),
        Cmd::Account => cmd_account(),
        Cmd::Sync { cmd } => match cmd {
            SyncCmd::Push { path } => cmd_sync(&path, "push"),
            SyncCmd::Pull { path } => cmd_sync(&path, "pull"),
            SyncCmd::Status { path } => cmd_sync(&path, "status"),
        },
        Cmd::Schedule { cmd } => match cmd {
            ScheduleCmd::Add {
                prompt,
                at,
                backend,
                path,
                apply,
            } => cmd_schedule_add(prompt.join(" "), &at, &backend, &path, apply),
            ScheduleCmd::List { path } => cmd_schedule_list(&path),
            ScheduleCmd::Cancel { id } => cmd_schedule_cancel(&id),
            ScheduleCmd::RunDue { retry_after } => cmd_schedule_run_due(retry_after),
        },
        Cmd::Daemon { cmd } => match cmd {
            DaemonCmd::Status => cmd_daemon_status(),
        },
        Cmd::Mcp { cmd } => match cmd {
            McpCmd::Serve { path } => cmd_mcp_serve(&path),
        },
        Cmd::Conduct {
            prompt,
            path,
            apply,
            yolo,
            backend,
            metrics_out,
        } => cmd_conduct(prompt.join(" "), &path, apply, yolo, &backend, metrics_out),
        Cmd::LearnSystem => cmd_learn_system(),
    }
}

/// Learn the machine + save the always-on system profile.
fn cmd_learn_system() -> Result<()> {
    let engine = Engine::open_default()?;
    println!("{}", dim("Learning your system (the agent will probe it)…"));
    println!();
    let make = |b: &str, m: Option<String>| -> Box<dyn Backend> {
        let flags = RunFlags {
            fast: true,
            model: m,
            yolo: true,
            fallback_model: None,
        };
        pick_backend(b, &flags).unwrap_or_else(|_| Box::new(MockBackend { fast: true }))
    };
    let profile = engine.learn_system(&make, &mut render_event)?;
    println!();
    println!(
        "{}",
        dim(&format!(
            "saved {} chars → ~/.lectern/system.md",
            profile.len()
        ))
    );
    Ok(())
}

/// Serve the shared brain to MCP clients over stdio (blocks until the client disconnects).
fn cmd_mcp_serve(path: &std::path::Path) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    engine.mcp_serve(&ws)
}

/// Conductor: plan the task, then hand each sub-task to its routed model.
fn cmd_conduct(
    prompt: String,
    path: &std::path::Path,
    apply: bool,
    yolo: bool,
    backend: &str,
    metrics_out: Option<PathBuf>,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("provide a task, e.g. lectern conduct \"add a config file and a loader\"");
    }
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    println!(
        "{}  {}",
        bold(&ws.name),
        dim(&format!("· conductor · {}", truncate(&prompt, 60)))
    );
    println!();
    // Backend factory: "auto" honors the Conductor's per-step routing (harness+model);
    // any other --backend pins every step to that backend (forces mock/antigravity/claude).
    let make = move |b: &str, m: Option<String>| -> Box<dyn Backend> {
        let routed = backend == "auto";
        let use_backend = if routed { b } else { backend };
        let flags = RunFlags {
            fast: true,
            model: if routed { m } else { None },
            yolo,
            fallback_model: None,
        };
        pick_backend(use_backend, &flags).unwrap_or_else(|_| Box::new(MockBackend { fast: true }))
    };
    let mut metrics = RunMetrics::new("conductor", backend);
    let started = Instant::now();
    let outcome = {
        let m = &mut metrics;
        let mut sink = |ev: AgentEvent| {
            m.observe(&ev);
            render_event(ev);
        };
        engine.run_conductor(&ws, &prompt, &make, apply, &mut sink)
    };
    if let Ok(r) = &outcome {
        metrics.success = true;
        metrics.input_tokens = r.usage.input_tokens;
        metrics.output_tokens = r.usage.output_tokens;
        metrics.changes = r.changes.len() as u32;
        metrics.limit_hit = r.limit_hit;
    } else if let Err(e) = &outcome {
        metrics.error = Some(e.to_string());
    }
    metrics.finalize(started);
    if let Some(p) = &metrics_out {
        metrics.write(p);
    }
    let result = outcome?;
    println!();
    println!(
        "{}",
        dim(&format!(
            "conductor done · {} change(s) · {} in / {} out tokens",
            result.changes.len(),
            result.usage.input_tokens,
            result.usage.output_tokens
        ))
    );
    Ok(())
}

fn cloud_base(url: Option<String>) -> String {
    url.or_else(|| std::env::var("LECTERN_API_URL").ok())
        .unwrap_or_else(|| cloud::DEFAULT_BASE_URL.to_string())
}

fn cmd_login(url: Option<String>) -> Result<()> {
    let base = cloud_base(url);
    let dc = cloud::request_device_code(&base)?;
    println!("{}", bold("Sign in to Lectern"));
    println!("  1. open  {CYAN}{}{RESET}", dc.verification_uri);
    println!("  2. enter code  {}", bold(&dc.user_code));
    println!("{}", dim("waiting for approval…"));
    let token = cloud::poll_for_token(&base, &dc.device_code, dc.interval, dc.expires_in)?;
    cloud::save_auth(&cloud::Auth {
        base_url: base,
        token,
    })?;
    println!("{GREEN}✓ logged in{RESET}");
    if let Some(auth) = cloud::load_auth() {
        if let Ok(ent) = cloud::get_entitlements(&auth) {
            if let Some(plan) = ent.pointer("/token/plan").and_then(|p| p.as_str()) {
                println!("  {}", dim(&format!("plan: {plan}")));
            }
        }
    }
    Ok(())
}

fn cmd_logout() -> Result<()> {
    cloud::clear_auth()?;
    println!("{}", dim("signed out."));
    Ok(())
}

fn cmd_account() -> Result<()> {
    let auth =
        cloud::load_auth().ok_or_else(|| anyhow::anyhow!("not logged in — run `lectern login`"))?;
    println!("{}  {}", bold("Account"), dim(&auth.base_url));
    let ent = cloud::get_entitlements(&auth)?;
    let plan = ent
        .pointer("/token/plan")
        .and_then(|p| p.as_str())
        .unwrap_or("?");
    println!("  plan: {}", bold(plan));
    if let Some(limits) = ent.pointer("/token/limits").and_then(|l| l.as_object()) {
        for (k, v) in limits {
            println!("    {}", dim(&format!("{k}: {v}")));
        }
    }
    Ok(())
}

fn cmd_sync(path: &std::path::Path, action: &str) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    match action {
        "push" => {
            let n = engine.sync_push(&ws)?;
            println!(
                "{GREEN}✓{RESET} pushed {} skill(s) for {}",
                n,
                bold(&ws.name)
            );
        }
        "pull" => {
            let n = engine.sync_pull(&ws)?;
            println!(
                "{GREEN}✓{RESET} imported {} new skill(s) for {}",
                n,
                bold(&ws.name)
            );
        }
        _ => {
            let logged_in = cloud::load_auth().is_some();
            let local = engine.list_skills(&ws)?.len();
            println!("{}  {}", bold("Sync"), dim(&ws.name));
            println!(
                "  signed in: {}",
                if logged_in {
                    "yes"
                } else {
                    "no — run `lectern login`"
                }
            );
            println!("  local skills: {local}");
        }
    }
    Ok(())
}

fn cmd_open(path: &std::path::Path) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path).context("opening workspace")?;
    let idx = engine.index_workspace(&ws).context("indexing workspace")?;
    println!("{} {}", bold("opened"), ws.name);
    println!("  {}", dim(&ws.root.to_string_lossy()));
    println!(
        "  {}",
        dim(&format!(
            "indexed {} files · {} KB into memory",
            idx.files,
            idx.bytes / 1024
        ))
    );
    Ok(())
}

/// Backend-construction flags from `lectern run`.
#[derive(Clone)]
struct RunFlags {
    fast: bool,
    model: Option<String>,
    yolo: bool,
    fallback_model: Option<String>,
}

fn make_claude(flags: &RunFlags) -> Result<Box<dyn Backend>> {
    if !ClaudeCodeBackend::new().available() {
        anyhow::bail!(
            "claude-code backend not available — the `claude` CLI wasn't found.\n  install:  npm i -g @anthropic-ai/claude-code\n  then run `claude` once to log in, and `lectern doctor` to verify."
        );
    }
    Ok(Box::new(ClaudeCodeBackend {
        model: flags.model.clone(),
        fallback_model: flags.fallback_model.clone(),
        skip_permissions: flags.yolo,
        ..ClaudeCodeBackend::new()
    }))
}

fn pick_backend(name: &str, flags: &RunFlags) -> Result<Box<dyn Backend>> {
    match name {
        "auto" => {
            if ClaudeCodeBackend::new().available() {
                make_claude(flags)
            } else {
                Ok(Box::new(MockBackend { fast: flags.fast }))
            }
        }
        "mock" => Ok(Box::new(MockBackend { fast: flags.fast })),
        "mock-limit" => Ok(Box::new(LimitBackend)),
        "claude-code" | "claude" => make_claude(flags),
        "antigravity" | "gemini" => {
            if !AntigravityBackend::new().available() {
                anyhow::bail!("antigravity backend not available — the `agy` CLI wasn't found. Install Antigravity and run `agy` once to log in.");
            }
            Ok(Box::new(AntigravityBackend {
                model: flags.model.clone(),
                skip_permissions: flags.yolo,
                ..AntigravityBackend::new()
            }))
        }
        "opencode" => {
            if !OpenCodeBackend::new().available() {
                anyhow::bail!("opencode backend not available — install from opencode.ai, then `opencode auth login` (its free models need no key).");
            }
            Ok(Box::new(OpenCodeBackend {
                model: flags.model.clone(),
                ..OpenCodeBackend::new()
            }))
        }
        "openrouter" => {
            if !OpenCodeBackend::new().available() {
                anyhow::bail!("OpenRouter runs through opencode — install from opencode.ai, then `opencode auth login` and connect OpenRouter.");
            }
            let model = flags.model.clone().or_else(|| {
                lectern_engine::backend::discover_openrouter_models()
                    .first()
                    .map(|(id, _)| id.clone())
            });
            if model.is_none() {
                anyhow::bail!("OpenRouter isn't connected in opencode yet — run `opencode auth login`, pick OpenRouter (free models available), then retry.");
            }
            Ok(Box::new(OpenCodeBackend {
                model,
                ..OpenCodeBackend::new()
            }))
        }
        "ollama" => {
            let models = lectern_engine::backend::discover_ollama_models();
            if models.is_empty() {
                anyhow::bail!("Ollama isn't running — install from ollama.com, start it, then `ollama pull llama3`.");
            }
            if !OpenCodeBackend::new().available() {
                anyhow::bail!(
                    "Ollama models run through opencode — install from opencode.ai, then retry."
                );
            }
            let model = flags
                .model
                .clone()
                .or_else(|| models.first().map(|(id, _)| id.clone()));
            Ok(Box::new(OpenCodeBackend {
                model,
                ..OpenCodeBackend::new()
            }))
        }
        other => {
            anyhow::bail!(
                "unknown backend: {other} (try: auto, claude-code, antigravity, opencode, openrouter, ollama, mock)"
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    prompt: String,
    backend_name: &str,
    path: &std::path::Path,
    apply: bool,
    worktree: bool,
    flags: RunFlags,
    retry_after: i64,
    metrics_out: Option<PathBuf>,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("provide a prompt, e.g. lectern run \"add a settings page\"");
    }
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let yolo = flags.yolo;
    // Smart routing: --model auto picks the harness + model that excel at this task.
    let mut flags = flags;
    let mut chosen_backend = backend_name.to_string();
    if flags.model.as_deref() == Some("auto") {
        let mut r = lectern_engine::route::route_model(&prompt);
        if r.backend == "antigravity" && !AntigravityBackend::new().available() {
            r.reason = format!("{} (Gemini unavailable → Sonnet)", r.reason);
            r.backend = "claude-code".into();
            r.model = "sonnet".into();
            r.label = "Sonnet 4.6".into();
        }
        println!(
            "{}",
            dim(&format!("  routed to {} — {}", r.label, r.reason))
        );
        flags.model = Some(r.model);
        chosen_backend = r.backend;
    }
    let backend = pick_backend(&chosen_backend, &flags)?;

    println!(
        "{}  {}",
        bold(&ws.name),
        dim(&format!("· {} · {}", backend.id(), truncate(&prompt, 60)))
    );
    if backend.id() == "claude-code" {
        let mode = if yolo {
            "autonomous · skips permission prompts, runs commands"
        } else if apply {
            "apply · edits land in your workspace"
        } else {
            "plan · proposes changes, edits nothing (add --apply to write)"
        };
        println!("{}", dim(&format!("  Claude Code mode: {mode}")));
    }
    println!();

    let mut metrics = RunMetrics::new("run", backend_name);
    let started = Instant::now();
    let outcome = {
        let m = &mut metrics;
        let sink = |ev: AgentEvent| {
            m.observe(&ev);
            render_event(ev);
        };
        engine.run(
            &ws,
            &prompt,
            backend.as_ref(),
            RunOptions { apply, worktree },
            sink,
        )
    };
    if let Ok(r) = &outcome {
        metrics.success = true;
        metrics.input_tokens = r.usage.input_tokens;
        metrics.output_tokens = r.usage.output_tokens;
        metrics.changes = r.changes.len() as u32;
        metrics.limit_hit = r.limit_hit;
    } else if let Err(e) = &outcome {
        metrics.error = Some(e.to_string());
    }
    metrics.finalize(started);
    if let Some(p) = &metrics_out {
        metrics.write(p);
    }
    let result = outcome?;

    println!();
    if result.changes.is_empty() {
        println!("{}", dim("no file changes proposed."));
    } else {
        let total = result.changes.len();
        println!("{} {} file(s):", bold("Changes"), total);
        for c in &result.changes {
            println!(
                "  {}  {GREEN}+{}{RESET} {RED}-{}{RESET}",
                c.path, c.added, c.removed
            );
        }
        if result.applied {
            println!("{GREEN}✓ applied to disk{RESET}");
        } else {
            println!(
                "{}",
                dim("review then re-run with --apply to write these changes.")
            );
        }
    }
    if let Some(wt) = &result.worktree {
        println!(
            "{}",
            dim(&format!(
                "↳ isolated in worktree {} (branch {})",
                wt.path.display(),
                bold(&wt.branch)
            ))
        );
        println!(
            "{}",
            dim(&format!(
                "  merge with: git -C {} merge {}",
                ws.root.display(),
                wt.branch
            ))
        );
    }
    println!(
        "{}",
        dim(&format!(
            "session {} · {} in / {} out tokens",
            &result.session_id[..8],
            result.usage.input_tokens,
            result.usage.output_tokens
        ))
    );
    // Best-effort content-free usage telemetry (only if signed in; counts only).
    engine.report_usage(
        backend.id(),
        result.usage.input_tokens,
        result.usage.output_tokens,
    );
    // Auto-continue: if the backend hit a usage limit, schedule a retry for later.
    if result.limit_hit {
        let id = engine.schedule_retry(
            &ws.id,
            &prompt,
            backend_name,
            apply,
            retry_after,
            "auto-continue after limit",
        )?;
        println!(
            "{}",
            dim(&format!(
                "⏳ usage limit hit — scheduled auto-continue in {retry_after}s (schedule {})",
                &id[..8]
            ))
        );
        println!(
            "{}",
            dim("   run it when due with: lectern schedule run-due (or leave lecternd running)")
        );
    }
    Ok(())
}

fn parse_at(s: &str) -> Result<i64> {
    let now = lectern_engine::now_ts();
    let s = s.trim();
    if s == "now" {
        return Ok(now);
    }
    if let Some(rest) = s.strip_prefix('+') {
        let last = rest.chars().last().unwrap_or(' ');
        let (num, mult) = match last {
            's' => (&rest[..rest.len() - 1], 1),
            'm' => (&rest[..rest.len() - 1], 60),
            'h' => (&rest[..rest.len() - 1], 3600),
            'd' => (&rest[..rest.len() - 1], 86_400),
            _ => (rest, 1), // bare number = seconds
        };
        let n: i64 = num
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("bad --at value: {s} (try +30m, +2h, now)"))?;
        return Ok(now + n * mult);
    }
    s.parse::<i64>()
        .map_err(|_| anyhow::anyhow!("bad --at value: {s} (try +30m, +2h, now, or a unix ts)"))
}

fn cmd_schedule_add(
    prompt: String,
    at: &str,
    backend: &str,
    path: &std::path::Path,
    apply: bool,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("provide a prompt to schedule");
    }
    let run_at = parse_at(at)?;
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let id = engine.schedule_add(&ws, &prompt, backend, apply, run_at, "scheduled")?;
    let delta = (run_at - lectern_engine::now_ts()).max(0);
    println!(
        "{GREEN}✓{RESET} scheduled {} {}",
        bold(&truncate(&prompt, 40)),
        dim(&format!("· runs in ~{delta}s · id {}", &id[..8]))
    );
    Ok(())
}

fn cmd_schedule_list(path: &std::path::Path) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let rows = engine.list_schedules(&ws)?;
    if rows.is_empty() {
        println!(
            "{}",
            dim("no schedules. add one with `lectern schedule add`.")
        );
        return Ok(());
    }
    let now = lectern_engine::now_ts();
    println!("{} {}", bold("Schedules"), dim(&format!("· {}", ws.name)));
    for (id, prompt, backend, _apply, run_at, reason, status) in rows {
        let when = run_at - now;
        let when = if when > 0 {
            format!("in {when}s")
        } else {
            format!("{}s ago", -when)
        };
        println!(
            "  {} {}  {}",
            dim(&id[..8]),
            bold(&truncate(&prompt, 40)),
            dim(&format!("[{status}] {when} · {backend} · {reason}"))
        );
    }
    Ok(())
}

fn cmd_schedule_cancel(id: &str) -> Result<()> {
    Engine::open_default()?.cancel_schedule(id)?;
    println!("{}", dim("cancelled."));
    Ok(())
}

fn cmd_schedule_run_due(retry_after: i64) -> Result<()> {
    let engine = Engine::open_default()?;
    let ran = engine.run_due_schedules(retry_after, render_event)?;
    println!();
    println!("{GREEN}✓{RESET} ran {} due schedule(s)", ran.len());
    Ok(())
}

fn render_event(ev: AgentEvent) {
    match ev {
        AgentEvent::Thinking => println!("{}", dim("● thinking…")),
        AgentEvent::Thought { summary, recalls } => {
            println!("{GREEN}✓{RESET} {}", dim(&summary));
            for r in recalls {
                println!("    {}", dim(&format!("recall · {r}")));
            }
        }
        AgentEvent::SkillApplied { name, why } => {
            println!(
                "{GREEN}★{RESET} applied skill: {} {}",
                bold(&name),
                dim(&format!("· {why}"))
            );
        }
        AgentEvent::ModelRouted { model, reason } => {
            println!(
                "⇄ routed to {} {}",
                bold(&model),
                dim(&format!("· {reason}"))
            );
        }
        AgentEvent::Plan { steps } => {
            println!("{}", bold("Plan"));
            for s in steps {
                let mark = if s.done {
                    format!("{GREEN}✓{RESET}")
                } else {
                    dim("•").to_string()
                };
                println!("  {mark} {}", s.text);
            }
        }
        AgentEvent::FileEdit {
            path,
            added,
            removed,
            preview,
        } => {
            println!(
                "{} {}  {GREEN}+{}{RESET} {RED}-{}{RESET}",
                bold("Edit"),
                CYAN.to_string() + &path + RESET,
                added,
                removed
            );
            for line in preview {
                match line.kind {
                    DiffKind::Add => println!("  {GREEN}+ {}{RESET}", line.text),
                    DiffKind::Remove => println!("  {RED}- {}{RESET}", line.text),
                    DiffKind::Context => println!("    {}", dim(&line.text)),
                }
            }
        }
        AgentEvent::Terminal {
            command,
            output,
            exit_code,
        } => {
            println!("{} {}", dim("$"), command);
            let color = if exit_code == 0 { GREEN } else { RED };
            println!("  {color}{}{RESET}", output);
        }
        AgentEvent::Message { text } => println!("\n{}", text),
        AgentEvent::MessageDelta { text } => {
            // Stream assistant text inline as it arrives.
            use std::io::Write;
            print!("{}", text);
            let _ = std::io::stdout().flush();
        }
        AgentEvent::Usage { .. } => {}
        AgentEvent::LimitHit { reason } => println!(
            "{RED}! usage limit:{RESET} {reason} {}",
            dim("(would fall back to next backend)")
        ),
        AgentEvent::Error { message } => println!("{RED}error:{RESET} {message}"),
        AgentEvent::Done => {}
    }
}

fn cmd_context(prompt: String, path: &std::path::Path, budget: u64) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("provide a prompt, e.g. lectern context \"fix the login flow\"");
    }
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    engine.index_workspace(&ws)?;
    let m = engine.assemble_context(&ws, &prompt, budget);
    println!("{} {}", bold("Context for"), dim(&truncate(&prompt, 60)));
    println!();
    if m.recalls.is_empty() {
        println!("{}", dim("no relevant files recalled from memory."));
    } else {
        println!(
            "{} {}",
            bold("Recalled + included"),
            dim(&format!("({} match)", m.recalls.len()))
        );
        for it in &m.included {
            let toks = if it.tokens > 0 {
                format!("~{} tok", it.tokens)
            } else {
                "omitted".to_string()
            };
            println!(
                "  {CYAN}{}{RESET}  {}  {}",
                it.path,
                dim(&pad(&toks, 12)),
                dim(&it.reason)
            );
        }
    }
    if !m.skills_applied.is_empty() {
        println!("{}", bold("Skills applied"));
        for s in &m.skills_applied {
            println!("  {s}");
        }
    }
    println!();
    let pct = (m.token_estimate * 100)
        .checked_div(m.budget_tokens)
        .unwrap_or(0)
        .min(999);
    println!(
        "{}",
        dim(&format!(
            "≈ {} / {} tokens ({}% of budget){}",
            m.token_estimate,
            m.budget_tokens,
            pct,
            if m.truncated {
                " · trimmed to fit"
            } else {
                ""
            }
        ))
    );
    println!(
        "{}",
        dim("(local embeddings + vector recall layer on top of this next)")
    );
    Ok(())
}

fn cmd_sessions(path: &std::path::Path) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let rows = engine.recent_sessions(&ws, 20)?;
    if rows.is_empty() {
        println!("{}", dim("no sessions yet — run `lectern run \"…\"`."));
        return Ok(());
    }
    println!(
        "{} {}",
        bold("Recent sessions"),
        dim(&format!("· {}", ws.name))
    );
    for (id, title, backend, _ts, status) in rows {
        println!(
            "  {}  {}  {}  {}",
            &id[..8],
            pad(&status, 8),
            dim(&pad(&backend, 12)),
            title
        );
    }
    Ok(())
}

fn cmd_skills_list(path: &std::path::Path) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let skills = engine.list_skills(&ws)?;
    if skills.is_empty() {
        println!(
            "{}",
            dim("no skills yet — record one with `lectern skills record`.")
        );
        return Ok(());
    }
    println!("{} {}", bold("Skills"), dim(&format!("· {}", ws.name)));
    for s in skills {
        println!(
            "  {GREEN}★{RESET} {}  {}",
            bold(&s.name),
            dim(&format!(
                "[{}] · {} step(s) · {} uses",
                s.scope,
                s.body.steps.len(),
                s.uses
            ))
        );
        println!(
            "    {}",
            dim(&format!("triggers: {}", s.triggers.join(", ")))
        );
    }
    Ok(())
}

fn cmd_skills_record(
    name: Option<String>,
    session: Option<String>,
    path: &std::path::Path,
) -> Result<()> {
    let engine = Engine::open_default()?;
    let ws = engine.open_workspace(path)?;
    let skill = engine.record_skill(&ws, session.as_deref(), name.as_deref())?;
    println!("{} {}", bold("Recorded skill"), bold(&skill.name));
    println!("  {}", dim(&skill.description));
    println!(
        "  {}",
        dim(&format!("triggers: {}", skill.triggers.join(", ")))
    );
    if !skill.body.steps.is_empty() {
        println!("  {}", bold("steps:"));
        for s in &skill.body.steps {
            println!("    {} {}", dim("•"), s);
        }
    }
    println!(
        "{}",
        dim("auto-applies when a future task matches its triggers.")
    );
    Ok(())
}

fn cmd_backends() -> Result<()> {
    println!("{}", bold("Backends"));
    let mock = MockBackend::new();
    println!(
        "  {GREEN}●{RESET} {}  {}",
        pad(mock.id(), 14),
        dim("always available (demo pipeline)")
    );
    let cc = ClaudeCodeBackend::new();
    let (dot, note) = match cc.version() {
        Some(v) => (format!("{GREEN}●{RESET}"), format!("detected · {v}")),
        None => (
            format!("{DIM}○{RESET}"),
            "not found / not logged in".to_string(),
        ),
    };
    println!("  {dot} {}  {}", pad(cc.id(), 14), dim(&note));
    println!(
        "  {DIM}○{RESET} {}  {}",
        pad("api-key", 14),
        dim("planned (Anthropic/OpenAI/local)")
    );
    println!(
        "  {DIM}○{RESET} {}  {}",
        pad("antigravity", 14),
        dim("planned (workspace token)")
    );
    Ok(())
}

fn cmd_tui(args: &[String]) -> Result<()> {
    // 1) PATH, 2) sibling of this exe, 3) dev checkout via bun.
    let exe_name = if cfg!(windows) {
        "lectern-tui.exe"
    } else {
        "lectern-tui"
    };
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe_name)))
        .filter(|p| p.exists());
    let status = if which(exe_name) {
        std::process::Command::new(exe_name).args(args).status()
    } else if let Some(bin) = sibling {
        std::process::Command::new(bin).args(args).status()
    } else {
        // dev fallback: repo checkout relative to CWD
        let dev = std::path::Path::new("apps/tui/src/index.tsx");
        if !dev.exists() {
            anyhow::bail!(
                "lectern-tui not found — install it on PATH, place it next to `lectern`, or run from the repo root (bun fallback)"
            );
        }
        std::process::Command::new("bun")
            .arg("run")
            .arg(dev)
            .args(args)
            .status()
    };
    let code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);
    std::process::exit(code);
}

fn which(bin: &str) -> bool {
    let sep = if cfg!(windows) { ';' } else { ':' };
    std::env::var("PATH")
        .map(|p| {
            p.split(sep)
                .any(|d| std::path::Path::new(d).join(bin).exists())
        })
        .unwrap_or(false)
}

fn cmd_doctor() -> Result<()> {
    println!("{}", bold("Lectern doctor"));

    match Engine::open_default() {
        Ok(_) => println!("  {GREEN}✓{RESET} engine + local store ready"),
        Err(e) => println!("  {RED}✗{RESET} engine: {e}"),
    }

    let cc = ClaudeCodeBackend::new();
    match cc.version() {
        Some(v) => {
            println!("  {GREEN}✓{RESET} Claude Code {}", bold(&v));
            println!(
                "    {}",
                dim("default = plan mode · --apply edits in place · --yolo runs commands")
            );
            let discovered = lectern_engine::backend::discover_claude_models();
            if !discovered.is_empty() {
                let names: Vec<&str> = discovered.iter().take(3).map(|(_, l)| l.as_str()).collect();
                let more = if discovered.len() > 3 { ", …" } else { "" };
                println!(
                    "    {}",
                    dim(&format!(
                        "models on this account: {}{more}",
                        names.join(", ")
                    ))
                );
            }
        }
        None => {
            println!("  {RED}✗{RESET} Claude Code not found");
            println!(
                "    {}",
                dim("install: npm i -g @anthropic-ai/claude-code   then run `claude` once to log in")
            );
        }
    }

    let ag = AntigravityBackend::new();
    if ag.available() {
        println!(
            "  {GREEN}✓{RESET} Antigravity {}",
            dim("(agy · Gemini models + Conductor provider)")
        );
    } else {
        println!(
            "  {DIM}○{RESET} Antigravity not found {}",
            dim("(optional)")
        );
        println!(
            "    {}",
            dim("install the Antigravity CLI (agy), then run `agy` once to log in")
        );
    }

    let oc = OpenCodeBackend::new();
    if oc.available() {
        println!(
            "  {GREEN}✓{RESET} OpenCode {}",
            dim(&format!(
                "({} · OpenRouter + many providers; free models built in)",
                oc.version().unwrap_or_else(|| "installed".into())
            ))
        );
    } else {
        println!(
            "  {DIM}○{RESET} OpenCode not found {}",
            dim("(optional — install from opencode.ai for OpenRouter & more)")
        );
    }

    let or_models = lectern_engine::backend::discover_openrouter_models();
    if !or_models.is_empty() {
        let free_n = or_models
            .iter()
            .filter(|(id, _)| id.ends_with(":free"))
            .count();
        println!(
            "  {GREEN}✓{RESET} OpenRouter {}",
            dim(&format!(
                "(via opencode · {} models, {free_n} free)",
                or_models.len()
            ))
        );
    } else if OpenCodeBackend::new().available() {
        println!(
            "  {DIM}·{RESET} OpenRouter {}",
            dim("not connected — `opencode auth login` → OpenRouter (has free models)")
        );
    }

    let ollama_models = lectern_engine::backend::discover_ollama_models();
    if !ollama_models.is_empty() {
        println!(
            "  {GREEN}✓{RESET} Ollama {}",
            dim(&format!("(local · {} models)", ollama_models.len()))
        );
    } else {
        println!(
            "  {DIM}·{RESET} Ollama {}",
            dim("not running — ollama.com, then `ollama pull llama3`")
        );
    }

    let sock = lecternd_socket_path();
    if daemon_alive(&sock) {
        println!(
            "  {GREEN}✓{RESET} lecternd running {}",
            dim("(scheduler active)")
        );
    } else if sock.exists() {
        println!(
            "  {DIM}○{RESET} lecternd socket stale {}",
            dim("(unresponsive — restart lecternd)")
        );
    } else {
        println!(
            "  {DIM}○{RESET} lecternd not running {}",
            dim("(optional — schedules also run via `lectern schedule run-due`)")
        );
    }

    match cloud::load_auth() {
        Some(a) => println!(
            "  {GREEN}✓{RESET} cloud: signed in {}",
            dim(&format!("· {}", a.base_url))
        ),
        None => println!(
            "  {DIM}○{RESET} cloud: not signed in {}",
            dim("(optional — run `lectern login`)")
        ),
    }

    println!();
    if cc.available() {
        println!(
            "{}",
            dim("ready → lectern run \"<task>\"           (plan; review first)")
        );
        println!(
            "{}",
            dim("        lectern run \"<task>\" --apply   (let Claude Code make the edits)")
        );
    } else {
        println!(
            "{}",
            dim("install Claude Code above, then: lectern run \"<task>\"")
        );
    }
    Ok(())
}

fn cmd_daemon_status() -> Result<()> {
    let oc = OpenCodeBackend::new();
    if oc.available() {
        println!(
            "  {GREEN}✓{RESET} OpenCode {}",
            dim(&format!(
                "({} · OpenRouter + many providers; free models built in)",
                oc.version().unwrap_or_else(|| "installed".into())
            ))
        );
    } else {
        println!(
            "  {DIM}○{RESET} OpenCode not found {}",
            dim("(optional — install from opencode.ai for OpenRouter & more)")
        );
    }

    let or_models = lectern_engine::backend::discover_openrouter_models();
    if !or_models.is_empty() {
        let free_n = or_models
            .iter()
            .filter(|(id, _)| id.ends_with(":free"))
            .count();
        println!(
            "  {GREEN}✓{RESET} OpenRouter {}",
            dim(&format!(
                "(via opencode · {} models, {free_n} free)",
                or_models.len()
            ))
        );
    } else if OpenCodeBackend::new().available() {
        println!(
            "  {DIM}·{RESET} OpenRouter {}",
            dim("not connected — `opencode auth login` → OpenRouter (has free models)")
        );
    }

    let ollama_models = lectern_engine::backend::discover_ollama_models();
    if !ollama_models.is_empty() {
        println!(
            "  {GREEN}✓{RESET} Ollama {}",
            dim(&format!("(local · {} models)", ollama_models.len()))
        );
    } else {
        println!(
            "  {DIM}·{RESET} Ollama {}",
            dim("not running — ollama.com, then `ollama pull llama3`")
        );
    }

    let sock = lecternd_socket_path();
    if daemon_alive(&sock) {
        println!(
            "{GREEN}●{RESET} lecternd running at {}",
            dim(&sock.to_string_lossy())
        );
    } else if sock.exists() {
        println!(
            "{DIM}○{RESET} lecternd socket present but unresponsive {}",
            dim("(stale — restart lecternd)")
        );
    } else {
        println!(
            "{DIM}○{RESET} lecternd not running — the engine currently runs embedded in the CLI."
        );
        println!("  {}", dim("start it with: lecternd"));
    }
    Ok(())
}

/// True when a live daemon answers ping on the socket (a socket FILE alone can
/// be stale after a crash) — same probe lecternd uses for single-instance.
/// Non-unix: the daemon listens on 127.0.0.1 with a port+token pair in
/// ~/.lectern — probe that.
#[cfg(not(unix))]
fn daemon_alive(_sock: &std::path::Path) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let dir = lectern_engine::data_dir();
    let (Ok(port), Ok(token)) = (
        std::fs::read_to_string(dir.join("lecternd.port")),
        std::fs::read_to_string(dir.join("lecternd.token")),
    ) else {
        return false;
    };
    let Ok(port) = port.trim().parse::<u16>() else {
        return false;
    };
    let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let timeout = Some(std::time::Duration::from_millis(500));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
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

#[cfg(unix)]
fn daemon_alive(sock: &std::path::Path) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let Ok(stream) = std::os::unix::net::UnixStream::connect(sock) else {
        return false;
    };
    let timeout = Some(std::time::Duration::from_millis(500));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
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

fn lecternd_socket_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("lectern").join("lecternd.sock")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}
fn pad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len >= n {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(n - len))
    }
}
