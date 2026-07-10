//! Local store (SQLite via rusqlite). Persists workspaces, sessions, the event log,
//! and proposed changes — the seed of cross-session memory.
//! See Lectern-Brain/03-Architecture/Data Model & Storage.md.
use crate::event::AgentEvent;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

/// Epoch seconds → "YYYY-MM-DD" (UTC, no chrono dependency; civil-from-days algorithm).
fn chrono_day(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub struct Store {
    pub conn: Connection,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY, root TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
  created_at INTEGER NOT NULL, last_opened INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, title TEXT NOT NULL,
  backend TEXT NOT NULL, created_at INTEGER NOT NULL, status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
  idx INTEGER NOT NULL, payload TEXT NOT NULL, ts INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS changes (
  id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
  path TEXT NOT NULL, added INTEGER NOT NULL, removed INTEGER NOT NULL, status TEXT NOT NULL
);
-- Memory v1: lexical recall over repo files (FTS5). Vector recall (sqlite-vec) is next.
CREATE VIRTUAL TABLE IF NOT EXISTS file_index USING fts5(path, content, workspace_id UNINDEXED);
-- Vector recall: per-file embeddings (brute-force cosine; sqlite-vec ANN is the scale step).
CREATE TABLE IF NOT EXISTS vectors (workspace_id TEXT NOT NULL, path TEXT NOT NULL, dim INTEGER NOT NULL, vec BLOB NOT NULL);
CREATE INDEX IF NOT EXISTS vectors_ws ON vectors(workspace_id);
-- Skills v1: recorded/learned procedures (subject-keyed: workspace_id NULL = global).
CREATE TABLE IF NOT EXISTS skills (
  id TEXT PRIMARY KEY, workspace_id TEXT, scope TEXT NOT NULL, name TEXT NOT NULL,
  description TEXT NOT NULL, triggers TEXT NOT NULL, body TEXT NOT NULL,
  uses INTEGER NOT NULL DEFAULT 0, successes INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
-- Scheduled / auto-continue tasks (one-shot run_at; the daemon runs due ones).
CREATE TABLE IF NOT EXISTS schedules (
  id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, prompt TEXT NOT NULL,
  backend TEXT NOT NULL, apply INTEGER NOT NULL DEFAULT 0, run_at INTEGER NOT NULL,
  reason TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'pending',
  created_at INTEGER NOT NULL, last_run INTEGER
);
-- Checkpoints: the session↔snapshot index for rewind. The snapshot content lives in the
-- shadow-git store (crate::checkpoint); git_sha links a row to its commit there.
CREATE TABLE IF NOT EXISTS checkpoints (
  id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
  git_sha TEXT NOT NULL, label TEXT NOT NULL, created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS checkpoints_ws ON checkpoints(workspace_id);
"#;

pub type ScheduleRow = (String, String, String, i64, i64, String, String);
// id, prompt, backend, apply, run_at, reason, status
pub type DueSchedule = (String, String, String, String, String, bool);
// id, workspace_id, workspace_root, prompt, backend, apply

pub type SkillRow = (String, String, String, String, String, String, i64, i64);
// id, scope, name, description, triggers(json), body(json), uses, successes

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub type SessionRow = (String, String, String, i64, String); // id, title, backend, created_at, status

pub type CheckpointRow = (String, String, String, i64); // git_sha, label, session_id, created_at

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn);
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn);
        Ok(Self { conn })
    }

    /// Idempotent column additions (duplicate-column errors ignored). One home
    /// so file-backed and in-memory stores can never drift.
    fn migrate(conn: &Connection) {
        for sql in [
            "ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE sessions ADD COLUMN meta TEXT",
            "ALTER TABLE sessions ADD COLUMN updated_at INTEGER",
        ] {
            let _ = conn.execute(sql, []);
        }
    }

    pub fn upsert_workspace(&self, id: &str, root: &str, name: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workspaces (id, root, name, created_at, last_opened) VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(root) DO UPDATE SET last_opened = ?4",
            params![id, root, name, ts],
        )?;
        Ok(())
    }

    pub fn workspace_id_for_root(&self, root: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM workspaces WHERE root = ?1")?;
        let mut rows = stmt.query(params![root])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn create_session(
        &self,
        id: &str,
        ws: &str,
        title: &str,
        backend: &str,
        ts: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, workspace_id, title, backend, created_at, status) VALUES (?1,?2,?3,?4,?5,'running')",
            params![id, ws, title, backend, ts],
        )?;
        Ok(())
    }

    pub fn finish_session(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    /// At-a-glance usage: token totals per day (last 14), per backend,
    /// and the most recent sessions — extracted from persisted `usage` events
    /// joined to their session's backend.
    pub fn usage_stats(&self) -> Result<serde_json::Value> {
        let mut days: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
        let mut backends: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
        let mut sessions: Vec<serde_json::Value> = vec![];
        let mut stmt = self.conn.prepare(
            "SELECT e.payload, e.ts, s.backend, s.title, s.id FROM events e              JOIN sessions s ON s.id = e.session_id              WHERE e.payload LIKE '%\"type\":\"usage\"%' ORDER BY e.ts DESC LIMIT 2000",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut seen_sessions = std::collections::HashSet::new();
        for row in rows.flatten() {
            let (payload, ts, backend, title, sid) = row;
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) else {
                continue;
            };
            if v.get("type").and_then(|t| t.as_str()) != Some("usage") {
                continue;
            }
            let inp = v.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            let out = v.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            let day = chrono_day(ts);
            let d = days.entry(day).or_default();
            d.0 += inp;
            d.1 += out;
            let b = backends.entry(backend.clone()).or_default();
            b.0 += inp;
            b.1 += out;
            if sessions.len() < 10 && seen_sessions.insert(sid.clone()) {
                sessions.push(serde_json::json!({
                    "title": title, "backend": backend, "input": inp, "output": out, "ts": ts
                }));
            }
        }
        let days_v: Vec<serde_json::Value> = days
            .iter()
            .rev()
            .take(126) // 18 weeks — feeds both the bars and the activity-grid view
            .map(|(d, (i, o))| serde_json::json!({ "day": d, "input": i, "output": o }))
            .collect();
        let backends_v: Vec<serde_json::Value> = backends
            .iter()
            .map(|(b, (i, o))| serde_json::json!({ "backend": b, "input": i, "output": o }))
            .collect();
        let (ti, to): (i64, i64) = backends
            .values()
            .fold((0, 0), |a, x| (a.0 + x.0, a.1 + x.1));
        Ok(serde_json::json!({
            "days": days_v, "backends": backends_v, "recent": sessions,
            "total_input": ti, "total_output": to,
        }))
    }

    pub fn append_event(&self, session_id: &str, idx: i64, ev: &AgentEvent, ts: i64) -> Result<()> {
        let payload = serde_json::to_string(ev)?;
        self.conn.execute(
            "INSERT INTO events (session_id, idx, payload, ts) VALUES (?1,?2,?3,?4)",
            params![session_id, idx, payload, ts],
        )?;
        Ok(())
    }

    pub fn record_change(
        &self,
        session_id: &str,
        path: &str,
        added: i64,
        removed: i64,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO changes (session_id, path, added, removed, status) VALUES (?1,?2,?3,?4,?5)",
            params![session_id, path, added, removed, status],
        )?;
        Ok(())
    }

    /// Record that a workspace snapshot (`git_sha`, in the shadow-git store) was taken
    /// before `session_id`'s turn. The label is the prompt.
    pub fn record_checkpoint(
        &self,
        session_id: &str,
        workspace_id: &str,
        git_sha: &str,
        label: &str,
        ts: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO checkpoints (session_id, workspace_id, git_sha, label, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![session_id, workspace_id, git_sha, label, ts],
        )?;
        Ok(())
    }

    /// Checkpoints for a workspace, newest first: (git_sha, label, session_id, created_at).
    pub fn list_checkpoints(&self, workspace_id: &str, limit: i64) -> Result<Vec<CheckpointRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT git_sha, label, session_id, created_at FROM checkpoints
             WHERE workspace_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![workspace_id, limit], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now_secs(), session_id],
        )?;
        Ok(())
    }

    pub fn set_session_pinned(&self, session_id: &str, pinned: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET pinned = ?1, updated_at = ?2 WHERE id = ?3",
            params![pinned as i64, now_secs(), session_id],
        )?;
        Ok(())
    }

    pub fn session_pinned(&self, session_id: &str) -> Result<bool> {
        let v: i64 = self.conn.query_row(
            "SELECT pinned FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;
        Ok(v != 0)
    }

    /// Desktop-owned metadata blob (model, mode, view, project, personalAgent …).
    /// The engine never interprets it — it round-trips for surface parity.
    pub fn set_session_meta(&self, session_id: &str, meta_json: &str) -> Result<()> {
        // validate it's JSON so a bad write can't poison every reader
        let _: serde_json::Value = serde_json::from_str(meta_json)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "UPDATE sessions SET meta = ?1, updated_at = ?2 WHERE id = ?3",
            params![meta_json, now, session_id],
        )?;
        Ok(())
    }

    /// Full session objects for surface unification (pinned first, most recent
    /// activity next). SessionRow stays for existing callers.
    pub fn sessions_with_meta(&self, ws: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, backend, created_at, status, pinned, meta, COALESCE(updated_at, created_at)
             FROM sessions WHERE workspace_id = ?1
             ORDER BY pinned DESC, COALESCE(updated_at, created_at) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![ws, limit], |r| {
            let meta_raw: Option<String> = r.get(6)?;
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "backend": r.get::<_, String>(2)?,
                "created_at": r.get::<_, i64>(3)?,
                "status": r.get::<_, String>(4)?,
                "pinned": r.get::<_, i64>(5)? != 0,
                "meta": meta_raw.and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok()),
                "updated_at": r.get::<_, i64>(7)?,
            }))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn recent_sessions(&self, ws: &str, limit: i64) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, backend, created_at, status FROM sessions WHERE workspace_id = ?1 ORDER BY pinned DESC, created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![ws, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Memory v1: file index (FTS5 lexical recall) ──────────────────────────
    pub fn clear_file_index(&self, ws: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM file_index WHERE workspace_id = ?1",
            params![ws],
        )?;
        Ok(())
    }

    pub fn index_file(&self, ws: &str, path: &str, content: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO file_index (path, content, workspace_id) VALUES (?1, ?2, ?3)",
            params![path, content, ws],
        )?;
        Ok(())
    }

    /// All indexed file paths for a workspace (for the brain/memory graph).
    pub fn list_indexed_files(&self, ws: &str, limit: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM file_index WHERE workspace_id = ?1 ORDER BY path LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![ws, limit], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Lexical recall: returns the top matching file paths for an FTS5 query.
    pub fn search_files(&self, ws: &str, fts_query: &str, limit: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM file_index WHERE file_index MATCH ?1 AND workspace_id = ?2 ORDER BY rank LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![fts_query, ws, limit], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Vector index ─────────────────────────────────────────────────────────
    pub fn clear_vectors(&self, ws: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM vectors WHERE workspace_id = ?1", params![ws])?;
        Ok(())
    }

    pub fn index_vector(&self, ws: &str, path: &str, dim: i64, vec: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO vectors (workspace_id, path, dim, vec) VALUES (?1,?2,?3,?4)",
            params![ws, path, dim, vec],
        )?;
        Ok(())
    }

    pub fn all_vectors(&self, ws: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, vec FROM vectors WHERE workspace_id = ?1")?;
        let rows = stmt.query_map(params![ws], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Skills v1 ────────────────────────────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    pub fn create_skill(
        &self,
        id: &str,
        ws: Option<&str>,
        scope: &str,
        name: &str,
        description: &str,
        triggers_json: &str,
        body_json: &str,
        ts: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO skills (id, workspace_id, scope, name, description, triggers, body, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, ws, scope, name, description, triggers_json, body_json, ts],
        )?;
        Ok(())
    }

    /// Skills visible to a workspace: its repo-scoped skills plus global ones.
    pub fn list_skills(&self, ws: &str) -> Result<Vec<SkillRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, name, description, triggers, body, uses, successes FROM skills
             WHERE workspace_id = ?1 OR workspace_id IS NULL ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![ws], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn bump_skill_use(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET uses = uses + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn last_session_id(&self, ws: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM sessions WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![ws])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn session_events(&self, session_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload FROM events WHERE session_id = ?1 ORDER BY idx ASC")?;
        let rows = stmt.query_map(params![session_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn session_title(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT title FROM sessions WHERE id = ?1")?;
        let mut rows = stmt.query(params![session_id])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn session_count(&self, ws: &str) -> Result<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE workspace_id = ?1",
            params![ws],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    // ── Schedules ────────────────────────────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    pub fn create_schedule(
        &self,
        id: &str,
        ws: &str,
        prompt: &str,
        backend: &str,
        apply: bool,
        run_at: i64,
        reason: &str,
        ts: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedules (id, workspace_id, prompt, backend, apply, run_at, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, ws, prompt, backend, apply as i64, run_at, reason, ts],
        )?;
        Ok(())
    }

    pub fn list_schedules(&self, ws: &str) -> Result<Vec<ScheduleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, prompt, backend, apply, run_at, reason, status FROM schedules
             WHERE workspace_id = ?1 ORDER BY run_at ASC",
        )?;
        let rows = stmt.query_map(params![ws], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete a learned skill (all rows with this name).
    pub fn delete_skill_by_name(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM skills WHERE name = ?1", params![name])?;
        Ok(())
    }

    /// Make all learned skills global (shared across workspaces) — the brain is global.
    pub fn globalize_skills(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET workspace_id = NULL, scope = 'global' WHERE workspace_id IS NOT NULL",
            [],
        )?;
        Ok(())
    }

    /// All schedules across every workspace (global Schedule view).
    pub fn list_all_schedules(&self) -> Result<Vec<ScheduleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, prompt, backend, apply, run_at, reason, status FROM schedules ORDER BY run_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Pending schedules whose time has come, joined with the workspace root.
    pub fn due_schedules(&self, now: i64) -> Result<Vec<DueSchedule>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.workspace_id, w.root, s.prompt, s.backend, s.apply
             FROM schedules s JOIN workspaces w ON w.id = s.workspace_id
             WHERE s.status = 'pending' AND s.run_at <= ?1 ORDER BY s.run_at ASC",
        )?;
        let rows = stmt.query_map(params![now], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)? != 0,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete finished schedule rows (done/error/cancelled). Pending and
    /// running rows are never touched. Returns how many were removed.
    pub fn clear_finished_schedules(&self) -> Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM schedules WHERE status IN ('done','error','cancelled')",
            [],
        )?;
        Ok(n)
    }

    pub fn set_schedule_status(&self, id: &str, status: &str, last_run: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE schedules SET status = ?2, last_run = ?3 WHERE id = ?1",
            params![id, status, last_run],
        )?;
        Ok(())
    }

    /// Set status matching a full id OR the short id prefix that `schedule list`
    /// prints. Returns the number of rows changed so callers can report truthfully
    /// instead of claiming success on a no-op.
    pub fn set_schedule_status_by_prefix(&self, id: &str, status: &str) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE schedules SET status = ?2 WHERE id = ?1 OR id LIKE ?1 || '%'",
            params![id, status],
        )?;
        Ok(n)
    }

    /// Atomically claim a due schedule (pending → running). Returns false when
    /// another runner (a second lecternd / `lectern serve`) claimed it first —
    /// the double-run guard. Every run outcome overwrites the status afterwards
    /// (done/error/limit); a crash mid-run leaves it 'running', which is visible
    /// rather than silently re-fired.
    pub fn claim_schedule(&self, id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE schedules SET status = 'running' WHERE id = ?1 AND status = 'pending'",
            params![id],
        )?;
        Ok(n > 0)
    }
}
