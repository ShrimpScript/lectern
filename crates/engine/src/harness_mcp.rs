//! Cross-harness MCP registration. One add in Lectern should reach
//! every agent the user has — each harness keeps its own config format, so each
//! gets a writer that READ-MERGES-WRITES and preserves everything it doesn't
//! understand. Hard rule: content we can't parse is never overwritten — we refuse
//! with a clear error instead (the user may have hand-edited state in there).
//! Formats verified in docs/mcp-cross-harness.md (research cycle C4a).
use crate::backend::Backend;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// A harness-agnostic server spec: either a local stdio command or a remote URL.
#[derive(Debug, Clone)]
pub struct McpSpec {
    pub name: String,
    /// Local server: the full command split into argv (e.g. ["npx","-y","pkg"]).
    pub command: Vec<String>,
    /// Remote server: the endpoint. When set, `command` is ignored.
    pub url: Option<String>,
    pub env: Vec<(String, String)>,
}

impl McpSpec {
    /// Build a spec from the UI's raw fields: a URL command becomes a remote
    /// server; anything else splits on whitespace into argv.
    pub fn parse(name: &str, command: &str, env: Vec<(String, String)>) -> Self {
        let command = command.trim();
        if command.starts_with("http://") || command.starts_with("https://") {
            McpSpec {
                name: name.trim().to_string(),
                command: vec![],
                url: Some(command.to_string()),
                env,
            }
        } else {
            McpSpec {
                name: name.trim().to_string(),
                command: command.split_whitespace().map(str::to_string).collect(),
                url: None,
                env,
            }
        }
    }
}

/// OpenCode's global config (`opencode.json` under XDG config).
pub fn opencode_config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", crate::home_dir()));
    PathBuf::from(base).join("opencode").join("opencode.json")
}

/// Load a JSON object from `path`, treating a missing or empty file as `{}` and
/// refusing (not clobbering) anything unparseable.
fn load_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => bail!(
            "{} exists but isn't valid JSON — refusing to modify it. Fix or remove the file, then retry.",
            path.display()
        ),
    }
}

fn save_object(path: &Path, map: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map.clone()))?;
    std::fs::write(path, text + "\n").with_context(|| format!("writing {}", path.display()))
}

