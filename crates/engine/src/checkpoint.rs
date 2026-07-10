//! Shadow-git checkpoints — snapshot the workspace before an agent writes to it, so the
//! user can rewind.
//!
//! The store is a private git directory under `~/.lectern/checkpoints/` whose *work tree*
//! is the workspace root. It is entirely separate from the user's own `.git`: a different
//! `GIT_DIR`, its own identity, and the user's global/system git config disabled. That
//! means checkpoints work on folders that aren't git repos at all, and can never disturb
//! the user's real history, index, branches, or hooks.
//!
//! Rewinding is itself reversible: [`restore`] first snapshots the current state (the
//! "redo" checkpoint), then `reset --hard`s the work tree back to the target — which both
//! reverts edits and removes files the agent added afterwards.

use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// One saved snapshot of the workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Short git sha — the id the user references (`lectern rewind <id>`).
    pub id: String,
    /// Human label (usually the prompt that was about to run).
    pub label: String,
    /// Unix seconds (git commit time).
    pub created_at: i64,
}

/// The result of a [`restore`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Restore {
    /// Full sha the work tree was reset to.
    pub target: String,
    /// The redo checkpoint captured just before rewinding — rewind *this* to undo the
    /// rewind. `None` only if the state was already identical to the last checkpoint.
    pub redo: Option<String>,
    /// Paths that changed on disk as a result of the rewind.
    pub changed: Vec<String>,
}

/// Where the shadow stores live. Overridable for tests via `LECTERN_CHECKPOINT_DIR`.
fn base_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LECTERN_CHECKPOINT_DIR") {
        return PathBuf::from(p);
    }
    crate::data_dir().join("checkpoints")
}

/// The private git dir that snapshots `ws_root`. Unique per canonical workspace path.
fn git_dir_for(ws_root: &Path) -> PathBuf {
    let canon = std::fs::canonicalize(ws_root).unwrap_or_else(|_| ws_root.to_path_buf());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    canon.to_string_lossy().hash(&mut h);
    base_dir().join(format!("{:016x}.git", h.finish()))
}

