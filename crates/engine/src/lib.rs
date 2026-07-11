//! Lectern engine — the local "V8". Owns workspaces, sessions, the backend adapter
//! layer, the normalized event stream, and the local store. The CLI embeds this
//! directly; `lecternd` will expose it over IPC. See Lectern-Brain/03-Architecture.
pub mod a2a;
pub mod audit;
pub mod backend;
pub mod checkpoint;
pub mod codegraph;
pub mod diag;
pub mod embed;
pub mod event;
pub mod harness_mcp;
pub mod mcp;
pub mod orchestrator;
pub mod registry;
pub mod route;
pub mod securebundle;
pub mod skillstats;
pub mod store;

use anyhow::Result;
use directories::ProjectDirs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::backend::{Backend, ProposedChange, TurnContext, TurnOutcome, Usage};
use crate::event::AgentEvent;
use crate::store::Store;

pub use backend::{
    AntigravityBackend, ClaudeCodeBackend, LimitBackend, MockBackend, OpenCodeBackend,
};

/// Builds a backend for a (backend_id, model) pair — passed into the Conductor so the
/// engine stays backend-agnostic (the CLI/desktop wire it to their build_backend).
pub type BackendFactory<'a> = dyn Fn(&str, Option<String>) -> Box<dyn Backend> + Sync + 'a;

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lectern's data home — a clean, visible ~/.lectern (like .claude) holding the
/// global brain (skills, memory index, sessions, schedules).
/// The user's home directory, cross-platform (HOME on unix, USERPROFILE on
/// Windows — the G2 portability sweep runs everything through here).
pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into())
}

/// Per-process index freshness (latency: walking + embedding a repo cost ~2s
/// PER MESSAGE on a mid-size repo — measured — while recall itself is
/// milliseconds). One index per workspace per 2 minutes keeps the brain
/// current without taxing every turn; a fresh process always reindexes.
fn index_marks() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>
{
    static M: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}
fn index_is_stale(ws_id: &str) -> bool {
    index_marks()
        .lock()
        .ok()
        .and_then(|m| m.get(ws_id).map(|t| t.elapsed().as_secs() >= 120))
        .unwrap_or(true)
}
fn mark_indexed(ws_id: &str) {
    if let Ok(mut m) = index_marks().lock() {
        m.insert(ws_id.to_string(), std::time::Instant::now());
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(home_dir()).join(".lectern")
}

/// The always-on machine profile file (learned via `learn_system`, injected into runs).
pub fn system_profile_path() -> PathBuf {
    data_dir().join("system.md")
}

/// The user model — an editable profile of how THIS person works (stack,
/// review strictness, verbosity, standing preferences). Injected into every
/// run alongside the machine profile. Plain markdown; the user owns it.
pub fn user_profile_path() -> PathBuf {
    data_dir().join("user.md")
}

/// The pre-~/.lectern location (XDG ProjectDirs), used once to migrate existing data.
fn legacy_data_dir() -> Option<PathBuf> {
    ProjectDirs::from("ai", "Lectern", "Lectern").map(|p| p.data_dir().to_path_buf())
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,
    pub root: PathBuf,
    pub name: String,
}

/// Options for a run. `worktree` isolates a concurrent write-session in its own git
/// worktree/branch so parallel agents in one repo don't clobber (ADR-022).
#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptions {
    pub apply: bool,
    pub worktree: bool,
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

pub struct RunResult {
    pub session_id: String,
    pub workspace_root: PathBuf,
    pub changes: Vec<ProposedChange>,
    pub usage: Usage,
    pub applied: bool,
    pub worktree: Option<Worktree>,
    /// True if the backend reported a usage/rate limit (drives auto-continue).
    pub limit_hit: bool,
}

pub struct IndexSummary {
    pub files: u64,
    pub bytes: u64,
}

/// Kill-switch for the persistent brain. When `LECTERN_NO_BRAIN=1`, memory
/// recall and learned-skill matching are skipped, so a run relies only on the
/// base agent's own context — a no-memory mode, and the control arm that
/// measures how much the brain actually contributes.
fn brain_disabled() -> bool {
    std::env::var("LECTERN_NO_BRAIN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// One item the Adaptive Context Builder selected for the payload.
pub struct ContextItem {
    pub path: String,
    pub bytes: u64,
    pub tokens: u64,
    pub reason: String,
}

/// What the engine would feed a backend for a prompt + why + token cost.
pub struct ContextManifest {
    pub prompt: String,
    pub recalls: Vec<String>,
    pub included: Vec<ContextItem>,
    pub skills_applied: Vec<String>,
    pub token_estimate: u64,
    pub budget_tokens: u64,
    pub truncated: bool,
}

/// Rough token estimate (~4 chars/token). Real builder uses the provider tokenizer.
fn est_tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(4)
}

/// How many times the same command must run before a run is flagged as possibly stuck.
const STUCK_RUN_THRESHOLD: u32 = 3;

/// Watches a run's terminal commands and flags when it looks stuck — the same command
/// failing over and over, a common way an agent burns tokens in a loop. Warns once per
/// command so the notice doesn't itself spam the transcript.
#[derive(Default)]
struct StuckDetector {
    counts: HashMap<String, u32>,
    warned: HashSet<String>,
}
impl StuckDetector {
    /// Record a finished command; return a one-time warning if it's now repeated + failing.
    fn observe(&mut self, command: &str, exit_code: i32) -> Option<String> {
        let n = {
            let c = self.counts.entry(command.to_string()).or_insert(0);
            *c += 1;
            *c
        };
        if n >= STUCK_RUN_THRESHOLD && exit_code != 0 && self.warned.insert(command.to_string()) {
            let short: String = command.chars().take(60).collect();
            Some(format!(
                "this run looks stuck — `{short}` has run {n}× and keeps failing (exit {exit_code}); consider stopping it"
            ))
        } else {
            None
        }
    }
}

/// A short "what this file is about" header prepended to content before embedding, so
/// the vector reflects the file's *purpose* — its path plus its title/first meaningful
/// line — not only its raw token soup. This is contextual retrieval: it lets a query
/// about a file's topic ("the scheduler") match `src/engine/scheduler.rs` even when the
/// body never repeats that word, which measurably lowers recall misses.
fn contextual_prefix(rel: &str, content: &str) -> String {
    // Path components carry strong topic signal; split them into words to embed.
    let path_words = rel.replace(['/', '_', '-', '.'], " ");
    // First meaningful line — a markdown title, a doc comment, or the first definition
    // for most code — skipping empties and lines that are just punctuation/brackets.
    let title: String = content
        .lines()
        .map(str::trim)
        .find(|l| l.len() >= 3 && !l.starts_with(['{', '}', '[', ']', '(', ')', ';', ',']))
        .unwrap_or("")
        .chars()
        .take(120)
        .collect();
    format!("{path_words}\n{title}")
}

/// The slice of a recalled file worth putting in context — the `max_lines` window
/// with the most query-token overlap, not the whole file. A 2000-line file with one
/// relevant function costs a handful of lines here instead of the entire thing; this
/// is what keeps recall from draining tokens when content (not just a path) is needed.
/// Returns (excerpt, truncated). Falls back to the head of the file when nothing
/// matches, so a caller always gets *something* representative.
fn relevant_snippet(content: &str, query: &str, max_lines: usize) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        return (content.to_string(), false);
    }
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_string())
        .collect();
    // Per-line relevance = number of query terms it contains.
    let line_score = |l: &str| -> usize {
        if terms.is_empty() {
            return 0;
        }
        let low = l.to_lowercase();
        terms.iter().filter(|t| low.contains(t.as_str())).count()
    };
    // Slide a max_lines window; pick the one with the highest summed score.
    let scores: Vec<usize> = lines.iter().map(|l| line_score(l)).collect();
    let mut best_start = 0usize;
    let mut window: usize = scores.iter().take(max_lines).sum();
    let mut best_sum = window;
    for start in 1..=lines.len().saturating_sub(max_lines) {
        window = window - scores[start - 1] + scores[start + max_lines - 1];
        if window > best_sum {
            best_sum = window;
            best_start = start;
        }
    }
    let end = (best_start + max_lines).min(lines.len());
    let body = lines[best_start..end].join("\n");
    let header = format!("… lines {}-{} of {} …\n", best_start + 1, end, lines.len());
    (format!("{header}{body}"), true)
}

