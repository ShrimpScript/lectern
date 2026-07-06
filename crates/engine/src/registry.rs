//! Community skill hub — the Git-backed marketplace (Phase B).
//!
//! Skills are shared as plain JSON in a public Git repo (default
//! `ShrimpScript/lectern-hub`):
//!
//! ```text
//! lectern-hub/
//! ├── index.json          ← list of RegistryEntry (browse metadata)
//! └── skills/<id>.json     ← one SkillBundle per skill (the full thing)
//! ```
//!
//! Browsing and installing are READ-ONLY HTTP GETs against
//! `raw.githubusercontent.com` — no auth, no token. Installing never runs
//! anything: the caller fetches the bundle, SHOWS the user its exact
//! rules/steps, and only imports on explicit confirmation
//! (review-before-install).
//!
//! Publishing is browser-based: we build GitHub's prefilled "new file" URL so
//! the user proposes the skill as a pull request under their own GitHub login.
//! Nothing sensitive is ever stored in the app.

use crate::SkillBundle;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Which Git repo backs the hub. Editable at `~/.lectern/registry.json`, so a
/// user can point Lectern at a fork or a private mirror.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistryConfig {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        RegistryConfig {
            owner: "ShrimpScript".into(),
            repo: "lectern-hub".into(),
            branch: "main".into(),
        }
    }
}

impl RegistryConfig {
    /// Base for read-only raw fetches.
    pub fn raw_base(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            self.owner, self.repo, self.branch
        )
    }
    /// Human-facing repo page.
    pub fn repo_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }
    /// GitHub's prefilled "create new file" URL — lands the user on a page with
    /// `path` and `content` already filled in; they click "Propose new file" to
    /// open a PR. Used for browser-based publishing (no token in the app).
    pub fn new_file_url(&self, filename: &str, content: &str) -> String {
        format!(
            "https://github.com/{}/{}/new/{}?filename={}&value={}",
            self.owner,
            self.repo,
            self.branch,
            percent_encode(filename),
            percent_encode(content),
        )
    }
}

/// One row in `index.json` — just enough to render a browse card. The full
/// rules/steps live in `skills/<id>.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default = "one")]
    pub version: u32,
    /// Ecosystem tier (2026-07-05): external collections link out with
    /// attribution — they are browsable, never installable from here.
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    /// "instruction" (rules/steps for the agent) or "gui" (recorded replay).
    #[serde(default = "instruction")]
    pub kind: String,
    /// sha256 of the bundle file, computed by the hub's index Action — lets the
    /// client verify the downloaded bundle matches what was indexed/reviewed.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Curated by the Lectern team — shown as an Official shelf/badge.
    #[serde(default)]
    pub official: bool,
}
fn one() -> u32 {
    1
}
fn instruction() -> String {
    "instruction".into()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistryIndex {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<RegistryEntry>,
}

/// Where the editable hub config lives.
pub fn registry_config_path() -> PathBuf {
    let home = crate::home_dir();
    PathBuf::from(home).join(".lectern").join("registry.json")
}

/// Load the hub config fresh each call (so edits take effect live). Missing or
/// invalid → write the defaults so the user has something to edit, and use them.
pub fn config() -> RegistryConfig {
    let path = registry_config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<RegistryConfig>(&text) {
            if !cfg.owner.is_empty() && !cfg.repo.is_empty() {
                return cfg;
            }
        }
    }
    let cfg = RegistryConfig::default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cfg) {
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("lectern: could not write {}: {e}", path.display());
        }
    }
    cfg
}

/// Fetch the hub index (browse). Read-only HTTP GET, no auth.
pub fn fetch_index(cfg: &RegistryConfig) -> Result<Vec<RegistryEntry>> {
    let url = format!("{}/index.json", cfg.raw_base());
    let text = ureq::get(&url)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_string()?;
    let index: RegistryIndex =
        serde_json::from_str(&text).context("parsing community index.json")?;
    Ok(index.skills)
}