/// Run a git command against the shadow store: our `GIT_DIR`, the workspace as work tree,
/// and the user's global/system git config disabled so nothing leaks in or out.
fn git(git_dir: &Path, ws_root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .env("GIT_DIR", git_dir)
        .env("GIT_WORK_TREE", ws_root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .map_err(|e| anyhow!("git not available: {e}"))
}

/// Baseline ignore patterns so a snapshot never captures build output, VCS dirs, or the
/// brain store. The workspace's own `.gitignore` is honored automatically (its files are
/// in the work tree), so this only adds a floor for non-git folders.
fn write_excludes(git_dir: &Path) -> Result<()> {
    let info = git_dir.join("info");
    std::fs::create_dir_all(&info)?;
    let mut patterns: Vec<String> = crate::IGNORE.iter().map(|s| format!("{s}/")).collect();
    patterns.push(".lectern/".into());
    let body = patterns.join("\n") + "\n";
    std::fs::File::create(info.join("exclude"))?.write_all(body.as_bytes())?;
    Ok(())
}

/// Create the shadow store for `ws_root` if it doesn't exist yet. Idempotent.
fn ensure_in(git_dir: &Path, ws_root: &Path) -> Result<()> {
    if !git_dir.join("HEAD").exists() {
        std::fs::create_dir_all(git_dir)?;
        let out = git(git_dir, ws_root, &["init", "-q"])?;
        if !out.status.success() {
            return Err(anyhow!(
                "checkpoint init failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        // Own identity + no signing, so commits never depend on the user's git config.
        git(git_dir, ws_root, &["config", "user.name", "Lectern"])?;
        git(
            git_dir,
            ws_root,
            &["config", "user.email", "checkpoints@lectern.local"],
        )?;
        git(git_dir, ws_root, &["config", "commit.gpgsign", "false"])?;
    }
    // Keep excludes fresh in case the baseline list grew between versions.
    write_excludes(git_dir)?;
    Ok(())
}

/// Snapshot the current workspace state. Returns the new checkpoint id, or `None` if the
/// state was identical to the latest checkpoint (nothing to record).
pub fn snapshot(ws_root: &Path, label: &str) -> Result<Option<String>> {
    let git_dir = git_dir_for(ws_root);
    snapshot_in(&git_dir, ws_root, label)
}

fn snapshot_in(git_dir: &Path, ws_root: &Path, label: &str) -> Result<Option<String>> {
    ensure_in(git_dir, ws_root)?;
    let add = git(git_dir, ws_root, &["add", "-A"])?;
    if !add.status.success() {
        return Err(anyhow!(
            "checkpoint add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        ));
    }
    // If we already have a base commit and nothing is staged, don't churn a duplicate.
    if has_head(git_dir, ws_root) {
        let clean = git(git_dir, ws_root, &["diff", "--cached", "--quiet"])?;
        if clean.status.success() {
            return Ok(None);
        }
    }
    let msg = {
        let t = label.trim();
        if t.is_empty() {
            "checkpoint"
        } else {
            t
        }
    };
    let commit = git(
        git_dir,
        ws_root,
        &["commit", "-q", "--allow-empty", "-m", msg],
    )?;
    if !commit.status.success() {
        return Err(anyhow!(
            "checkpoint commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        ));
    }
    Ok(Some(short_head(git_dir, ws_root)?))
}

/// List checkpoints, newest first.
pub fn list(ws_root: &Path) -> Result<Vec<Checkpoint>> {
    let git_dir = git_dir_for(ws_root);
    list_in(&git_dir, ws_root)
}

fn list_in(git_dir: &Path, ws_root: &Path) -> Result<Vec<Checkpoint>> {
    if !git_dir.join("HEAD").exists() {
        return Ok(vec![]);
    }
    let out = git(
        git_dir,
        ws_root,
        &["log", "--format=%h%x00%ct%x00%s", "-n", "200"],
    )?;
    if !out.status.success() {
        return Ok(vec![]); // no commits yet
    }
    let mut v = vec![];
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.splitn(3, '\0');
        let id = parts.next().unwrap_or("").to_string();
        let ts: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let label = parts.next().unwrap_or("").to_string();
        if !id.is_empty() {
            v.push(Checkpoint {
                id,
                label,
                created_at: ts,
            });
        }
    }
    Ok(v)
}

/// Rewind the workspace to checkpoint `id`. Snapshots the current state first (the redo
/// checkpoint) so the rewind is reversible, then resets the work tree to `id`.
pub fn restore(ws_root: &Path, id: &str) -> Result<Restore> {
    let git_dir = git_dir_for(ws_root);
    restore_in(&git_dir, ws_root, id)
}

fn restore_in(git_dir: &Path, ws_root: &Path, id: &str) -> Result<Restore> {
    ensure_in(git_dir, ws_root)?;
    let spec = format!("{id}^{{commit}}");
    let verify = git(git_dir, ws_root, &["rev-parse", "--verify", "-q", &spec])?;
    if !verify.status.success() {
        return Err(anyhow!("unknown checkpoint: {id}"));
    }
    let target = String::from_utf8_lossy(&verify.stdout).trim().to_string();
    // Redo point: capture the current state so the rewind itself can be undone. If the
    // state already matches the latest checkpoint (nothing new to commit), the redo target
    // is simply the current HEAD — that's where "undo the rewind" restores to.
    let fresh = snapshot_in(git_dir, ws_root, "before rewind")?;
    let from = short_head(git_dir, ws_root)?;
    let redo = fresh.or_else(|| Some(from.clone()));
    let changed = diff_names(git_dir, ws_root, id, &from);
    let reset = git(git_dir, ws_root, &["reset", "-q", "--hard", &target])?;
    if !reset.status.success() {
        return Err(anyhow!(
            "rewind failed: {}",
            String::from_utf8_lossy(&reset.stderr)
        ));
    }
    Ok(Restore {
        target,
        redo,
        changed,
    })
}

fn has_head(git_dir: &Path, ws_root: &Path) -> bool {
    git(git_dir, ws_root, &["rev-parse", "--verify", "-q", "HEAD"])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn short_head(git_dir: &Path, ws_root: &Path) -> Result<String> {
    let out = git(git_dir, ws_root, &["rev-parse", "--short", "HEAD"])?;
    if !out.status.success() {
        return Err(anyhow!("checkpoint has no HEAD"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn diff_names(git_dir: &Path, ws_root: &Path, a: &str, b: &str) -> Vec<String> {
    let Ok(out) = git(git_dir, ws_root, &["diff", "--name-only", a, b]) else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway workspace + isolated checkpoint store, cleaned up on drop.
    struct Fixture {
        root: PathBuf,
        ws: PathBuf,
        git_dir: PathBuf,
    }
    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "lectern-ckpt-{tag}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let ws = root.join("ws");
            let git_dir = root.join("shadow.git");
            std::fs::create_dir_all(&ws).unwrap();
            Fixture { root, ws, git_dir }
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.ws.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, body).unwrap();
        }
        fn read(&self, rel: &str) -> Option<String> {
            std::fs::read_to_string(self.ws.join(rel)).ok()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn snapshot_restore_reverts_edits_and_removes_added_files() {
        let f = Fixture::new("basic");
        f.write("a.txt", "hello");
        let a = snapshot_in(&f.git_dir, &f.ws, "before run")
            .unwrap()
            .expect("first snapshot creates a checkpoint");

        // Agent edits a.txt and adds b.txt.
        f.write("a.txt", "AGENT WROTE THIS");
        f.write("b.txt", "new");

        let r = restore_in(&f.git_dir, &f.ws, &a).unwrap();
        assert_eq!(f.read("a.txt").as_deref(), Some("hello"), "edit reverted");
        assert!(f.read("b.txt").is_none(), "added file removed");
        let mut changed = r.changed.clone();
        changed.sort();
        assert_eq!(changed, vec!["a.txt".to_string(), "b.txt".to_string()]);
        assert!(r.redo.is_some(), "a redo checkpoint was captured");
    }

    #[test]
    fn rewind_is_reversible_via_redo_checkpoint() {
        let f = Fixture::new("redo");
        f.write("a.txt", "v1");
        let v1 = snapshot_in(&f.git_dir, &f.ws, "v1").unwrap().unwrap();
        f.write("a.txt", "v2");
        let _ = snapshot_in(&f.git_dir, &f.ws, "v2").unwrap();

        // Rewind to v1, then rewind to the redo checkpoint to get v2 back.
        let r = restore_in(&f.git_dir, &f.ws, &v1).unwrap();
        assert_eq!(f.read("a.txt").as_deref(), Some("v1"));
        let redo = r.redo.expect("redo id");
        restore_in(&f.git_dir, &f.ws, &redo).unwrap();
        assert_eq!(f.read("a.txt").as_deref(), Some("v2"), "redo restores v2");
    }

    #[test]
    fn build_output_is_never_snapshotted() {
        let f = Fixture::new("ignore");
        f.write("src.rs", "fn main() {}");
        f.write("node_modules/dep/index.js", "junk");
        f.write("target/debug/bin", "binary");
        snapshot_in(&f.git_dir, &f.ws, "base").unwrap();
        let files = git(&f.git_dir, &f.ws, &["ls-files"]).unwrap();
        let tracked = String::from_utf8_lossy(&files.stdout);
        assert!(tracked.contains("src.rs"));
        assert!(!tracked.contains("node_modules"), "node_modules excluded");
        assert!(!tracked.contains("target"), "target excluded");
    }

    #[test]
    fn identical_state_makes_no_duplicate_checkpoint() {
        let f = Fixture::new("dup");
        f.write("a.txt", "x");
        snapshot_in(&f.git_dir, &f.ws, "one").unwrap().unwrap();
        // Nothing changed → no new checkpoint.
        assert!(snapshot_in(&f.git_dir, &f.ws, "two").unwrap().is_none());
        assert_eq!(list_in(&f.git_dir, &f.ws).unwrap().len(), 1);
    }

    #[test]
    fn list_is_newest_first() {
        let f = Fixture::new("list");
        f.write("a.txt", "1");
        snapshot_in(&f.git_dir, &f.ws, "first").unwrap();
        f.write("a.txt", "2");
        snapshot_in(&f.git_dir, &f.ws, "second").unwrap();
        let cps = list_in(&f.git_dir, &f.ws).unwrap();
        assert_eq!(cps.len(), 2);
        assert_eq!(cps[0].label, "second");
        assert_eq!(cps[1].label, "first");
    }

    #[test]
    fn works_on_an_empty_workspace() {
        let f = Fixture::new("empty");
        // No files at all → still creates a base checkpoint (via --allow-empty).
        let id = snapshot_in(&f.git_dir, &f.ws, "empty base").unwrap();
        assert!(id.is_some());
        assert_eq!(list_in(&f.git_dir, &f.ws).unwrap().len(), 1);
    }
}