/// Merge `spec` into an opencode.json at `path` (testable core; see the schema in
/// docs/mcp-cross-harness.md — local: type/command/environment, remote: type/url).
pub fn merge_opencode(path: &Path, spec: &McpSpec) -> Result<()> {
    let mut root = load_object(path)?;
    let mcp = root
        .entry("mcp".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let Some(mcp_map) = mcp.as_object_mut() else {
        bail!("opencode.json has a non-object \"mcp\" key — refusing to modify it.");
    };
    let entry = if let Some(url) = &spec.url {
        serde_json::json!({ "type": "remote", "url": url, "enabled": true })
    } else {
        let mut e =
            serde_json::json!({ "type": "local", "command": spec.command, "enabled": true });
        if !spec.env.is_empty() {
            let env: serde_json::Map<String, serde_json::Value> = spec
                .env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            e["environment"] = serde_json::Value::Object(env);
        }
        e
    };
    mcp_map.insert(spec.name.clone(), entry);
    save_object(path, &root)
}

/// Remove a server by name from an opencode.json. Ok(false) when it wasn't there.
pub fn remove_opencode(path: &Path, name: &str) -> Result<bool> {
    let mut root = load_object(path)?;
    let removed = root
        .get_mut("mcp")
        .and_then(|m| m.as_object_mut())
        .map(|m| m.remove(name).is_some())
        .unwrap_or(false);
    if removed {
        save_object(path, &root)?;
    }
    Ok(removed)
}

/// Whether OpenCode looks present (binary on PATH or in its install dirs) — the
/// fan-out only writes configs for harnesses that exist.
pub fn opencode_detected() -> bool {
    crate::OpenCodeBackend::new().available()
}

// ── Antigravity ────────────────────────────────────────────────────────────────
// C4a found TWO mcp_config.json placeholders (~/.gemini/config/ and
// ~/.gemini/antigravity/) — different agy components appear to read different
// paths, so registration writes every one whose parent dir exists. Schema is the
// Gemini-family standard: {"mcpServers": {name: {command, args, env}}}. That
// shape is MEDIUM confidence (files were empty; validated by the first real
// add) — which is exactly why the merge is defensive and remote servers are
// refused until verified.

/// Every Antigravity MCP config location whose parent directory exists.
pub fn antigravity_config_paths() -> Vec<PathBuf> {
    let home = crate::home_dir();
    [
        format!("{home}/.gemini/config/mcp_config.json"),
        format!("{home}/.gemini/antigravity/mcp_config.json"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.parent().is_some_and(|d| d.is_dir()))
    .collect()
}

/// Merge `spec` into one Antigravity mcp_config.json (testable core).
pub fn merge_antigravity(path: &Path, spec: &McpSpec) -> Result<()> {
    if spec.url.is_some() {
        bail!(
            "Antigravity remote-MCP support is unverified — local (command) servers only for now."
        );
    }
    if spec.command.is_empty() {
        bail!("a local MCP server needs a command");
    }
    let mut root = load_object(path)?;
    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let Some(map) = servers.as_object_mut() else {
        bail!(
            "{} has a non-object \"mcpServers\" key — refusing to modify it.",
            path.display()
        );
    };
    let mut entry = serde_json::json!({
        "command": spec.command[0],
        "args": spec.command[1..],
    });
    if !spec.env.is_empty() {
        let env: serde_json::Map<String, serde_json::Value> = spec
            .env
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        entry["env"] = serde_json::Value::Object(env);
    }
    map.insert(spec.name.clone(), entry);
    save_object(path, &root)
}

/// Remove a server from one Antigravity config. Ok(false) when absent.
pub fn remove_antigravity(path: &Path, name: &str) -> Result<bool> {
    let mut root = load_object(path)?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(|m| m.as_object_mut())
        .map(|m| m.remove(name).is_some())
        .unwrap_or(false);
    if removed {
        save_object(path, &root)?;
    }
    Ok(removed)
}

/// Whether a server name is present in OpenCode's config (truthful per-row chips).
pub fn opencode_has(name: &str) -> bool {
    load_object(&opencode_config_path())
        .ok()
        .and_then(|r| r.get("mcp").cloned())
        .and_then(|m| m.as_object().map(|o| o.contains_key(name)))
        .unwrap_or(false)
}

/// Whether a server name is present in any Antigravity config.
pub fn antigravity_has(name: &str) -> bool {
    antigravity_config_paths().iter().any(|p| {
        load_object(p)
            .ok()
            .and_then(|r| r.get("mcpServers").cloned())
            .and_then(|m| m.as_object().map(|o| o.contains_key(name)))
            .unwrap_or(false)
    })
}

pub fn antigravity_detected() -> bool {
    crate::AntigravityBackend::new().available()
}

/// Read-only overview for status surfaces (TUI /mcp-servers): server names
/// registered in each harness config. Never writes; missing files → empty.
pub fn harness_mcp_overview() -> serde_json::Value {
    harness_mcp_overview_at(
        &std::path::PathBuf::from(crate::home_dir()).join(".claude.json"),
        &opencode_config_path(),
        &antigravity_config_paths(),
    )
}

/// Injectable-path core (unit-testable without touching real configs).
pub fn harness_mcp_overview_at(
    claude_config: &std::path::Path,
    opencode_config: &std::path::Path,
    antigravity_configs: &[std::path::PathBuf],
) -> serde_json::Value {
    let names = |v: Option<serde_json::Value>| -> Vec<String> {
        v.and_then(|m| m.as_object().map(|o| o.keys().cloned().collect()))
            .unwrap_or_default()
    };
    let claude = {
        let root: Option<serde_json::Value> = std::fs::read_to_string(claude_config)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        names(root.and_then(|r| r.get("mcpServers").cloned()))
    };
    let opencode = names(
        load_object(opencode_config)
            .ok()
            .and_then(|r| r.get("mcp").cloned()),
    );
    let antigravity = antigravity_configs
        .iter()
        .flat_map(|p| {
            names(
                load_object(p)
                    .ok()
                    .and_then(|r| r.get("mcpServers").cloned()),
            )
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    serde_json::json!({ "claude": claude, "opencode": opencode, "antigravity": antigravity })
}

#[cfg(test)]
mod tests {
    #[test]
    fn overview_reads_all_three_harness_shapes() {
        let claude = tmp("ov-claude");
        let oc = tmp("ov-oc");
        let agy = tmp("ov-agy");
        std::fs::write(&claude, r#"{"mcpServers":{"github":{},"gcal":{}}}"#).unwrap();
        std::fs::write(&oc, r#"{"mcp":{"context7":{"type":"remote"}}}"#).unwrap();
        std::fs::write(&agy, r#"{"mcpServers":{"memory":{}}}"#).unwrap();
        let v = harness_mcp_overview_at(&claude, &oc, std::slice::from_ref(&agy));
        assert_eq!(v["claude"].as_array().unwrap().len(), 2);
        assert_eq!(v["opencode"][0], "context7");
        assert_eq!(v["antigravity"][0], "memory");
        // missing/garbage files → empty lists, never errors
        let v = harness_mcp_overview_at(&tmp("ov-none1"), &tmp("ov-none2"), &[tmp("ov-none3")]);
        assert!(v["claude"].as_array().unwrap().is_empty());
        std::fs::write(&agy, "garbage not json").unwrap();
        let v = harness_mcp_overview_at(&claude, &oc, &[agy]);
        assert!(v["antigravity"].as_array().unwrap().is_empty());
    }

    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("lectern-hmcp-{}-{}.json", name, std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn spec(name: &str) -> McpSpec {
        McpSpec {
            name: name.into(),
            command: vec!["npx".into(), "-y".into(), "pkg".into()],
            url: None,
            env: vec![("TOKEN".into(), "x".into())],
        }
    }

    #[test]
    fn spec_parse_detects_remote_vs_local() {
        let r = McpSpec::parse("linear", " https://mcp.linear.app/mcp ", vec![]);
        assert_eq!(r.url.as_deref(), Some("https://mcp.linear.app/mcp"));
        let l = McpSpec::parse("gh", "npx -y @modelcontextprotocol/server-github", vec![]);
        assert_eq!(l.url, None);
        assert_eq!(l.command.len(), 3);
    }

    #[test]
    fn creates_config_from_nothing() {
        let p = tmp("create");
        merge_opencode(&p, &spec("github")).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["mcp"]["github"]["type"], "local");
        assert_eq!(v["mcp"]["github"]["command"][0], "npx");
        assert_eq!(v["mcp"]["github"]["environment"]["TOKEN"], "x");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn preserves_unknown_keys_and_other_servers() {
        let p = tmp("preserve");
        std::fs::write(
            &p,
            r#"{"$schema":"https://opencode.ai/config.json","theme":"dark","mcp":{"old":{"type":"local","command":["x"]}}}"#,
        )
        .unwrap();
        merge_opencode(&p, &spec("new")).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["$schema"], "https://opencode.ai/config.json");
        assert_eq!(v["mcp"]["old"]["command"][0], "x");
        assert_eq!(v["mcp"]["new"]["type"], "local");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn refuses_unparseable_content() {
        let p = tmp("refuse");
        std::fs::write(&p, "{ not json").unwrap();
        let err = merge_opencode(&p, &spec("x")).unwrap_err().to_string();
        assert!(err.contains("refusing"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn agy_seeds_the_empty_placeholder_file() {
        // The real machines have 0-byte mcp_config.json placeholders — exactly this.
        let p = tmp("agy-empty");
        std::fs::write(&p, "").unwrap();
        merge_antigravity(&p, &spec("github")).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], "npx");
        assert_eq!(v["mcpServers"]["github"]["args"][0], "-y");
        assert_eq!(v["mcpServers"]["github"]["env"]["TOKEN"], "x");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn agy_preserves_refuses_and_removes() {
        let p = tmp("agy-guard");
        std::fs::write(
            &p,
            r#"{"otherTopLevel":1,"mcpServers":{"keep":{"command":"x","args":[]}}}"#,
        )
        .unwrap();
        merge_antigravity(&p, &spec("new")).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["otherTopLevel"], 1);
        assert_eq!(v["mcpServers"]["keep"]["command"], "x");
        assert!(remove_antigravity(&p, "new").unwrap());
        assert!(!remove_antigravity(&p, "new").unwrap());

        let bad = tmp("agy-bad");
        std::fs::write(&bad, "not json at all").unwrap();
        assert!(merge_antigravity(&bad, &spec("x")).is_err());
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "not json at all");

        let mut remote = spec("r");
        remote.url = Some("https://x".into());
        assert!(merge_antigravity(&p, &remote).is_err());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn presence_readers_see_merged_servers() {
        let p = tmp("presence");
        merge_opencode(&p, &spec("present")).unwrap();
        let root = load_object(&p).unwrap();
        let has = root
            .get("mcp")
            .and_then(|m| m.as_object())
            .is_some_and(|o| o.contains_key("present") && !o.contains_key("absent"));
        assert!(has);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn remote_shape_and_removal() {
        let p = tmp("remote");
        let mut s = spec("linear");
        s.url = Some("https://mcp.linear.app/mcp".into());
        merge_opencode(&p, &s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["mcp"]["linear"]["type"], "remote");
        assert!(remove_opencode(&p, "linear").unwrap());
        assert!(!remove_opencode(&p, "linear").unwrap());
        let _ = std::fs::remove_file(&p);
    }
}