/// Today's date as YYYY-MM-DD (UTC), without pulling in a date crate.
fn today_ymd() -> String {
    let days = now_ts().div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-from-civil inverse: epoch-day count → (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A learned skill (subject-keyed; see [[Shared Memory & Hive-Mind Learning]]).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillBody {
    pub rules: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub id: String,
    pub scope: String,
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub body: SkillBody,
    pub uses: i64,
}

/// A portable, shareable skill bundle — the export/import + marketplace wire format. No local
/// ids or use-counts, so it travels cleanly between machines.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillBundle {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default = "one")]
    pub version: u32,
    /// Optional markdown documentation, rendered natively in the app.
    #[serde(default)]
    pub docs: Option<String>,
}
fn one() -> u32 {
    1
}

pub struct Engine {
    store: Store,
    embedder: Box<dyn embed::Embedder>,
}

impl Engine {
    pub fn open_default() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir)?;
        let db = dir.join("lectern.db");
        // One-time migration from the old XDG location so the brain isn't lost.
        if !db.exists() {
            if let Some(old) = legacy_data_dir() {
                let old_db = old.join("lectern.db");
                if old_db.exists() {
                    let _ = std::fs::copy(&old_db, &db);
                }
            }
        }
        let store = Store::open(&db)?;
        // The brain is global: ensure all learned skills are shared across workspaces.
        let _ = store.globalize_skills();
        Ok(Self {
            store,
            embedder: Box::new(embed::HashEmbedder::new()),
        })
    }

    pub fn with_store(store: Store) -> Self {
        Self {
            store,
            embedder: Box::new(embed::HashEmbedder::new()),
        }
    }

    /// At-a-glance token usage — see Store::usage_stats.
    pub fn usage_stats(&self) -> Result<serde_json::Value> {
        self.store.usage_stats()
    }

    pub fn open_workspace(&self, path: &Path) -> Result<Workspace> {
        let root = std::fs::canonicalize(path)?;
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".into());
        let root_str = root.to_string_lossy().to_string();
        let id = match self.store.workspace_id_for_root(&root_str)? {
            Some(id) => id,
            None => Uuid::new_v4().to_string(),
        };
        self.store
            .upsert_workspace(&id, &root_str, &name, now_ts())?;
        Ok(Workspace { id, root, name })
    }

    /// Build the memory index for a workspace: walk text files and load them into
    /// the FTS5 lexical index (Memory v1). The vector index (sqlite-vec + local
    /// embeddings) layers on top of this next. Returns (files, bytes) indexed.
    pub fn index_workspace(&self, ws: &Workspace) -> Result<IndexSummary> {
        self.store.clear_file_index(&ws.id)?;
        self.store.clear_vectors(&ws.id)?;
        let mut files = 0u64;
        let mut bytes = 0u64;
        let mut indexed: Vec<(String, String)> = Vec::new();
        collect_text_files(&ws.root, &ws.root, &mut indexed, 0);
        for (rel, content) in &indexed {
            bytes += content.len() as u64;
            files += 1;
            self.store.index_file(&ws.id, rel, content)?;
            // vector embedding for semantic recall, over a contextual header + content
            // so the vector captures what the file is about, not just its raw tokens.
            let v = self
                .embedder
                .embed(&format!("{}\n{content}", contextual_prefix(rel, content)));
            self.store
                .index_vector(&ws.id, rel, v.len() as i64, &embed::to_bytes(&v))?;
        }
        Ok(IndexSummary { files, bytes })
    }

    /// Hybrid recall: fuse lexical (FTS) and vector (cosine) results via reciprocal-
    /// rank fusion. Either signal can surface a file the other misses.
    pub fn recall(&self, ws: &Workspace, prompt: &str, limit: i64) -> Vec<String> {
        if brain_disabled() {
            return Vec::new();
        }
        let k = limit.max(1) as usize;

        // lexical
        let lexical: Vec<String> = {
            let q = fts_query(prompt);
            if q.is_empty() {
                Vec::new()
            } else {
                self.store
                    .search_files(&ws.id, &q, (k * 2) as i64)
                    .unwrap_or_default()
            }
        };

        // vector (brute-force cosine over stored embeddings), gated by a relevance
        // floor. Without it, recall returns the top-k files for ANY prompt — so a
        // greeting like "hey hows it going" surfaces whatever scored least-badly
        // (unrelated files from a broad workspace), which is noise and can send the
        // agent off reading junk. Calibrated on the hash embedder: greetings cosine
        // <=0.07 against any file, while genuinely relevant files land at 0.18-0.51.
        // 0.12 sits cleanly between, so noise is dropped and real matches survive.
        let qv = self.embedder.embed(prompt);
        let all = self.store.all_vectors(&ws.id).unwrap_or_default();
        let mut scored: Vec<(f32, String)> = all
            .iter()
            .map(|(path, bytes)| (embed::cosine(&qv, &embed::from_bytes(bytes)), path.clone()))
            .filter(|(score, _)| *score >= RECALL_RELEVANCE_FLOOR)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let vector: Vec<String> = scored.into_iter().take(k * 2).map(|(_, p)| p).collect();

        // Path/filename signal: files whose PATH contains a query word — a precise "you
        // named a file" hint ("the scheduler" → src/scheduler.rs) that content embeddings
        // can miss or drop under the relevance floor. Fused as a third signal, not a
        // rerank, so RRF weighs agreement across all three.
        let toks: Vec<String> = prompt
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 4)
            .map(str::to_string)
            .collect();
        let path_hits: Vec<String> = if toks.is_empty() {
            Vec::new()
        } else {
            let mut ph: Vec<(usize, String)> = all
                .iter()
                .filter_map(|(path, _)| {
                    let pl = path.to_lowercase();
                    let n = toks.iter().filter(|t| pl.contains(t.as_str())).count();
                    (n > 0).then_some((n, path.clone()))
                })
                .collect();
            ph.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            ph.into_iter().take(k * 2).map(|(_, p)| p).collect()
        };

        let (nlex, nvec, npath) = (lexical.len(), vector.len(), path_hits.len());
        let mut out = rrf(&[lexical, vector, path_hits], k);
        // Phase B: fold in graphify code-graph symbols relevant to the prompt (functions/
        // types/files), so the agent starts knowing the relevant code structure. No-op when
        // no graph has been built for this workspace.
        out.extend(crate::codegraph::recall_symbols(&ws.root, prompt, 3));
        crate::diag::log(
            "recall",
            &format!(
                "{} hit(s) fused from {nlex} lexical + {nvec} vector + {npath} path (k={k})",
                out.len()
            ),
        );
        out
    }

    /// Summary of the workspace's graphify code graph (for the Brain view). `built: false`
    /// when no graph has been generated yet.
    pub fn code_graph_summary(&self, ws: &Workspace) -> crate::codegraph::CodeGraphSummary {
        crate::codegraph::summary(&ws.root)
    }

    /// Code-graph symbols relevant to a prompt — folded into recall so the agent starts with
    /// the right functions/types/files. Empty when no graph exists.
    pub fn code_graph_recall(&self, ws: &Workspace, prompt: &str, limit: usize) -> Vec<String> {
        crate::codegraph::recall_symbols(&ws.root, prompt, limit)
    }

    /// Adaptive Context Builder v1: assemble the budgeted context that *would* be
    /// sent to a backend for `prompt`, with a manifest of what/why/cost. This is the
    /// transparency surface ("why did it use this?") and the seed of the real
    /// token-budgeting pipeline. See Lectern-Brain/03-Architecture/Adaptive Context Builder.md.
    pub fn assemble_context(
        &self,
        ws: &Workspace,
        prompt: &str,
        budget_tokens: u64,
    ) -> ContextManifest {
        let recalls = self.recall(ws, prompt, 8);
        let mut included = Vec::new();
        let mut token_estimate: u64 = est_tokens(prompt); // the task itself
        let mut truncated = false;
        for path in &recalls {
            let full = ws.root.join(path);
            let Ok(content) = std::fs::read_to_string(&full) else {
                continue;
            };
            // Only the query-relevant window enters context, not the whole file.
            let (snippet, snipped) = relevant_snippet(&content, prompt, RECALL_SNIPPET_LINES);
            let toks = est_tokens(&snippet);
            if token_estimate + toks > budget_tokens {
                truncated = true;
                // include a signature-only entry instead of the whole file
                included.push(ContextItem {
                    path: path.clone(),
                    bytes: content.len() as u64,
                    tokens: 0,
                    reason: "recalled — omitted (over budget)".into(),
                });
                continue;
            }
            token_estimate += toks;
            included.push(ContextItem {
                path: path.clone(),
                bytes: content.len() as u64,
                tokens: toks,
                reason: if snipped {
                    "recalled (hybrid: lexical+vector) — relevant snippet only"
                } else {
                    "recalled (hybrid: lexical+vector)"
                }
                .into(),
            });
        }
        // Matched skills are injected into context (their rules/recipe) + counted.
        let skills = self.match_skills(ws, prompt, 3);
        let mut skills_applied = Vec::new();
        for s in &skills {
            let body_text = format!("{}\n{}", s.body.rules.join("\n"), s.body.steps.join("\n"));
            token_estimate += est_tokens(&body_text);
            skills_applied.push(s.name.clone());
        }

        ContextManifest {
            prompt: prompt.to_string(),
            recalls,
            included,
            skills_applied,
            token_estimate,
            budget_tokens,
            truncated,
        }
    }

    /// Snapshot the workspace before a run writes to it, so the user can rewind this turn.
    /// Skipped for the home/default workspace (too big and personal). Records the snapshot
    /// and emits a [`AgentEvent::Checkpoint`] through `sink`. Best-effort: a snapshot
    /// failure is logged, never fatal — a run must never be blocked by checkpointing.
    fn checkpoint_before_run(
        &self,
        ws_root: &Path,
        ws_id: &str,
        session_id: &str,
        label: &str,
        is_home: bool,
        sink: &mut dyn FnMut(AgentEvent),
    ) {
        if is_home {
            return;
        }
        match crate::checkpoint::snapshot(ws_root, label) {
            Ok(Some(id)) => {
                let _ = self
                    .store
                    .record_checkpoint(session_id, ws_id, &id, label, now_ts());
                sink(AgentEvent::Checkpoint {
                    id,
                    label: label.to_string(),
                });
            }
            // Nothing changed since the last checkpoint — it already covers this state.
            Ok(None) => {}
            Err(e) => crate::diag::log("checkpoint", &format!("snapshot skipped: {e}")),
        }
    }

    /// Run one turn against `backend`, streaming normalized events to `sink` while
    /// persisting them. Proposed edits are written to disk only when `apply` is true
    /// (the Apply gate).
    pub fn run<F: FnMut(AgentEvent)>(
        &self,
        ws: &Workspace,
        prompt: &str,
        backend: &dyn Backend,
        opts: RunOptions,
        mut sink: F,
    ) -> Result<RunResult> {
        let session_id = Uuid::new_v4().to_string();
        self.store.create_session(
            &session_id,
            &ws.id,
            &truncate(&session_title(prompt), 60),
            backend.id(),
            now_ts(),
        )?;

        // Optional isolation: run a concurrent write-session in its own git worktree
        // so parallel agents in one repo don't clobber each other (ADR-022).
        let worktree = if opts.worktree {
            Some(self.create_worktree(&ws.root, &session_id[..8])?)
        } else {
            None
        };
        let work_root: PathBuf = worktree
            .as_ref()
            .map(|w| w.path.clone())
            .unwrap_or_else(|| ws.root.clone());

        // Refresh the memory index, then recall relevant files + matching skills.
        // Skip indexing the home dir (the default workspace) — it's huge and personal;
        // the global brain (skills) still applies and the agent can read files directly.
        let is_home = std::fs::canonicalize(home_dir())
            .ok()
            .is_some_and(|h| h == ws.root);
        if !is_home && index_is_stale(&ws.id) {
            let _ = self.index_workspace(ws);
            mark_indexed(&ws.id);
        }
        let recalls = self.recall(ws, prompt, 4);
        // Matched skills, minus any that paused themselves after repeated failures
        // (zero-token self-regulation — see skillstats). Paused ones are surfaced,
        // not silently dropped.
        let all_matched = self.match_skills(ws, prompt, 3);
        let stats = crate::skillstats::load();
        let (skills, paused): (Vec<_>, Vec<_>) = all_matched.into_iter().partition(|s| {
            stats
                .get(&s.name)
                .map(|st| !crate::skillstats::is_paused(st))
                .unwrap_or(true)
        });
        for s in &skills {
            let _ = self.store.bump_skill_use(&s.id);
        }
        // Make Lectern's learned skills available to native agents (Claude Code reads
        // .claude/skills/), so they apply out of the box.
        if backend.id().contains("claude") {
            let _ = self.sync_skills_to_claude(ws, &work_root);
        }

        let ctx = TurnContext {
            workspace_root: &work_root,
            recalls: recalls.clone(),
            skills: skills.iter().map(|s| s.name.clone()).collect(),
            system: self.injected_profile(),
            apply: opts.apply,
        };

        let store = &self.store;
        let sid = session_id.clone();
        let mut idx: i64 = 0;
        let limit_hit = std::cell::Cell::new(false);
        let mut stuck = StuckDetector::default();
        let outcome = {
            let mut wrapper = |ev: AgentEvent| {
                if matches!(ev, AgentEvent::LimitHit { .. }) {
                    limit_hit.set(true);
                }
                // Flag a run that keeps re-running the same failing command (a loop).
                let warning = match &ev {
                    AgentEvent::Terminal {
                        command, exit_code, ..
                    } => stuck.observe(command, *exit_code),
                    _ => None,
                };
                let _ = store.append_event(&sid, idx, &ev, now_ts());
                idx += 1;
                sink(ev);
                if let Some(summary) = warning {
                    let w = AgentEvent::Thought {
                        summary,
                        recalls: vec![],
                    };
                    let _ = store.append_event(&sid, idx, &w, now_ts());
                    idx += 1;
                    sink(w);
                }
            };
            // Snapshot the workspace before the agent writes, so this turn can be rewound.
            // In-place applied runs only — worktree runs are already isolated on a branch.
            if opts.apply && worktree.is_none() {
                let label = truncate(&session_title(prompt), 80);
                self.checkpoint_before_run(
                    &work_root,
                    &ws.id,
                    &session_id,
                    &label,
                    is_home,
                    &mut wrapper,
                );
            }
            // Surface real memory recall at the start of the turn (persisted + shown).
            if !recalls.is_empty() {
                wrapper(AgentEvent::Thought {
                    summary: format!("recalled {} relevant file(s) from memory", recalls.len()),
                    recalls: recalls.clone(),
                });
            }
            // Auto-apply matched skills (their conventions/recipe inform the turn).
            for s in &skills {
                wrapper(AgentEvent::SkillApplied {
                    name: s.name.clone(),
                    why: format!("matched this task ({} step(s))", s.body.steps.len()),
                });
            }
            for s in &paused {
                wrapper(AgentEvent::Thought {
                    summary: format!(
                        "skill \"{}\" matched but is paused (failing) — re-enable it in the Hub",
                        s.name
                    ),
                    recalls: vec![],
                });
            }
            // Record the outcome for every auto-applied skill BEFORE propagating
            // errors — failures are exactly what the stats need to see.
            let turn = backend.run_turn(prompt, &ctx, &mut wrapper);
            if !skills.is_empty() {
                let names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
                crate::skillstats::record_outcome(&names, turn.is_ok(), now_ts());
            }
            turn?
        };

        for c in &outcome.changes {
            self.store.record_change(
                &session_id,
                &c.path,
                c.added as i64,
                c.removed as i64,
                if opts.apply { "applied" } else { "pending" },
            )?;
        }

        let mut applied = false;
        if opts.apply {
            for c in &outcome.changes {
                if let Some(content) = &c.new_content {
                    let target = work_root.join(&c.path);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, content)?;
                    applied = true;
                }
            }
        }

        self.store.finish_session(&session_id, "idle")?;
        Ok(RunResult {
            session_id,
            workspace_root: work_root,
            changes: outcome.changes,
            usage: outcome.usage,
            applied,
            worktree,
            limit_hit: limit_hit.get(),
        })
    }

    /// The Conductor: decompose a task with a planner model, then hand each sub-task off
    /// to the model that excels at it (per [`crate::route`]), streaming the plan +
    /// per-step progress + a summary. `make_backend(backend_id, model)` builds a backend
    /// for the caller's environment (keeps the engine backend-agnostic). v1 runs steps
    /// sequentially in the workspace; parallel worktrees + cross-model review come next.
    /// One direct turn — no plan, no per-step routing, no cross-review — for conversational
    /// or single-answer requests. The Conductor's short-circuit so basic questions don't get
    /// the full multi-model ceremony.
    #[allow(clippy::too_many_arguments)]
    fn conductor_direct(
        &self,
        prompt: &str,
        ws: &Workspace,
        recalls: &[String],
        skills: &[String],
        system: &Option<String>,
        make_backend: &BackendFactory,
        apply: bool,
        claude_ok: bool,
        agy_ok: bool,
        session_id: String,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<RunResult> {
        let r = crate::route::available_route(crate::route::route_model(prompt), claude_ok, agy_ok);
        let be = make_backend(&r.backend, Some(r.model.clone()));
        let ctx = TurnContext {
            workspace_root: &ws.root,
            recalls: recalls.to_vec(),
            skills: skills.to_vec(),
            system: system.clone(),
            // Honor the run's apply flag — so a NO_PLAN'd real task (not just a greeting) still
            // makes its edits, rather than silently planning.
            apply,
        };
        let outcome = be.run_turn(prompt, &ctx, sink)?;
        sink(AgentEvent::Usage {
            input_tokens: outcome.usage.input_tokens,
            output_tokens: outcome.usage.output_tokens,
        });
        sink(AgentEvent::Done);
        self.store.finish_session(&session_id, "idle")?;
        Ok(RunResult {
            session_id,
            workspace_root: ws.root.clone(),
            changes: vec![],
            usage: outcome.usage,
            applied: false,
            worktree: None,
            limit_hit: false,
        })
    }

    pub fn run_conductor(
        &self,
        ws: &Workspace,
        prompt: &str,
        make_backend: &BackendFactory,
        apply: bool,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<RunResult> {
        let session_id = Uuid::new_v4().to_string();
        self.store.create_session(
            &session_id,
            &ws.id,
            &truncate(&session_title(prompt), 60),
            "conductor",
            now_ts(),
        )?;

        let is_home = std::fs::canonicalize(home_dir())
            .ok()
            .is_some_and(|h| h == ws.root);
        // Snapshot the workspace before the Conductor writes anything, so the whole
        // multi-step run can be rewound as one checkpoint (covers the direct + planned paths).
        if apply {
            let label = truncate(&session_title(prompt), 80);
            self.checkpoint_before_run(&ws.root, &ws.id, &session_id, &label, is_home, sink);
        }
        if !is_home && index_is_stale(&ws.id) {
            let _ = self.index_workspace(ws);
            mark_indexed(&ws.id);
        }
        let recalls = self.recall(ws, prompt, 4);
        let skills = self.match_skills(ws, prompt, 3);
        let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
        let system = self.injected_profile();
        // Make learned skills available to any Claude steps (writes .claude/skills/lectern-*).
        let _ = self.sync_skills_to_claude(ws, &ws.root);

        // Which agent CLIs are actually connected — so routing only ever picks an available
        // provider (works with Claude Code only, Antigravity only, both, or neither).
        let claude_ok = crate::backend::ClaudeCodeBackend::new().available();
        let agy_ok = crate::backend::AntigravityBackend::new().available();

        // Trivial / conversational input ("hello", "thanks") gets a direct answer — no
        // plan→execute theater, no "Conductor — planning with Haiku…" for a greeting.
        if crate::orchestrator::is_conversational(prompt) {
            return self.conductor_direct(
                prompt,
                ws,
                &recalls,
                &skill_names,
                &system,
                make_backend,
                apply,
                claude_ok,
                agy_ok,
                session_id,
                sink,
            );
        }

        // Surface what the brain is contributing to this run, so the orchestration is
        // legible: recalled files/symbols + (if learned) the machine profile.
        {
            let mut items = recalls.clone();
            if system
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
            {
                items.insert(0, "your machine profile (from Learn my system)".into());
            }
            if !items.is_empty() {
                sink(AgentEvent::Thought {
                    summary: format!(
                        "started with {} context item(s) from your brain",
                        items.len()
                    ),
                    recalls: items,
                });
            }
        }

        // 1) PLAN — a planner model decomposes the task into ordered sub-tasks.
        let pr =
            crate::route::available_route(crate::route::route_model(prompt), claude_ok, agy_ok);
        sink(AgentEvent::Message {
            text: format!("Conductor — planning with {}…", pr.label),
        });
        let planner = make_backend(&pr.backend, Some(pr.model.clone()));
        let plan_prompt = format!(
            "{}\n\nTask: {prompt}",
            crate::orchestrator::PLAN_INSTRUCTION
        );
        let plan_ctx = TurnContext {
            workspace_root: &ws.root,
            recalls: recalls.clone(),
            skills: skill_names.clone(),
            system: system.clone(),
            apply: false,
        };
        let mut plan_text = String::new();
        {
            let mut cap = |ev: AgentEvent| match &ev {
                AgentEvent::MessageDelta { text } => plan_text.push_str(text),
                AgentEvent::Message { text } => {
                    plan_text.push_str(text);
                    plan_text.push('\n');
                }
                _ => {}
            };
            planner.run_turn(&plan_prompt, &plan_ctx, &mut cap)?;
        }
        // Backstop: if the planner judged the request needs no plan (conversational / a single
        // direct answer it couldn't have known to short-circuit earlier), answer directly —
        // no per-step routing, no cross-review.
        if crate::orchestrator::no_plan(&plan_text) {
            return self.conductor_direct(
                prompt,
                ws,
                &recalls,
                &skill_names,
                &system,
                make_backend,
                apply,
                claude_ok,
                agy_ok,
                session_id,
                sink,
            );
        }
        let steps = crate::orchestrator::parse_plan(&plan_text, prompt);
        crate::diag::log("conductor", &format!("plan: {} step(s)", steps.len()));
        sink(AgentEvent::Plan {
            steps: steps
                .iter()
                .map(|s| crate::event::PlanStep {
                    done: false,
                    text: s.title.clone(),
                })
                .collect(),
        });

        // 2) EXECUTE — hand each sub-task to its routed model. Consecutive steps the
        // planner marked `parallel` run concurrently in isolated git worktrees (C2),
        // then merge back; everything else stays sequential. Context hand-off threads a
        // log of completed work into each step so handed-off models build on prior steps.
        let n = steps.len();
        let mut total = Usage::default();
        let mut all_changes: Vec<ProposedChange> = Vec::new();
        let mut applied = false;
        let mut done: Vec<String> = Vec::new();

        // Parallelism is gated to a CLEAN git repo with apply on — otherwise sequential.
        let is_git = Command::new("git")
            .current_dir(&ws.root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        // Clean = no USER changes pending (ignore Lectern's own .claude/.lectern scaffolding,
        // which sync_skills_to_claude writes). A clean repo lets us fork worktrees safely.
        let repo_clean = is_git
            && !git_porcelain(&ws.root)
                .iter()
                .any(|p| !p.starts_with(".claude") && !p.starts_with(".lectern"));

        for group in crate::orchestrator::parallel_groups(&steps) {
            let prior = if done.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\nAlready completed by earlier steps — the project ALREADY reflects this, so build on it; do NOT redo it or re-explore the project from scratch:\n{}",
                    done.join("\n")
                )
            };
            let parallel = apply && repo_clean && group.len() > 1;

            // (StepRun, files merged from its worktree) for each step, in group order.
            let mut outcomes: Vec<(StepRun, Vec<String>)> = Vec::new();

            if parallel {
                sink(AgentEvent::Message {
                    text: format!(
                        "Running {} steps in parallel (isolated worktrees)…",
                        group.len()
                    ),
                });
                let mut wts: Vec<Worktree> = Vec::new();
                for &i in &group {
                    match self.create_worktree(&ws.root, &format!("{}s{i}", &session_id[..7])) {
                        Ok(wt) => {
                            overlay_worktree(&ws.root, &wt.path);
                            wts.push(wt);
                        }
                        Err(_) => break,
                    }
                }
                if wts.len() == group.len() {
                    let steps_ref = &steps;
                    let prior_ref = &prior;
                    let recalls_ref = &recalls;
                    let skills_ref = &skill_names;
                    let system_ref = &system;
                    let results: Vec<Result<StepRun>> = std::thread::scope(|sc| {
                        let handles: Vec<_> = group
                            .iter()
                            .zip(&wts)
                            .map(|(&i, wt)| {
                                let wp = wt.path.clone();
                                sc.spawn(move || {
                                    let mut buf: Vec<AgentEvent> = Vec::new();
                                    let mut run = run_conductor_step(
                                        make_backend,
                                        i,
                                        n,
                                        &steps_ref[i],
                                        prompt,
                                        prior_ref,
                                        &wp,
                                        recalls_ref,
                                        skills_ref,
                                        system_ref,
                                        apply,
                                        claude_ok,
                                        agy_ok,
                                        &mut |ev| buf.push(ev),
                                    )?;
                                    run.events = buf;
                                    Ok::<StepRun, anyhow::Error>(run)
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .zip(group.iter())
                            .map(|(h, &i)| {
                                h.join().unwrap_or_else(|_| {
                                    Err(anyhow::anyhow!(
                                        "parallel step {}/{} (“{}”) panicked",
                                        i + 1,
                                        n,
                                        steps_ref[i].title
                                    ))
                                })
                            })
                            .collect()
                    });
                    let mut touched: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    for (res, wt) in results.into_iter().zip(&wts) {
                        match res {
                            Ok(mut run) => {
                                for ev in std::mem::take(&mut run.events) {
                                    sink(ev);
                                }
                                let changed = merge_worktree(&wt.path, &ws.root);
                                for c in &changed {
                                    if !touched.insert(c.clone()) {
                                        sink(AgentEvent::Message {
                                            text: format!("Note: two parallel steps both changed {c} — kept the later one"),
                                        });
                                    }
                                }
                                outcomes.push((run, changed));
                            }
                            Err(e) => sink(AgentEvent::Message {
                                text: format!("step failed: {e}"),
                            }),
                        }
                    }
                    for wt in &wts {
                        remove_worktree(&ws.root, wt);
                    }
                } else {
                    for wt in &wts {
                        remove_worktree(&ws.root, wt);
                    }
                    sink(AgentEvent::Message {
                        text: "(worktrees unavailable — running this group sequentially)".into(),
                    });
                    for &i in &group {
                        let run = run_conductor_step(
                            make_backend,
                            i,
                            n,
                            &steps[i],
                            prompt,
                            &prior,
                            &ws.root,
                            &recalls,
                            &skill_names,
                            &system,
                            apply,
                            claude_ok,
                            agy_ok,
                            sink,
                        )?;
                        outcomes.push((run, vec![]));
                    }
                }
            } else {
                for &i in &group {
                    let run = run_conductor_step(
                        make_backend,
                        i,
                        n,
                        &steps[i],
                        prompt,
                        &prior,
                        &ws.root,
                        &recalls,
                        &skill_names,
                        &system,
                        apply,
                        claude_ok,
                        agy_ok,
                        sink,
                    )?;
                    outcomes.push((run, vec![]));
                }
            }

            // Finalize each step in order: record changes, cross-review (C3), hand-off log.
            for (run, changed) in outcomes {
                let step = &steps[run.idx];
                total.input_tokens += run.outcome.usage.input_tokens;
                total.output_tokens += run.outcome.usage.output_tokens;
                for c in &run.outcome.changes {
                    if apply {
                        if let Some(content) = &c.new_content {
                            let target = ws.root.join(&c.path);
                            if let Some(parent) = target.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::write(&target, content)?;
                        }
                    }
                    let _ = self.store.record_change(
                        &session_id,
                        &c.path,
                        c.added as i64,
                        c.removed as i64,
                        if apply { "applied" } else { "pending" },
                    );
                }
                for path in &changed {
                    let _ = self.store.record_change(&session_id, path, 0, 0, "applied");
                }
                if apply && (!run.outcome.changes.is_empty() || !changed.is_empty()) {
                    applied = true;
                }

                // C3: cross-model review of risky steps — a DIFFERENT provider reviews. Only
                // possible when that other provider is actually connected; with one agent,
                // there's no cross-provider reviewer, so skip it.
                let mut review_note = String::new();
                let reviewer_ok = if run.backend_id == "antigravity" {
                    claude_ok
                } else {
                    agy_ok
                };
                if apply
                    && reviewer_ok
                    && crate::orchestrator::should_review(
                        &step.kind,
                        !run.outcome.changes.is_empty() || !changed.is_empty(),
                    )
                {
                    let (rb, rm, rl) = crate::orchestrator::reviewer_for(&run.backend_id);
                    let reviewer = make_backend(&rb, Some(rm));
                    let review_prompt = format!(
                        "You are a cross-model reviewer. A different model just completed step {}/{} of a task: \"{}\" — {}. Read the files it changed and briefly assess whether it correctly accomplishes the step. List any bugs/issues; if it looks correct, say so. Be concise (max ~4 sentences). Do NOT make changes.",
                        run.idx + 1, n, step.title, step.detail
                    );
                    let review_ctx = TurnContext {
                        workspace_root: &ws.root,
                        recalls: vec![],
                        skills: vec![],
                        system: system.clone(),
                        apply: false,
                    };
                    let mut verdict = String::new();
                    let mut cap = |ev: AgentEvent| {
                        if let AgentEvent::Message { text } | AgentEvent::MessageDelta { text } =
                            &ev
                        {
                            verdict.push_str(text);
                        }
                    };
                    if reviewer
                        .run_turn(&review_prompt, &review_ctx, &mut cap)
                        .is_ok()
                    {
                        let v = verdict.trim();
                        if !v.is_empty() {
                            sink(AgentEvent::Message {
                                text: format!("Cross-review ({rl}): {v}"),
                            });
                            review_note = v.chars().take(220).collect();
                        }
                    }
                }

                let summary: String = run.summary.trim().chars().take(400).collect();
                let mut entry = format!("Step {}: {}", run.idx + 1, step.title);
                if !summary.is_empty() {
                    entry.push_str(&format!(" — {summary}"));
                }
                if !review_note.is_empty() {
                    entry.push_str(&format!(" (review: {review_note})"));
                }
                done.push(entry);
                all_changes.extend(run.outcome.changes);
            }
        }

        sink(AgentEvent::Message {
            text: format!("Conductor — completed {n} step(s)."),
        });
        sink(AgentEvent::Usage {
            input_tokens: total.input_tokens,
            output_tokens: total.output_tokens,
        });
        sink(AgentEvent::Done);
        self.store.finish_session(&session_id, "idle")?;
        Ok(RunResult {
            session_id,
            workspace_root: ws.root.clone(),
            changes: all_changes,
            usage: total,
            applied,
            worktree: None,
            limit_hit: false,
        })
    }

    /// Create an isolated git worktree + branch for a session. Requires a git repo
    /// with at least one commit. See Lectern-Brain/10-Advanced-Systems/Multi-Session & Concurrency.md.
    fn create_worktree(&self, repo_root: &Path, short: &str) -> Result<Worktree> {
        let branch = format!("lectern/{short}");
        let wt_dir = repo_root.join(".lectern").join("worktrees").join(short);
        std::fs::create_dir_all(repo_root.join(".lectern").join("worktrees"))?;
        ensure_gitignore(repo_root, ".lectern/");
        let out = Command::new("git")
            .current_dir(repo_root)
            .args(["worktree", "add", "-b", &branch])
            .arg(&wt_dir)
            .arg("HEAD")
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
        if !out.status.success() {
            anyhow::bail!(
                "git worktree failed (need a git repo with a commit): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(Worktree {
            path: wt_dir,
            branch,
        })
    }

    /// Export one session as an encrypted bundle (G3 — the sync payload format).
    pub fn export_session_encrypted(&self, session_id: &str, passphrase: &str) -> Result<String> {
        let events = self.store.session_events(session_id)?;
        if events.is_empty() {
            anyhow::bail!("session {session_id} has no events");
        }
        let title = self
            .store
            .session_title(session_id)?
            .unwrap_or_else(|| "exported session".into());
        let payload = serde_json::json!({
            "lectern_session": 1,
            "session": { "id": session_id, "title": title },
            "events": events
                .iter()
                .filter_map(|t| serde_json::from_str::<serde_json::Value>(t).ok())
                .collect::<Vec<_>>(),
        });
        crate::securebundle::seal(&payload.to_string(), passphrase)
    }

    /// Import an encrypted bundle into this workspace as a NEW session.
    pub fn import_session_encrypted(
        &self,
        ws: &Workspace,
        bundle: &str,
        passphrase: &str,
    ) -> Result<String> {
        let text = crate::securebundle::open(bundle, passphrase)?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        if v.get("lectern_session").and_then(|x| x.as_i64()) != Some(1) {
            anyhow::bail!("not a Lectern session bundle");
        }
        let title = v["session"]["title"]
            .as_str()
            .unwrap_or("imported")
            .to_string();
        let sid = Uuid::new_v4().to_string();
        self.store.create_session(
            &sid,
            &ws.id,
            &truncate(&format!("{title} (imported)"), 60),
            "imported",
            now_ts(),
        )?;
        let empty = vec![];
        let events = v["events"].as_array().unwrap_or(&empty);
        for (i, ev) in events.iter().enumerate() {
            if let Ok(parsed) = serde_json::from_value::<AgentEvent>(ev.clone()) {
                self.store.append_event(&sid, i as i64, &parsed, now_ts())?;
            }
        }
        self.store.finish_session(&sid, "idle")?;
        Ok(sid)
    }

    /// Raw persisted event payloads for one session (TUI history — F1).
    pub fn session_events(&self, session_id: &str) -> Result<Vec<String>> {
        self.store.session_events(session_id)
    }

    pub fn set_session_meta(&self, session_id: &str, meta_json: &str) -> Result<()> {
        self.store.set_session_meta(session_id, meta_json)
    }

    pub fn sessions_with_meta(&self, ws: &Workspace, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.store.sessions_with_meta(&ws.id, limit)
    }

    pub fn clear_finished_schedules(&self) -> Result<usize> {
        self.store.clear_finished_schedules()
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        self.store.rename_session(session_id, title)
    }

    pub fn set_session_pinned(&self, session_id: &str, pinned: bool) -> Result<()> {
        self.store.set_session_pinned(session_id, pinned)
    }

    pub fn session_pinned(&self, session_id: &str) -> Result<bool> {
        self.store.session_pinned(session_id)
    }

    pub fn recent_sessions(&self, ws: &Workspace, limit: i64) -> Result<Vec<store::SessionRow>> {
        self.store.recent_sessions(&ws.id, limit)
    }

    // ── Checkpoints (rewind) ─────────────────────────────────────────────────
    /// The workspace's snapshots, newest first — every point a run can be rewound to.
    /// Reads the shadow-git store directly (the authoritative, always-restorable set).
    pub fn checkpoints(&self, ws: &Workspace) -> Result<Vec<crate::checkpoint::Checkpoint>> {
        crate::checkpoint::list(&ws.root)
    }

    /// Rewind the workspace to checkpoint `id`. Captures a redo checkpoint first, then
    /// restores; returns which paths changed and the redo id (to undo the rewind).
    pub fn rewind(&self, ws: &Workspace, id: &str) -> Result<crate::checkpoint::Restore> {
        crate::checkpoint::restore(&ws.root, id)
    }

    // ── Skills v1 ────────────────────────────────────────────────────────────
    /// Record a reusable skill by distilling a session's event log (in-app
    /// `/record`). Defaults to the workspace's most recent session.
    /// See Lectern-Brain/09-Deep-Dives/Skill Acquisition & Learning (deep).md.
    pub fn record_skill(
        &self,
        ws: &Workspace,
        session_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<Skill> {
        let sid = match session_id {
            Some(s) => s.to_string(),
            None => self
                .store
                .last_session_id(&ws.id)?
                .ok_or_else(|| anyhow::anyhow!("no sessions to record from — run one first"))?,
        };
        let title = self.store.session_title(&sid)?.unwrap_or_default();

        let mut steps: Vec<String> = Vec::new();
        let mut rules: Vec<String> = Vec::new();
        for payload in self.store.session_events(&sid)? {
            let Ok(ev) = serde_json::from_str::<AgentEvent>(&payload) else {
                continue;
            };
            match ev {
                AgentEvent::Plan { steps: ps } => {
                    for s in ps {
                        steps.push(s.text);
                    }
                }
                AgentEvent::FileEdit { path, .. } => steps.push(format!("Edit {path}")),
                AgentEvent::Terminal { command, .. } => {
                    if !is_meta_command(&command) {
                        steps.push(format!("Run `{command}`"));
                    }
                }
                AgentEvent::Thought { recalls, .. } => {
                    for r in recalls {
                        rules.push(format!("Relevant: {r}"));
                    }
                }
                _ => {}
            }
        }
        dedup_preserve(&mut steps);
        dedup_preserve(&mut rules);

        let triggers = tokenize(&title);
        let name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| derive_name(&title));
        let description = format!("Recorded from session: {}", truncate(&title, 60));
        let body = SkillBody { rules, steps };
        let id = Uuid::new_v4().to_string();
        self.store.create_skill(
            &id,
            None, // global: learned skills are shared across all workspaces
            "global",
            &name,
            &description,
            &serde_json::to_string(&triggers)?,
            &serde_json::to_string(&body)?,
            now_ts(),
        )?;
        Ok(Skill {
            id,
            scope: "global".into(),
            name,
            description,
            triggers,
            body,
            uses: 0,
        })
    }

    /// Create a skill directly from given content (used by /record — a captured
    /// demonstration distilled into steps). Triggers are derived from the name.
    pub fn add_skill(
        &self,
        _ws: &Workspace, // skills are global now; kept for API symmetry
        name: &str,
        description: &str,
        steps: Vec<String>,
    ) -> Result<Skill> {
        let triggers = tokenize(name);
        let body = SkillBody {
            rules: Vec::new(),
            steps,
        };
        let id = Uuid::new_v4().to_string();
        self.store.create_skill(
            &id,
            None, // global: learned skills are shared across all workspaces
            "global",
            name,
            description,
            &serde_json::to_string(&triggers)?,
            &serde_json::to_string(&body)?,
            now_ts(),
        )?;
        Ok(Skill {
            id,
            scope: "global".into(),
            name: name.to_string(),
            description: description.to_string(),
            triggers,
            body,
            uses: 0,
        })
    }

    pub fn list_skills(&self, ws: &Workspace) -> Result<Vec<Skill>> {
        Ok(self
            .store
            .list_skills(&ws.id)?
            .into_iter()
            .map(row_to_skill)
            .collect())
    }

    /// Deterministically replay a recorded GUI skill at its captured pace (xdotool/wmctrl) —
    /// no LLM, so it runs immediately instead of being re-reasoned (and second-guessed) by an
    /// agent. Errors if the named skill isn't a recorded GUI macro.
    pub fn replay_skill(
        &self,
        ws: &Workspace,
        name: &str,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<()> {
        // Ports P3d: replay drives xdotool/wmctrl — Linux/X11 only today. Fail
        // with a plain sentence instead of a cryptic spawn error elsewhere
        // (SendInput/CGEvent ports are a future enhancement).
        #[cfg(not(target_os = "linux"))]
        anyhow::bail!(
            "GUI skill replay is Linux-only for now — this skill was recorded with Linux desktop tooling."
        );
        #[cfg(target_os = "linux")]
        {
            let _ = ();
        }
        let sk = self
            .list_skills(ws)?
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("skill not found: {name}"))?;
        let steps = gui_replay_steps(&sk.body.steps).ok_or_else(|| {
            anyhow::anyhow!(
                "\"{}\" isn't a recorded GUI skill — run it through the agent instead",
                sk.name
            )
        })?;
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        sink(AgentEvent::Message {
            text: format!(
                "Replaying \"{}\" — {} recorded action(s)…",
                sk.name,
                steps.len()
            ),
        });
        for (delay, cmd) in &steps {
            std::thread::sleep(std::time::Duration::from_secs_f64(*delay));
            let out = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .env("DISPLAY", &display)
                .output();
            match out {
                Ok(o) if o.status.success() => sink(AgentEvent::Terminal {
                    command: cmd.clone(),
                    output: String::new(),
                    exit_code: 0,
                }),
                Ok(o) => sink(AgentEvent::Terminal {
                    command: cmd.clone(),
                    output: String::from_utf8_lossy(&o.stderr).trim().to_string(),
                    exit_code: o.status.code().unwrap_or(1),
                }),
                Err(e) => sink(AgentEvent::Terminal {
                    command: cmd.clone(),
                    output: e.to_string(),
                    exit_code: 1,
                }),
            }
        }
        sink(AgentEvent::Message {
            text: format!("Replayed \"{}\" ({} actions).", sk.name, steps.len()),
        });
        sink(AgentEvent::Done);
        Ok(())
    }

    /// Indexed memory file paths for a workspace (for the brain graph).
    pub fn memory_files(&self, ws: &Workspace, limit: i64) -> Result<Vec<String>> {
        self.store.list_indexed_files(&ws.id, limit)
    }

    /// Delete a learned skill by name.
    pub fn delete_skill(&self, name: &str) -> Result<()> {
        self.store.delete_skill_by_name(name)
    }

    /// Create or replace a skill by name (hand-authored in the Marketplace, or imported).
    /// This is the editable, portable path — distinct from `record_skill` (GUI capture) and
    /// `add_skill` (session-distilled). Empty `triggers` fall back to the name's tokens.
    pub fn upsert_skill(
        &self,
        name: &str,
        description: &str,
        triggers: Vec<String>,
        rules: Vec<String>,
        steps: Vec<String>,
    ) -> Result<Skill> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("a skill needs a name");
        }
        let _ = self.store.delete_skill_by_name(name); // replace if one already exists
        let triggers = if triggers.iter().all(|t| t.trim().is_empty()) {
            tokenize(name)
        } else {
            triggers
                .into_iter()
                .filter(|t| !t.trim().is_empty())
                .collect()
        };
        let body = SkillBody { rules, steps };
        let id = Uuid::new_v4().to_string();
        self.store.create_skill(
            &id,
            None,
            "global",
            name,
            description,
            &serde_json::to_string(&triggers)?,
            &serde_json::to_string(&body)?,
            now_ts(),
        )?;
        Ok(Skill {
            id,
            scope: "global".into(),
            name: name.to_string(),
            description: description.to_string(),
            triggers,
            body,
            uses: 0,
        })
    }

    /// Load a skill by name (for editing).
    pub fn get_skill(&self, ws: &Workspace, name: &str) -> Option<Skill> {
        self.list_skills(ws)
            .ok()?
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Export a skill as a portable JSON bundle (for sharing / the marketplace).
    pub fn export_skill(&self, ws: &Workspace, name: &str) -> Result<String> {
        let s = self
            .get_skill(ws, name)
            .ok_or_else(|| anyhow::anyhow!("skill not found: {name}"))?;
        let bundle = SkillBundle {
            name: s.name,
            description: s.description,
            triggers: s.triggers,
            rules: s.body.rules,
            steps: s.body.steps,
            author: None,
            version: 1,
            docs: None,
        };
        Ok(serde_json::to_string_pretty(&bundle)?)
    }

    /// Import a skill from a portable JSON bundle.
    pub fn import_skill(&self, json: &str) -> Result<Skill> {
        let b: SkillBundle = serde_json::from_str(json.trim())?;
        self.upsert_skill(&b.name, &b.description, b.triggers, b.rules, b.steps)
    }

    /// Import a skill from SKILL.md text (the open-standard ecosystem format), converting
    /// it to Lectern's model so any of the ecosystem's SKILL.md skills can be installed.
    pub fn import_skill_md(&self, md: &str) -> Result<Skill> {
        let b = parse_skill_md(md)?;
        self.upsert_skill(&b.name, &b.description, b.triggers, b.rules, b.steps)
    }

    /// Browse the community hub — read-only, no auth. Returns the index entries.
    pub fn browse_registry(&self) -> Result<Vec<crate::registry::RegistryEntry>> {
        crate::registry::fetch_index(&crate::registry::config())
    }

    /// Fetch one community skill's full bundle so the UI can SHOW its exact
    /// rules/steps before the user confirms an install (review-before-install).
    /// This only downloads JSON; it never imports or runs anything.
    pub fn fetch_registry_skill(&self, id: &str) -> Result<SkillBundle> {
        crate::registry::fetch_bundle(&crate::registry::config(), id)
    }

    /// Review-modal fetch with integrity: verifies against the index sha256
    /// when the entry carries one (mismatch = hard error). Returns (bundle, verified).
    pub fn fetch_registry_skill_verified(
        &self,
        id: &str,
        expected_sha: Option<&str>,
    ) -> Result<(SkillBundle, bool)> {
        crate::registry::fetch_bundle_verified(&crate::registry::config(), id, expected_sha)
    }

    /// Install a community skill by id: fetch its bundle, import it into the
    /// brain, and record the installed version so the UI can later flag updates.
    /// Call ONLY after the user has reviewed and confirmed. Returns the name.
    pub fn install_registry_skill(&self, id: &str, expected_sha: Option<&str>) -> Result<String> {
        let cfg = crate::registry::config();
        let (bundle, _verified) = crate::registry::fetch_bundle_verified(&cfg, id, expected_sha)?;
        let version = bundle.version;
        let json = serde_json::to_string(&bundle)?;
        let skill = self.import_skill(&json)?;
        crate::registry::record_installed(id, version);
        Ok(skill.name)
    }

    /// Map of hub skill id -> installed version (for "update available" badges).
    pub fn installed_registry_versions(&self) -> std::collections::HashMap<String, u32> {
        crate::registry::load_installed()
    }

    /// Build the GitHub "propose new file" URL that publishes one of the user's
    /// skills to the hub as a pull request (browser-based — no token in the app).
    pub fn publish_url(&self, ws: &Workspace, name: &str) -> Result<String> {
        let json = self.export_skill(ws, name)?;
        let cfg = crate::registry::config();
        let filename = format!("skills/{}.json", crate::registry::slug(name));
        Ok(cfg.new_file_url(&filename, &json))
    }

    /// The community hub's repo page (for a "view on GitHub" link).
    pub fn registry_repo_url(&self) -> String {
        crate::registry::config().repo_url()
    }

    /// Serve this workspace's brain (recall + skills) to MCP clients over stdio.
    pub fn mcp_serve(&self, ws: &Workspace) -> Result<()> {
        crate::mcp::serve_stdio(self, ws)
    }

    /// The always-on machine profile (`~/.lectern/system.md`), if it's been learned.
    /// Injected into every run so agents know the system upfront. Capped to stay light.
    /// The user model (user.md), if written. See `user_profile_path`.
    pub fn user_profile(&self) -> Option<String> {
        let p = user_profile_path();
        std::fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// What actually gets injected each turn: the machine profile plus, when
    /// present, the user model under its own label (one channel, two sections —
    /// backends stay unchanged).
    pub fn injected_profile(&self) -> Option<String> {
        let sys = self.system_profile();
        let user = self.user_profile();
        match (sys, user) {
            (None, None) => None,
            (Some(s), None) => Some(s),
            (None, Some(u)) => Some(format!(
                "[Lectern user] How this user works — honor these preferences:\n{u}"
            )),
            (Some(s), Some(u)) => Some(format!(
                "{s}\n\n[Lectern user] How this user works — honor these preferences:\n{u}"
            )),
        }
    }

    pub fn system_profile(&self) -> Option<String> {
        let p = system_profile_path();
        let s = std::fs::read_to_string(p).ok()?;
        let s = s.trim();
        if s.is_empty() {
            None
        } else if s.len() > 8000 {
            Some(s.chars().take(8000).collect())
        } else {
            Some(s.to_string())
        }
    }

    /// How many days old the learned system profile is (None if never learned).
    pub fn system_profile_age_days(&self) -> Option<u64> {
        let meta = std::fs::metadata(system_profile_path()).ok()?;
        let modified = meta.modified().ok()?;
        let secs = SystemTime::now().duration_since(modified).ok()?.as_secs();
        Some(secs / 86_400)
    }

    /// Learn the user's machine: route an agent to probe the system, then save a concise
    /// Markdown profile to `~/.lectern/system.md`. Streams the agent's work to `sink`.
    pub fn learn_system(
        &self,
        make_backend: &BackendFactory,
        sink: &mut dyn FnMut(AgentEvent),
    ) -> Result<String> {
        const BRIEF: &str = "Profile THIS Linux machine for an AI assistant's long-term memory. \
Run shell commands to discover, as available: OS/distro + version, kernel, hostname, CPU/RAM/GPU, \
desktop environment + display server + monitor layout, shell, package manager(s), installed \
languages/runtimes with versions (python/node/rust/go/java/…), key dev tools (git, docker, etc.), \
important paths, default editor, notable running services, and relevant user shell/dotfile config. \
Be efficient. When done, output ONLY a concise Markdown profile: short '## ' sections with one fact \
per bullet — no preamble, no narration around it.";
        let r = crate::route::route_model(BRIEF);
        sink(AgentEvent::Message {
            text: format!("Learning your system with {}…", r.label),
        });
        let be = make_backend(&r.backend, Some(r.model.clone()));
        let root = PathBuf::from(home_dir());
        let ctx = TurnContext {
            workspace_root: &root,
            recalls: vec![],
            skills: vec![],
            system: None,
            apply: false,
        };
        let mut captured = String::new();
        {
            let mut fwd = |ev: AgentEvent| {
                match &ev {
                    AgentEvent::MessageDelta { text } => captured.push_str(text),
                    AgentEvent::Message { text } => {
                        captured.push_str(text);
                        captured.push('\n');
                    }
                    _ => {}
                }
                sink(ev);
            };
            be.run_turn(BRIEF, &ctx, &mut fwd)?;
        }
        // Extract the Markdown profile (from the first heading) out of the agent's output.
        let profile = match captured.find("\n#").or_else(|| captured.find('#')) {
            Some(idx) => captured[idx..].trim().to_string(),
            None => captured.trim().to_string(),
        };
        if profile.is_empty() {
            anyhow::bail!("the agent produced no profile");
        }
        let path = system_profile_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let dated = format!(
            "<!-- Lectern system profile — learned {} -->\n\n{profile}\n",
            today_ymd()
        );
        std::fs::write(&path, &dated)?;
        sink(AgentEvent::Message {
            text: "Saved your system profile — agents will now know your machine upfront.".into(),
        });
        sink(AgentEvent::Done);
        Ok(profile)
    }

    /// Materialize this workspace's learned skills as native Claude Code skills under
    /// `<root>/.claude/skills/lectern-<slug>/SKILL.md`, so the agent picks them up
    /// natively. Returns how many were written. Removes stale `lectern-*` skill dirs.
    pub fn sync_skills_to_claude(&self, ws: &Workspace, root: &std::path::Path) -> Result<usize> {
        let skills = self.list_skills(ws)?;
        let base = root.join(".claude").join("skills");
        // Clear previously-synced Lectern skills so deletions/renames propagate.
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("lectern-") {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        if skills.is_empty() {
            return Ok(0);
        }
        std::fs::create_dir_all(&base)?;
        for sk in &skills {
            let dir = base.join(format!("lectern-{}", slugify(&sk.name)));
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("SKILL.md"), render_skill_md(sk))?;
        }
        Ok(skills.len())
    }

    /// Match learned skills to a prompt by trigger overlap (lexical v1; embeddings next).
    pub fn match_skills(&self, ws: &Workspace, prompt: &str, limit: usize) -> Vec<Skill> {
        if brain_disabled() {
            return Vec::new();
        }
        let ptoks: HashSet<String> = tokenize(prompt).into_iter().collect();
        let mut scored: Vec<(usize, Skill)> = Vec::new();
        for row in self.store.list_skills(&ws.id).unwrap_or_default() {
            let skill = row_to_skill(row);
            let score = skill.triggers.iter().filter(|t| ptoks.contains(*t)).count();
            if score > 0 {
                scored.push((score, skill));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.uses.cmp(&a.1.uses)));
        scored.into_iter().take(limit).map(|(_, s)| s).collect()
    }

    // ── Scheduling & auto-continue ───────────────────────────────────────────
    /// Schedule a one-shot task to run at `run_at` (unix seconds).
    pub fn schedule_add(
        &self,
        ws: &Workspace,
        prompt: &str,
        backend: &str,
        apply: bool,
        run_at: i64,
        reason: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.store.create_schedule(
            &id,
            &ws.id,
            prompt,
            backend,
            apply,
            run_at,
            reason,
            now_ts(),
        )?;
        Ok(id)
    }

    pub fn list_schedules(&self, ws: &Workspace) -> Result<Vec<store::ScheduleRow>> {
        self.store.list_schedules(&ws.id)
    }

    /// All schedules across every workspace (global Schedule view).
    pub fn list_all_schedules(&self) -> Result<Vec<store::ScheduleRow>> {
        self.store.list_all_schedules()
    }

    pub fn cancel_schedule(&self, id: &str) -> Result<()> {
        self.store.set_schedule_status(id, "cancelled", None)
    }

    /// Cancel by full id or the short prefix `schedule list` prints. Returns true
    /// only if a schedule was actually cancelled.
    pub fn cancel_schedule_prefix(&self, id: &str) -> Result<bool> {
        Ok(self.store.set_schedule_status_by_prefix(id, "cancelled")? > 0)
    }

    /// Schedule a retry of a task `after_secs` from now (auto-continue on limit).
    pub fn schedule_retry(
        &self,
        ws_id: &str,
        prompt: &str,
        backend: &str,
        apply: bool,
        after_secs: i64,
        reason: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.store.create_schedule(
            &id,
            ws_id,
            prompt,
            backend,
            apply,
            now_ts() + after_secs,
            reason,
            now_ts(),
        )?;
        Ok(id)
    }

    /// Resolve a backend by id for unattended/scheduled runs.
    pub fn backend_for(&self, id: &str) -> Box<dyn Backend> {
        match id {
            "claude-code" | "claude" => Box::new(ClaudeCodeBackend::new()),
            "antigravity" | "gemini" => Box::new(AntigravityBackend::new()),
            "opencode" => Box::new(OpenCodeBackend::new()),
            // OpenRouter rides the OpenCode adapter — model ids are `openrouter/...`,
            // available once the user connects OpenRouter inside opencode.
            "openrouter" => Box::new(OpenCodeBackend::new()),
            // Ollama also rides the OpenCode adapter — model ids are `ollama/...`,
            // detected from a locally-running Ollama server.
            "ollama" => Box::new(OpenCodeBackend::new()),
            "mock-limit" => Box::new(LimitBackend),
            "auto" => {
                let cc = ClaudeCodeBackend::new();
                if cc.available() {
                    Box::new(cc)
                } else {
                    Box::new(MockBackend { fast: true })
                }
            }
            _ => Box::new(MockBackend { fast: true }),
        }
    }

    /// Run every schedule whose time has come. On a usage limit, re-schedules a
    /// retry `retry_after_secs` later (auto-continue). Returns the ids that ran.
    pub fn run_due_schedules<F: FnMut(AgentEvent)>(
        &self,
        retry_after_secs: i64,
        mut sink: F,
    ) -> Result<Vec<String>> {
        let mut ran = Vec::new();
        for (id, ws_id, root, prompt, backend_id, apply) in self.store.due_schedules(now_ts())? {
            // Atomic claim — if another runner got here first, skip (double-run guard).
            if !self.store.claim_schedule(&id)? {
                crate::diag::log("schedule", &format!("skip {id}: claimed by another runner"));
                continue;
            }
            let Ok(ws) = self.open_workspace(Path::new(&root)) else {
                crate::diag::log(
                    "schedule",
                    &format!("{id} error: workspace {root} won't open"),
                );
                self.store
                    .set_schedule_status(&id, "error", Some(now_ts()))?;
                continue;
            };
            crate::diag::log("schedule", &format!("run {id} via {backend_id}"));
            let backend = self.backend_for(&backend_id);
            let opts = RunOptions {
                apply,
                worktree: false,
            };
            match self.run(&ws, &prompt, backend.as_ref(), opts, &mut sink) {
                Ok(res) if res.limit_hit => {
                    let _ = self.schedule_retry(
                        &ws_id,
                        &prompt,
                        &backend_id,
                        apply,
                        retry_after_secs,
                        "auto-continue after limit",
                    );
                    crate::diag::log("schedule", &format!("{id} hit a limit — retry queued"));
                    self.store
                        .set_schedule_status(&id, "limit", Some(now_ts()))?;
                }
                Ok(_) => {
                    crate::diag::log("schedule", &format!("{id} done"));
                    self.store
                        .set_schedule_status(&id, "done", Some(now_ts()))?;
                }
                Err(e) => {
                    crate::diag::log("schedule", &format!("{id} failed: {e:#}"));
                    self.store
                        .set_schedule_status(&id, "error", Some(now_ts()))?;
                }
            }
            ran.push(id);
        }
        Ok(ran)
    }
}

/// Lowercase, hyphenated slug for a skill name (used for the skill directory).
fn slugify(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
    }
    let out = s.trim_matches('-').to_string();
    if out.is_empty() {
        "skill".into()
    } else {
        out
    }
}

/// Split a recorded step's optional "[+Ns] " timing prefix from its action text.
/// Returns (seconds-to-wait-before-this-action, action). Defaults to 0.3s for
/// older skills recorded without timing.
fn parse_step(step: &str) -> (f64, &str) {
    if let Some(rest) = step.strip_prefix("[+") {
        if let Some(end) = rest.find("s] ") {
            if let Ok(d) = rest[..end].parse::<f64>() {
                return (d.clamp(0.1, 10.0), &rest[end + 3..]);
            }
        }
    }
    (0.3, step)
}

/// Translate a recorded GUI step ("Click at (x, y) in \"W\"", "Type ...", "Switch
/// to ...") into a literal xdotool/wmctrl command, so a recorded skill is directly
/// executable. Returns None for non-GUI (coding) steps.
fn gui_step_to_cmd(step: &str) -> Option<String> {
    let (_, step) = parse_step(step);
    if let Some(w) = step
        .strip_prefix("Switch to \"")
        .and_then(|s| s.strip_suffix('"'))
    {
        return Some(format!(
            "wmctrl -a {w:?} || xdotool search --name {w:?} windowactivate"
        ));
    }
    if step.starts_with("Click at (") || step.starts_with("Right-click at (") {
        let btn = if step.starts_with("Right") { 3 } else { 1 };
        if let Some(coords) = step.split('(').nth(1).and_then(|s| s.split(')').next()) {
            let nums: Vec<&str> = coords.split(',').map(|s| s.trim()).collect();
            if nums.len() == 2 && nums.iter().all(|n| n.parse::<i32>().is_ok()) {
                return Some(format!(
                    "xdotool mousemove {} {} click {btn}",
                    nums[0], nums[1]
                ));
            }
        }
    }
    if let Some(rest) = step.strip_prefix("Type \"") {
        if let Some(end) = rest.rfind("\" in \"") {
            let text = rest[..end].replace('⏎', ""); // Enter handled separately if needed
            return Some(format!("xdotool type --clearmodifiers -- {text:?}"));
        }
    }
    None
}

/// A recorded GUI macro → its (delay-seconds, shell-command) replay list, or None when the
/// skill isn't a GUI recording (so callers fall back to the agent path for instruction skills).
/// A skill counts as GUI when at least half its steps translate to literal GUI commands.
pub fn gui_replay_steps(steps: &[String]) -> Option<Vec<(f64, String)>> {
    let out: Vec<(f64, String)> = steps
        .iter()
        .filter_map(|s| gui_step_to_cmd(s).map(|c| (parse_step(s).0, c)))
        .collect();
    if !out.is_empty() && out.len() * 2 >= steps.len() {
        Some(out)
    } else {
        None
    }
}

/// Render a learned skill as a Claude Code SKILL.md. Recorded GUI workflows become a
/// self-contained, directly-runnable recipe so the agent can just execute them.
/// Parse a SKILL.md (the open-standard skill format the 2026 ecosystem uses: a YAML-ish
/// frontmatter block plus a markdown body) into Lectern's portable [`SkillBundle`], so
/// ecosystem skills — not just Lectern-recorded ones — can be imported. Frontmatter
/// supplies name/description/(optional triggers); the body's paragraphs become the skill's
/// rules. The inverse of [`render_skill_md`].
fn parse_skill_md(md: &str) -> Result<SkillBundle> {
    let md = md.trim_start_matches('\u{feff}').trim_start();
    // Frontmatter is a `---` … `---` block at the very top.
    let (front, body) = match md.strip_prefix("---") {
        Some(rest) => match rest.find("\n---") {
            Some(end) => (&rest[..end], rest[end + 4..].trim_start_matches('\n')),
            None => ("", md),
        },
        None => ("", md),
    };
    let mut name = String::new();
    let mut description = String::new();
    let mut triggers: Vec<String> = Vec::new();
    for line in front.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("name:") {
            name = v.trim().trim_matches(['"', '\'']).to_string();
        } else if let Some(v) = l.strip_prefix("description:") {
            description = v.trim().trim_matches(['"', '\'']).to_string();
        } else if let Some(v) = l.strip_prefix("triggers:") {
            triggers = v
                .trim()
                .trim_matches(['[', ']'])
                .split([',', ';'])
                .map(|t| t.trim().trim_matches(['"', '\'']).to_string())
                .filter(|t| !t.is_empty())
                .collect();
        }
    }
    // Strip Lectern's own re-export prefix so a round-trip doesn't stack `lectern-lectern-`.
    let name = name.trim_start_matches("lectern-").trim().to_string();
    if name.is_empty() {
        anyhow::bail!("SKILL.md has no `name` in its frontmatter");
    }
    if description.is_empty() {
        description = format!("Imported skill: {name}");
    }
    // Drop a leading `# Title` heading, then keep the instruction paragraphs as rules.
    let body = body.trim();
    let body = match body.lines().next() {
        Some(first) if first.trim_start().starts_with('#') => {
            body.lines().skip(1).collect::<Vec<_>>().join("\n")
        }
        _ => body.to_string(),
    };
    let rules: Vec<String> = body
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    Ok(SkillBundle {
        name,
        description,
        triggers,
        rules,
        steps: Vec::new(),
        author: None,
        version: 1,
        docs: None,
    })
}

fn render_skill_md(sk: &Skill) -> String {
    let triggers = if sk.triggers.is_empty() {
        "matching tasks".to_string()
    } else {
        sk.triggers.join(", ")
    };
    let cmds: Vec<String> = sk
        .body
        .steps
        .iter()
        .filter_map(|s| gui_step_to_cmd(s))
        .collect();
    let is_gui = !cmds.is_empty() && cmds.len() * 2 >= sk.body.steps.len();
    let mut md = format!(
        "---\nname: lectern-{}\ndescription: {}\n---\n\n# {}\n",
        slugify(&sk.name),
        sk.description.replace('\n', " "),
        sk.name,
    );
    if is_gui {
        md.push_str(&format!(
            "\n**What this is:** a recorded GUI workflow on this machine ({} actions). When asked to use this skill, run the commands in the **Run** block below in order with the Bash tool — they are complete and self-contained. Do NOT load other skills, take screenshots, or ask questions; just execute them (DISPLAY=:0) and report the result.\n\n## Recorded actions\n",
            cmds.len()
        ));
        for (i, s) in sk.body.steps.iter().enumerate() {
            md.push_str(&format!("{}. {s}\n", i + 1));
        }
        md.push_str("\n## Run\n```bash\nexport DISPLAY=:0\n");
        // Replay at the recorded pace: wait the captured delay before each action.
        for s in &sk.body.steps {
            if let Some(c) = gui_step_to_cmd(s) {
                let (delay, _) = parse_step(s);
                md.push_str(&format!("sleep {delay:.1}\n{c}\n"));
            }
        }
        md.push_str("```\n");
    } else {
        md.push_str(&format!(
            "\n_A skill Lectern learned for this project — apply it on matching tasks._\n\n## When to use\n{triggers}\n"
        ));
        if !sk.body.rules.is_empty() {
            md.push_str("\n## Rules\n");
            for r in &sk.body.rules {
                md.push_str(&format!("- {r}\n"));
            }
        }
        if !sk.body.steps.is_empty() {
            md.push_str("\n## Steps\n");
            for (i, s) in sk.body.steps.iter().enumerate() {
                md.push_str(&format!("{}. {s}\n", i + 1));
            }
        }
    }
    md
}

fn row_to_skill(row: store::SkillRow) -> Skill {
    let (id, scope, name, description, triggers_json, body_json, uses, _succ) = row;
    Skill {
        id,
        scope,
        name,
        description,
        triggers: serde_json::from_str(&triggers_json).unwrap_or_default(),
        body: serde_json::from_str(&body_json).unwrap_or_default(),
        uses,
    }
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// A clean session title from a possibly preamble-augmented prompt: take the text after
/// the last injected "Task:"/"Brief:" marker (Personal Agent / one-shot), first line only,
/// so stored sessions read as the user's real task — not the injected preamble.
fn session_title(prompt: &str) -> String {
    let body = prompt
        .rsplit_once("\nTask: ")
        .map(|(_, t)| t)
        .or_else(|| prompt.rsplit_once("Brief: ").map(|(_, t)| t))
        .unwrap_or(prompt)
        .trim();
    body.lines().next().unwrap_or(body).trim().to_string()
}

// ── Conductor step execution + parallel worktree helpers (C2) ────────────────
/// Result of running one Conductor step (shared by the sequential + parallel paths).
struct StepRun {
    idx: usize,
    backend_id: String,
    summary: String,
    outcome: TurnOutcome,
    /// Buffered events for a parallel step (flushed in order after the wave); empty when
    /// the step streamed live to the real sink.
    events: Vec<AgentEvent>,
}

/// Route a sub-task, optionally refining AMBIGUOUS ones (those that match no preset rule)
/// with the classifier — the "both" mode (editable presets + classifier). When a preset rule
/// matches, or the classifier is disabled, this is just the preset route with no extra model
/// call. Otherwise a fast model labels the task ("quick"/"heavy"/"general") and we map it.
fn classified_route(
    prompt: &str,
    make_backend: &BackendFactory,
    work_root: &Path,
    claude_ok: bool,
    agy_ok: bool,
) -> crate::route::Route {
    let (base, matched) = crate::route::route_detail(prompt);
    if matched || !crate::route::classifier_enabled() {
        return base;
    }
    let (cb, cm) = crate::route::classifier_target();
    // Skip the classifier turn if its own provider isn't connected — fall back to the preset.
    let cb_ok = matches!(cb.as_str(), "claude-code" if claude_ok)
        || matches!(cb.as_str(), "antigravity" if agy_ok)
        || matches!(cb.as_str(), "auto" | "mock");
    if !cb_ok {
        return base;
    }
    let be = make_backend(&cb, Some(cm));
    let cls = format!(
        "Classify this software task with ONE lowercase word and nothing else — \"quick\" \
         (trivial: typo, rename, format, one-liner), \"heavy\" (architecture, debugging, \
         security, complex reasoning), or \"general\" (normal feature work).\n\nTask: {prompt}"
    );
    let ctx = TurnContext {
        workspace_root: work_root,
        recalls: vec![],
        skills: vec![],
        system: None,
        apply: false,
    };
    let mut out = String::new();
    {
        let mut cap = |ev: AgentEvent| {
            if let AgentEvent::Message { text } | AgentEvent::MessageDelta { text } = &ev {
                out.push_str(text);
            }
        };
        let _ = be.run_turn(&cls, &ctx, &mut cap);
    }
    crate::route::classifier_route(&out).unwrap_or(base)
}

/// The peer this Conductor run delegates steps to, if any. Delegation is opt-in via
/// `LECTERN_A2A_DELEGATE` (a configured peer name); unset → `None`, and the peer file
/// is not even read, so the default path is unchanged.
fn a2a_delegate_target() -> Option<crate::a2a::A2aPeer> {
    let env = std::env::var("LECTERN_A2A_DELEGATE").ok();
    let env = env.as_deref().filter(|s| !s.trim().is_empty())?;
    crate::orchestrator::select_delegate_peer(Some(env), &crate::a2a::load_peers())
}

/// Delegate one Conductor step to a local A2A peer instead of a local backend: send
/// the step, poll to completion, and fold the peer's reply back as the step summary
/// (which the Conductor threads into `prior` for later steps). The peer does the work
/// on its side, so a delegated step produces no local file changes.
fn delegate_step_via_a2a(
    peer: &crate::a2a::A2aPeer,
    idx: usize,
    n: usize,
    step: &crate::orchestrator::ConductorStep,
    overall: &str,
    prior: &str,
    sink: &mut dyn FnMut(AgentEvent),
) -> Result<StepRun> {
    sink(AgentEvent::ModelRouted {
        model: format!("A2A · {}", peer.name),
        reason: format!(
            "step {}/{}: {} — delegated to local A2A peer",
            idx + 1,
            n,
            step.title
        ),
    });
    crate::diag::log(
        "conductor",
        &format!(
            "step {}/{} → A2A peer {} ({})",
            idx + 1,
            n,
            peer.name,
            peer.url
        ),
    );
    let step_prompt = format!(
        "You are step {}/{} of a larger task — do ONLY this step, then stop.\nOverall task: {overall}{prior}\nThis step: {} — {}",
        idx + 1,
        n,
        step.title,
        step.detail
    );
    let task =
        crate::a2a::A2aClient::new().delegate(&peer.url, peer.token.as_deref(), &step_prompt)?;
    let reply = task
        .status
        .message
        .as_ref()
        .map(|m| crate::a2a::Part::joined_text(&m.parts))
        .unwrap_or_default();
    if !reply.is_empty() {
        sink(AgentEvent::Message {
            text: reply.clone(),
        });
    }
    Ok(StepRun {
        idx,
        backend_id: format!("a2a:{}", peer.name),
        summary: reply,
        outcome: TurnOutcome {
            changes: Vec::new(),
            usage: Usage::default(),
        },
        events: Vec::new(),
    })
}

/// Run one Conductor sub-task: route it, hand it to the chosen model, capture its prose.
/// No `&self`, so it can run inside a worktree thread.
#[allow(clippy::too_many_arguments)]
fn run_conductor_step(
    make_backend: &BackendFactory,
    idx: usize,
    n: usize,
    step: &crate::orchestrator::ConductorStep,
    overall: &str,
    prior: &str,
    work_root: &Path,
    recalls: &[String],
    skills: &[String],
    system: &Option<String>,
    apply: bool,
    claude_ok: bool,
    agy_ok: bool,
    sink: &mut dyn FnMut(AgentEvent),
) -> Result<StepRun> {
    // Opt-in: offload this step to a configured local A2A peer (LECTERN_A2A_DELEGATE).
    // Default (unset) falls straight through to local routing, unchanged.
    if let Some(peer) = a2a_delegate_target() {
        return delegate_step_via_a2a(&peer, idx, n, step, overall, prior, sink);
    }
    // Route the sub-task, then make the choice runnable on this machine: if the preferred
    // provider isn't connected, available_route remaps to the best available one.
    let r = crate::route::available_route(
        classified_route(
            &format!("{} {}", step.title, step.detail),
            make_backend,
            work_root,
            claude_ok,
            agy_ok,
        ),
        claude_ok,
        agy_ok,
    );
    sink(AgentEvent::ModelRouted {
        model: r.label.clone(),
        reason: format!("step {}/{}: {} — {}", idx + 1, n, step.title, r.reason),
    });
    crate::diag::log(
        "conductor",
        &format!("step {}/{} → {} [{}]", idx + 1, n, r.label, r.backend),
    );
    let be = make_backend(&r.backend, Some(r.model.clone()));
    let step_prompt = format!(
        "You are step {}/{} of a larger task — do ONLY this step, then stop.\nOverall task: {overall}{prior}\nThis step: {} — {}",
        idx + 1,
        n,
        step.title,
        step.detail
    );
    let ctx = TurnContext {
        workspace_root: work_root,
        recalls: recalls.to_vec(),
        skills: skills.to_vec(),
        system: system.clone(),
        apply,
    };
    let mut summary = String::new();
    let outcome = {
        let mut fwd = |ev: AgentEvent| {
            match &ev {
                AgentEvent::MessageDelta { text } => summary.push_str(text),
                AgentEvent::Message { text } => {
                    summary.push_str(text);
                    summary.push('\n');
                }
                _ => {}
            }
            sink(ev);
        };
        be.run_turn(&step_prompt, &ctx, &mut fwd)?
    };
    Ok(StepRun {
        idx,
        backend_id: r.backend,
        summary,
        outcome,
        events: Vec::new(),
    })
}

/// Relative paths with uncommitted changes in a git working tree (no index mutation).
fn git_porcelain(dir: &Path) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .current_dir(dir)
        .args(["status", "--porcelain"])
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.get(3..).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Copy the main tree's current uncommitted changes into a fresh worktree, so a parallel
/// step sees earlier (sequential) steps' work even though the worktree forked HEAD.
fn overlay_worktree(main: &Path, wt: &Path) {
    for rel in git_porcelain(main) {
        let src = main.join(&rel);
        if src.is_file() {
            let dst = wt.join(&rel);
            if let Some(p) = dst.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

/// Merge a finished worktree back into the main tree by copying its changed files.
/// Returns the relative paths changed (for overlap detection across a parallel wave).
fn merge_worktree(wt: &Path, main: &Path) -> Vec<String> {
    let _ = Command::new("git")
        .current_dir(wt)
        .args(["add", "-A"])
        .output();
    let Ok(out) = Command::new("git")
        .current_dir(wt)
        .args(["diff", "--cached", "--name-only"])
        .output()
    else {
        return vec![];
    };
    let paths: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| {
            !s.is_empty()
                && !s.starts_with(".claude")
                && !s.starts_with(".lectern")
                && s.as_str() != ".gitignore"
        })
        .collect();
    for rel in &paths {
        let src = wt.join(rel);
        let dst = main.join(rel);
        if src.is_file() {
            if let Some(p) = dst.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&src, &dst);
        } else if dst.exists() {
            let _ = std::fs::remove_file(&dst);
        }
    }
    paths
}

/// Tear down a worktree + its branch after merging.
fn remove_worktree(main: &Path, wt: &Worktree) {
    let _ = Command::new("git")
        .current_dir(main)
        .args(["worktree", "remove", "--force"])
        .arg(&wt.path)
        .output();
    let _ = Command::new("git")
        .current_dir(main)
        .args(["branch", "-D", &wt.branch])
        .output();
}

pub(crate) const IGNORE: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    ".data",
    "dist",
    "build",
    // graphify's own output — the code graph plus its AST cache blobs. Indexing
    // these pollutes recall with build artifacts instead of source.
    "graphify-out",
];
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_FILES: usize = 5000;
/// Minimum query↔file cosine for a vector hit to count as "relevant" recall.
/// Below this, a match is indistinguishable from trigram-overlap noise. See the
/// calibration note in `recall()`.
const RECALL_RELEVANCE_FLOOR: f32 = 0.12;
/// When recall injects file *content* (not just a path), this caps each file to its
/// most-relevant window so a large file can't blow the token budget on its own.
const RECALL_SNIPPET_LINES: usize = 24;

/// Walk `dir`, collecting (relative-path, utf8-content) for text files (skipping
/// ignored dirs, hidden dirs, oversized/binary files). Bounded by MAX_FILES.
fn collect_text_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>, depth: usize) {
    if depth > 12 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let raw = entry.file_name();
        let name = raw.to_string_lossy();
        if IGNORE.contains(&name.as_ref()) || (name.starts_with('.') && path.is_dir()) {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => collect_text_files(root, &path, out, depth + 1),
            Ok(ft) if ft.is_file() => {
                let too_big = entry
                    .metadata()
                    .map(|m| m.len() > MAX_FILE_BYTES)
                    .unwrap_or(true);
                if too_big {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();
                    out.push((rel, content));
                }
            }
            _ => {}
        }
    }
}

/// Alphanumeric tokens (len>=3), lowercased + deduped, order-preserving (cap 16).
fn tokenize(s: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in s.split(|c: char| !c.is_alphanumeric()) {
        let t = raw.to_lowercase();
        if t.len() >= 3 && seen.insert(t.clone()) {
            out.push(t);
        }
        if out.len() >= 16 {
            break;
        }
    }
    out
}

/// Build a safe FTS5 MATCH query: top tokens quoted + OR-joined. Empty if none.
fn fts_query(prompt: &str) -> String {
    tokenize(prompt)
        .into_iter()
        .take(8)
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Reciprocal-rank fusion: combine ranked lists into one, scoring each item by
/// sum(1/(60+rank)) across the lists it appears in. Returns top `limit` paths.
fn rrf(lists: &[Vec<String>], limit: usize) -> Vec<String> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (rank, item) in list.iter().enumerate() {
            *scores.entry(item.clone()).or_insert(0.0) += 1.0 / (60.0 + rank as f32);
        }
    }
    let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.into_iter().take(limit).map(|(p, _)| p).collect()
}