/// Fetch one skill's full bundle (for the review-before-install step). Read-only.
/// sha256 hex of a bundle's raw bytes (what the hub's index Action hashes).
pub fn bundle_sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// Fetch a bundle and verify it against the index's sha256 when present.
/// Returns (bundle, verified): verified=false only for legacy unsigned entries;
/// a MISMATCH is a hard error — never install tampered content.
pub fn fetch_bundle_verified(
    cfg: &RegistryConfig,
    id: &str,
    expected_sha: Option<&str>,
) -> Result<(SkillBundle, bool)> {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("invalid skill id: {id}");
    }
    let url = format!("{}/skills/{}.json", cfg.raw_base(), id);
    let text = ureq::get(&url)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_string()?;
    let verified = match expected_sha.map(str::trim).filter(|s| !s.is_empty()) {
        Some(expected) => {
            let actual = bundle_sha256(&text);
            if !actual.eq_ignore_ascii_case(expected) {
                anyhow::bail!(
                    "integrity check FAILED for '{id}': the downloaded file doesn't match the hub index (expected {expected}, got {actual}). Refusing to install."
                );
            }
            true
        }
        None => false,
    };
    let bundle: SkillBundle =
        serde_json::from_str(&text).with_context(|| format!("parsing skills/{id}.json"))?;
    Ok((bundle, verified))
}

pub fn fetch_bundle(cfg: &RegistryConfig, id: &str) -> Result<SkillBundle> {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("invalid skill id: {id}");
    }
    let url = format!("{}/skills/{}.json", cfg.raw_base(), id);
    let text = ureq::get(&url)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_string()?;
    let bundle: SkillBundle =
        serde_json::from_str(&text).with_context(|| format!("parsing skills/{id}.json"))?;
    Ok(bundle)
}

/// Sidecar that records which hub skill (by id) was installed and at what
/// version, so the UI can flag "update available" when the hub has a newer one.
/// A plain JSON map kept beside the brain — no schema migration needed.
pub fn installed_path() -> PathBuf {
    let home = crate::home_dir();
    PathBuf::from(home).join(".lectern").join("installed.json")
}

/// Read the installed-versions map (id -> version). Missing/invalid -> empty.
pub fn load_installed() -> HashMap<String, u32> {
    std::fs::read_to_string(installed_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Record that hub skill `id` was installed at `version` (best-effort).
pub fn record_installed(id: &str, version: u32) {
    let mut map = load_installed();
    map.insert(id.to_string(), version);
    let path = installed_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&map) {
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("lectern: could not write {}: {e}", path.display());
        }
    }
}

/// A URL-safe filename stem for a skill name (matches how `index.json` ids are formed).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true; // avoid leading dash
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("skill");
    }
    out
}

/// Percent-encode a query value (RFC 3986 unreserved set kept; everything else
/// `%XX`). Small + dependency-free; the engine has no urlencoding crate.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_url_safe() {
        assert_eq!(slug("Rust Pre-Push Check!"), "rust-pre-push-check");
        assert_eq!(slug("  spaces  "), "spaces");
        assert_eq!(slug("***"), "skill");
    }

    #[test]
    fn percent_encode_escapes_query() {
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_encode("safe-_.~"), "safe-_.~");
    }

    #[test]
    fn new_file_url_prefills_path_and_content() {
        let cfg = RegistryConfig::default();
        let u = cfg.new_file_url("skills/x.json", "{\"a\":1}");
        assert!(u.contains("/ShrimpScript/lectern-hub/new/main"));
        assert!(u.contains("filename=skills%2Fx.json"));
        assert!(u.contains("value=%7B%22a%22%3A1%7D"));
    }

    #[test]
    fn default_raw_base() {
        let cfg = RegistryConfig::default();
        assert_eq!(
            cfg.raw_base(),
            "https://raw.githubusercontent.com/ShrimpScript/lectern-hub/main"
        );
    }

    #[test]
    fn bundle_hash_is_stable_and_hex() {
        let h = bundle_sha256("{\"name\":\"x\"}");
        assert_eq!(h.len(), 64);
        assert_eq!(h, bundle_sha256("{\"name\":\"x\"}"));
        assert_ne!(h, bundle_sha256("{\"name\":\"y\"}"));
    }

    #[test]
    #[ignore = "network: hits the live community hub"]
    fn fetch_index_and_bundle_live() {
        let cfg = RegistryConfig::default();
        let skills = fetch_index(&cfg).expect("fetch index");
        assert!(skills.iter().any(|s| s.id == "rust-pre-push-check"));
        let b = fetch_bundle(&cfg, "rust-pre-push-check").expect("fetch bundle");
        assert_eq!(b.name, "Rust pre-push check");
        assert!(!b.steps.is_empty());
    }
}