/// Derive a kebab skill name from a title (first few tokens).
fn derive_name(title: &str) -> String {
    let toks = tokenize(title);
    if toks.is_empty() {
        "recorded-skill".into()
    } else {
        toks.into_iter().take(4).collect::<Vec<_>>().join("-")
    }
}

/// Remove duplicates while preserving first-seen order.
/// Exploratory/meta commands describe how the agent looked around, not what the
/// skill does — recording them as steps makes replays noisy and brittle. Only
/// state-changing commands belong in a recorded procedure.
fn is_meta_command(cmd: &str) -> bool {
    let t = cmd.trim();
    let first = t.split_whitespace().next().unwrap_or("");
    let head2 = t.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    matches!(
        first,
        "ls" | "pwd"
            | "cat"
            | "head"
            | "tail"
            | "echo"
            | "which"
            | "whoami"
            | "cd"
            | "find"
            | "grep"
            | "rg"
            | "wc"
            | "stat"
            | "file"
            | "du"
            | "df"
            | "env"
            | "printenv"
    ) || matches!(
        head2.as_str(),
        "git status" | "git log" | "git diff" | "git branch" | "git show" | "git remote"
    )
}

fn dedup_preserve(v: &mut Vec<String>) {
    let mut seen = HashSet::new();
    v.retain(|x| seen.insert(x.clone()));
}

/// Ensure `entry` is present in the repo's .gitignore (so Lectern's worktrees/data
/// aren't accidentally committed). Best-effort.
fn ensure_gitignore(repo_root: &Path, entry: &str) {
    let gi = repo_root.join(".gitignore");
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    if existing
        .lines()
        .any(|l| l.trim() == entry.trim_end_matches('/') || l.trim() == entry)
    {
        return;
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(entry);
    content.push('\n');
    let _ = std::fs::write(&gi, content);
}

#[cfg(test)]
mod tests {
    #[test]
    fn meta_commands_stay_out_of_recordings() {
        use super::is_meta_command;
        // exploration is meta
        assert!(is_meta_command("ls -la"));
        assert!(is_meta_command("cat src/main.rs"));
        assert!(is_meta_command("git status"));
        assert!(is_meta_command("  git diff --stat"));
        assert!(is_meta_command("grep -rn foo src/"));
        // state changes are the skill
        assert!(!is_meta_command("cargo test"));
        assert!(!is_meta_command("git commit -m x"));
        assert!(!is_meta_command("npm run build"));
        assert!(!is_meta_command("python3 tests.py"));
    }

    #[test]
    fn cancel_matches_short_id_and_reports_miss() {
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[("readme.md", "hi")]);
        let ws = eng.open_workspace(&dir).unwrap();
        let id = eng
            .schedule_add(
                &ws,
                "do a thing",
                "mock",
                false,
                now_ts() + 3600,
                "scheduled",
            )
            .unwrap();
        // `schedule list` prints only the first 8 chars — cancel must accept that.
        let short = &id[..8];
        assert!(
            eng.cancel_schedule_prefix(short).unwrap(),
            "short id should cancel"
        );
        // ScheduleRow tuple: (id, prompt, backend, apply, run_at, reason, status)
        let row = eng.list_schedules(&ws).unwrap().into_iter().next().unwrap();
        assert_eq!(row.6, "cancelled");
        // a non-matching id must report a miss, not a false success.
        assert!(!eng.cancel_schedule_prefix("zzzzzzzz").unwrap());
    }

    #[test]
    fn usage_stats_aggregates_days_and_backends() {
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[("readme.md", "hi")]);
        let ws = eng.open_workspace(&dir).unwrap();
        eng.store
            .create_session("u1", &ws.id, "one", "mock", 100)
            .unwrap();
        eng.store
            .append_event(
                "u1",
                0,
                &AgentEvent::Usage {
                    input_tokens: 1000,
                    output_tokens: 200,
                },
                86_400,
            )
            .unwrap();
        eng.store
            .append_event(
                "u1",
                1,
                &AgentEvent::Usage {
                    input_tokens: 500,
                    output_tokens: 100,
                },
                86_400 * 2,
            )
            .unwrap();
        let v = eng.usage_stats().unwrap();
        let days = v["days"].as_array().unwrap();
        assert_eq!(days.len(), 2, "two distinct days");
        let backends = v["backends"].as_array().unwrap();
        assert_eq!(backends[0]["backend"], "mock");
        assert_eq!(backends[0]["input"], 1500);
        assert_eq!(backends[0]["output"], 300);
    }

    #[test]
    fn session_meta_roundtrip_and_ordering() {
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[("readme.md", "hi")]);
        let ws = eng.open_workspace(&dir).unwrap();
        eng.store
            .create_session("s1", &ws.id, "first", "mock", 100)
            .unwrap();
        eng.store
            .create_session("s2", &ws.id, "second", "mock", 200)
            .unwrap();
        assert!(eng.set_session_meta("s1", "not json").is_err());
        eng.set_session_meta("s1", r#"{"model":"opus","view":"clean"}"#)
            .unwrap();
        let rows = eng.sessions_with_meta(&ws, 10).unwrap();
        assert_eq!(rows[0]["id"], "s1"); // meta write touched updated_at
        assert_eq!(rows[0]["meta"]["model"], "opus");
        assert!(rows[1]["meta"].is_null());
        eng.set_session_pinned("s2", true).unwrap();
        let rows = eng.sessions_with_meta(&ws, 10).unwrap();
        assert_eq!(rows[0]["id"], "s2"); // pin beats recency
        assert_eq!(rows[0]["pinned"], true);
    }

    #[test]
    fn index_throttle_marks_and_expires() {
        let id = format!("throttle-test-{}", std::process::id());
        assert!(super::index_is_stale(&id), "unknown workspace is stale");
        super::mark_indexed(&id);
        assert!(!super::index_is_stale(&id), "freshly marked is not stale");
    }

    use super::*;
    use crate::embed::{cosine, Embedder, HashEmbedder};
    use crate::store::Store;

    fn tmp_workspace(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lectern-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            let p = dir.join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, content).unwrap();
        }
        dir
    }

    #[test]
    fn snippet_extracts_relevant_window_not_whole_file() {
        // A long file where the relevant function is buried in the middle.
        let mut lines: Vec<String> = (0..200).map(|i| format!("// filler line {i}")).collect();
        lines[120] = "fn schedule_retry(after_secs: i64) {".into();
        lines[121] = "    // recompute the next run window".into();
        lines[122] = "    enqueue(after_secs);".into();
        let content = lines.join("\n");
        let (snip, truncated) = relevant_snippet(&content, "schedule retry window", 24);
        assert!(truncated, "a 200-line file must be snipped");
        assert!(
            snip.contains("schedule_retry"),
            "snippet should hold the relevant window"
        );
        assert!(
            est_tokens(&snip) < est_tokens(&content) / 4,
            "snippet must be far cheaper than the whole file"
        );
        // Nothing matches → falls back to the head, still bounded.
        let (head, t2) = relevant_snippet(&content, "zzzznomatch", 24);
        assert!(t2 && head.lines().count() <= 25);
    }

    #[test]
    fn contextual_recall_matches_file_purpose_from_path() {
        // scheduler.py's BODY never contains "schedul" — only its path does. Contextual
        // retrieval folds the path into the embedding, so a query about the file's
        // purpose still surfaces it; an unrelated file stays out.
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[
            ("src/scheduler.py", "def run(items):\n    out = []\n    for x in items:\n        out.append(process(x))\n    return out"),
            ("notes/groceries.txt", "milk\neggs\nbread\ncoffee\napples"),
        ]);
        let ws = eng.open_workspace(&dir).unwrap();
        eng.index_workspace(&ws).unwrap();
        let hits = eng.recall(&ws, "how does the scheduler work", 4);
        assert!(
            hits.iter().any(|h| h.contains("scheduler.py")),
            "path-context should surface scheduler.py even though its body never says 'scheduler', got {hits:?}"
        );
        assert!(
            !hits.iter().any(|h| h.contains("groceries")),
            "unrelated file must not be recalled, got {hits:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_skill_md_from_open_standard() {
        let md = "---\nname: pdf-tools\ndescription: Work with PDF files\ntriggers: pdf, extract\n---\n\n# PDF Tools\n\nUse pdfplumber to read text.\n\nAlways validate the file exists first.\n";
        let b = parse_skill_md(md).unwrap();
        assert_eq!(b.name, "pdf-tools");
        assert_eq!(b.description, "Work with PDF files");
        assert_eq!(b.triggers, vec!["pdf".to_string(), "extract".to_string()]);
        assert_eq!(
            b.rules.len(),
            2,
            "two body paragraphs → two rules: {:?}",
            b.rules
        );
        assert!(b.rules[0].contains("pdfplumber"));

        // Lectern's own re-exported name prefix is stripped (no lectern-lectern-).
        let round = parse_skill_md(
            "---\nname: lectern-deploy\ndescription: d\n---\n\n# Deploy\n\nship it\n",
        )
        .unwrap();
        assert_eq!(round.name, "deploy");

        // No frontmatter name is a hard error (can't import an unnamed skill).
        assert!(parse_skill_md("just some text, no frontmatter").is_err());
    }

    #[test]
    fn stuck_detector_warns_once_on_repeated_failure() {
        let mut d = StuckDetector::default();
        // A failing command warns only after it crosses the threshold, then stays quiet.
        assert!(d.observe("cargo test", 1).is_none());
        assert!(d.observe("cargo test", 1).is_none());
        let w = d.observe("cargo test", 1);
        assert!(w
            .as_deref()
            .is_some_and(|s| s.contains("stuck") && s.contains("cargo test")));
        assert!(
            d.observe("cargo test", 1).is_none(),
            "warns once, then silent"
        );
        // A command that keeps succeeding never warns, however often it runs.
        for _ in 0..6 {
            assert!(d.observe("ls", 0).is_none());
        }
        // A different failing command is tracked independently.
        d.observe("make", 2);
        d.observe("make", 2);
        assert!(d.observe("make", 2).is_some());
    }

    #[test]
    fn recall_floor_drops_noise_keeps_relevant() {
        // Reproduces the reported bug: a workspace that also holds unrelated files
        // (e.g. FL Studio projects) must NOT surface them for a greeting.
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[
            ("src/queue.py", "class TaskQueue:\n    def push(self, task): self._items.append(task)\n    def pop(self): return self._items.pop(0)"),
            ("README.md", "# taskflow\nA small task queue with a pluggable scheduler."),
            ("FLSTUDIO/Laurie Webb Vox.ini", "[General]\nTempo=140\nName=Laurie Webb Vox\nPlugin=Fruity NoteBook"),
            ("FLSTUDIO/.update-timestamp", "2026-07-08"),
        ]);
        let ws = eng.open_workspace(&dir).unwrap();
        eng.index_workspace(&ws).unwrap();

        // A greeting has no genuine match → recall must be empty (was: 4 FL files).
        assert!(
            eng.recall(&ws, "hey hows it going", 4).is_empty(),
            "greeting should recall nothing"
        );
        // A real task still recalls the relevant project files, and never the FL noise.
        let hits = eng.recall(&ws, "fix the task queue scheduler", 4);
        assert!(!hits.is_empty(), "a real task should recall something");
        assert!(
            !hits.iter().any(|h| h.contains("FLSTUDIO")),
            "recall must not surface unrelated FL Studio files, got {hits:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recall_surfaces_a_file_named_in_the_query() {
        // The file's CONTENT is unrelated to "authentication" — only its path names it.
        // The path signal (fused via RRF) should still surface it when the query does.
        let eng = Engine::with_store(crate::store::Store::open_in_memory().unwrap());
        let dir = tmp_workspace(&[
            (
                "src/authentication.py",
                "def helper(x):\n    return x + 1\n",
            ),
            ("notes/todo.txt", "buy milk\nwalk the dog\ncall mom"),
        ]);
        let ws = eng.open_workspace(&dir).unwrap();
        eng.index_workspace(&ws).unwrap();
        let hits = eng.recall(&ws, "fix the authentication timeout", 4);
        assert!(
            hits.iter().any(|h| h.contains("authentication")),
            "a file named in the query should surface, got {hits:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indexer_skips_graphify_output() {
        let dir = tmp_workspace(&[
            ("src/main.rs", "fn main() {}"),
            ("README.md", "hello"),
            ("graphify-out/graph.json", "{\"nodes\":[]}"),
            ("graphify-out/cache/ast/blob", "cache noise"),
        ]);
        let mut out = Vec::new();
        collect_text_files(&dir, &dir, &mut out, 0);
        let paths: Vec<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("main.rs")));
        assert!(paths.iter().any(|p| p.contains("README")));
        assert!(
            !paths.iter().any(|p| p.contains("graphify-out")),
            "graphify-out must not be indexed, got {paths:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_title_strips_preambles() {
        assert_eq!(
            session_title("You are the user's PERSONAL DESKTOP AGENT…\n\nTask: open Firefox"),
            "open Firefox"
        );
        assert_eq!(
            session_title("ONE-SHOT…\n\nBrief: build a calculator"),
            "build a calculator"
        );
        assert_eq!(
            session_title("just a normal prompt"),
            "just a normal prompt"
        );
    }

    #[test]
    fn recorded_skill_replays_at_recorded_pace() {
        // A recorded GUI workflow with real captured delays must render a Run block
        // that waits those exact delays (not a flat 0.3s) before each action.
        let sk = Skill {
            id: "x".into(),
            scope: "global".into(),
            name: "Timed test".into(),
            description: "d".into(),
            triggers: vec![],
            body: SkillBody {
                rules: vec![],
                steps: vec![
                    "[+0.2s] Click at (700, 450) in \"App\"".into(),
                    "[+2.5s] Type \"hello\" in \"App\"".into(),
                    "[+1.5s] Click at (850, 520) in \"App\"".into(),
                ],
            },
            uses: 0,
        };
        let md = render_skill_md(&sk);
        assert!(
            md.contains("## Run"),
            "should be a runnable GUI skill: {md}"
        );
        assert!(md.contains("sleep 2.5"), "must keep the 2.5s pause: {md}");
        assert!(md.contains("sleep 1.5"), "must keep the 1.5s pause: {md}");
        assert!(md.contains("xdotool type --clearmodifiers -- \"hello\""));
        // parse_step strips the prefix for command translation.
        assert_eq!(parse_step("[+2.5s] Type \"hi\" in \"W\"").0, 2.5);
    }

    #[test]
    fn gui_replay_steps_translates_or_declines() {
        // A GUI recording → (delay, command) replay list.
        let steps = vec![
            "[+0.2s] Click at (700, 450) in \"App\"".to_string(),
            "[+2.5s] Type \"hello\" in \"App\"".to_string(),
        ];
        let r = gui_replay_steps(&steps).expect("should be a GUI macro");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, 0.2);
        assert!(r[0].1.contains("xdotool mousemove 700 450 click 1"));
        assert!(r[1]
            .1
            .contains("xdotool type --clearmodifiers -- \"hello\""));
        // An instruction-only skill is NOT a GUI macro → None (caller uses the agent path).
        assert!(
            gui_replay_steps(&["Run the test suite".to_string(), "Fix failures".to_string()])
                .is_none()
        );
    }

    #[test]
    fn tokenize_filters_and_dedups() {
        let t = tokenize("Add a Settings page, add SETTINGS!");
        assert!(t.contains(&"settings".to_string()));
        assert!(t.contains(&"add".to_string()));
        assert!(!t.contains(&"a".to_string())); // len < 3 dropped
        assert_eq!(t.iter().filter(|x| *x == "settings").count(), 1); // deduped
    }

    #[test]
    fn fts_query_is_quoted_or_join() {
        assert_eq!(fts_query("login reset"), "\"login\" OR \"reset\"");
        assert_eq!(fts_query("!! a !!"), ""); // no usable tokens
    }

    #[test]
    fn rrf_fuses_and_ranks() {
        let a = vec!["x".to_string(), "y".to_string()];
        let b = vec!["y".to_string(), "z".to_string()];
        let fused = rrf(&[a, b], 3);
        assert_eq!(fused[0], "y"); // appears in both → highest
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn cosine_self_is_one() {
        let e = HashEmbedder::new();
        let v = e.embed("authentication and password reset");
        assert_eq!(v.len(), 256);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn hybrid_recall_finds_relevant_file() {
        let dir = tmp_workspace(&[
            ("auth.rs", "fn login() { /* session password reset */ }"),
            ("ui.rs", "fn render_button() {}"),
        ]);
        let engine = Engine::with_store(Store::open_in_memory().unwrap());
        let ws = engine.open_workspace(&dir).unwrap();
        engine.index_workspace(&ws).unwrap();
        let hits = engine.recall(&ws, "fix the login session", 3);
        assert!(hits.iter().any(|p| p == "auth.rs"), "got {hits:?}");
    }

    #[test]
    fn limit_hit_and_scheduling() {
        let dir = tmp_workspace(&[("package.json", "{}")]);
        let engine = Engine::with_store(Store::open_in_memory().unwrap());
        let ws = engine.open_workspace(&dir).unwrap();

        // The limit backend should set limit_hit (drives auto-continue).
        let res = engine
            .run(
                &ws,
                "x",
                &crate::LimitBackend,
                RunOptions::default(),
                |_| {},
            )
            .unwrap();
        assert!(res.limit_hit);

        // A schedule due in the past runs once and is marked done.
        engine
            .schedule_add(&ws, "do it", "mock", false, now_ts() - 1, "test")
            .unwrap();
        let ran = engine.run_due_schedules(60, |_| {}).unwrap();
        assert_eq!(ran.len(), 1);
        // A schedule far in the future is not due yet.
        engine
            .schedule_add(&ws, "later", "mock", false, now_ts() + 9999, "test")
            .unwrap();
        let ran2 = engine.run_due_schedules(60, |_| {}).unwrap();
        assert_eq!(ran2.len(), 0);

        // The claim is atomic: a second claim of the same schedule loses —
        // this is what stops lecternd + `lectern serve` double-running a task.
        let id = engine
            .schedule_add(&ws, "claim me", "mock", false, now_ts() - 1, "test")
            .unwrap();
        assert!(engine.store.claim_schedule(&id).unwrap());
        assert!(!engine.store.claim_schedule(&id).unwrap());
    }

    #[test]
    fn applied_run_creates_and_records_a_checkpoint() {
        // Redirect the shadow-git store to a temp dir so the test never touches ~/.lectern.
        // (No other test does an applied run, so nothing else reads this env var.)
        let ckpt_dir = std::env::temp_dir().join(format!(
            "lectern-ckpt-run-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::env::set_var("LECTERN_CHECKPOINT_DIR", &ckpt_dir);

        let dir = tmp_workspace(&[("readme.md", "hello")]);
        let engine = Engine::with_store(Store::open_in_memory().unwrap());
        let ws = engine.open_workspace(&dir).unwrap();
        let backend = MockBackend { fast: true };

        let mut saw_checkpoint = false;
        engine
            .run(
                &ws,
                "add a greeting to the readme",
                &backend,
                RunOptions {
                    apply: true,
                    worktree: false,
                },
                |ev| {
                    if matches!(ev, AgentEvent::Checkpoint { .. }) {
                        saw_checkpoint = true;
                    }
                },
            )
            .unwrap();
        assert!(saw_checkpoint, "an applied run emits a Checkpoint event");
        let cps = engine.store.list_checkpoints(&ws.id, 10).unwrap();
        assert_eq!(cps.len(), 1, "the checkpoint was recorded");
        assert!(!cps[0].0.is_empty(), "checkpoint has a git sha");

        // A plan-only run (apply=false) must NOT checkpoint — nothing was written.
        engine
            .run(
                &ws,
                "just look around",
                &backend,
                RunOptions::default(),
                |_| {},
            )
            .unwrap();
        assert_eq!(
            engine.store.list_checkpoints(&ws.id, 10).unwrap().len(),
            1,
            "a plan-only run does not checkpoint"
        );

        // Leave LECTERN_CHECKPOINT_DIR set (to a temp dir) — never remove it, so a
        // concurrent test can never fall back to the real ~/.lectern between tests.
        let _ = std::fs::remove_dir_all(&ckpt_dir);
    }

    #[test]
    fn home_workspace_is_not_checkpointed() {
        // Keep any checkpoint writes out of the real ~/.lectern even if the guard breaks.
        let ckpt_dir = std::env::temp_dir().join(format!(
            "lectern-ckpt-home-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::env::set_var("LECTERN_CHECKPOINT_DIR", &ckpt_dir);

        let dir = tmp_workspace(&[("a.txt", "hi")]);
        let engine = Engine::with_store(Store::open_in_memory().unwrap());
        let ws = engine.open_workspace(&dir).unwrap();

        // is_home = true → the pre-run snapshot is skipped entirely: no event, no record.
        let mut events = 0;
        engine.checkpoint_before_run(&ws.root, &ws.id, "sess", "prompt", true, &mut |_| {
            events += 1;
        });
        assert_eq!(events, 0, "no Checkpoint event for the home workspace");
        assert!(engine
            .store
            .list_checkpoints(&ws.id, 10)
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(&ckpt_dir);
    }

    #[test]
    fn record_and_match_skill() {
        let dir = tmp_workspace(&[("package.json", "{}")]);
        let engine = Engine::with_store(Store::open_in_memory().unwrap());
        let ws = engine.open_workspace(&dir).unwrap();
        // run a mock session so there's something to record
        let backend = MockBackend { fast: true };
        let mut events = 0;
        engine
            .run(
                &ws,
                "add a settings page",
                &backend,
                RunOptions::default(),
                |_| events += 1,
            )
            .unwrap();
        assert!(events > 0);
        let skill = engine
            .record_skill(&ws, None, Some("add-settings"))
            .unwrap();
        assert!(!skill.body.steps.is_empty());
        let matched = engine.match_skills(&ws, "please add a settings page now", 3);
        assert!(matched.iter().any(|s| s.name == "add-settings"));
    }

    #[test]
    fn conductor_delegates_a_step_to_an_a2a_peer() {
        use crate::a2a::{Message, Task, TaskState, TaskStatus};
        use crate::event::AgentEvent;
        use crate::orchestrator::ConductorStep;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        // A minimal echo A2A peer: message/send returns a COMPLETED task whose reply
        // echoes the prompt (so no polling is needed for this test).
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let handle = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                let mut req = match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(r)) => r,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let v: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
                let prompt = v["params"]["message"]["parts"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let msg = Message::agent_text(format!("PEER DONE: {prompt}"), "t1", "c1");
                let task = Task {
                    id: "t1".into(),
                    context_id: "c1".into(),
                    status: TaskStatus {
                        state: TaskState::Completed,
                        message: Some(msg.clone()),
                        timestamp: None,
                    },
                    history: vec![msg],
                    artifacts: vec![],
                };
                let out =
                    crate::a2a::rpc_result(&id, serde_json::to_value(task).unwrap()).to_string();
                let hdr =
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap();
                let _ = req.respond(tiny_http::Response::from_string(out).with_header(hdr));
            }
        });

        let peer = crate::a2a::A2aPeer {
            name: "echo".into(),
            url: base,
            token: None,
        };
        let step = ConductorStep {
            title: "Add a settings page".into(),
            detail: "create settings.tsx".into(),
            kind: "code".into(),
            parallel: false,
        };
        let mut events = Vec::new();
        let run = super::delegate_step_via_a2a(&peer, 0, 2, &step, "overall goal", "", &mut |ev| {
            events.push(ev)
        })
        .unwrap();

        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();

        assert_eq!(run.backend_id, "a2a:echo");
        // the peer's reply (which echoes our step prompt) is folded into the summary
        assert!(run.summary.contains("PEER DONE"));
        assert!(run.summary.contains("Add a settings page"));
        // a delegated step makes no local file changes
        assert!(run.outcome.changes.is_empty());
        // both the routing note and the reply were streamed
        assert!(events
            .iter()
            .any(|e| matches!(e, AgentEvent::ModelRouted { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, AgentEvent::Message { .. })));
    }
}
