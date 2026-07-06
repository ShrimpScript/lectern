//! Per-task model routing — pick the harness + model that excels at the kind of work in
//! the prompt (Omnigent-style per-task delegation). Multi-provider (Claude via Claude Code,
//! Gemini via Antigravity).
//!
//! v3: the rules are no longer hard-coded — they live in an EDITABLE config at
//! `~/.lectern/routing.json` (written with sensible defaults on first run), so the user can
//! retune which work goes to which model without recompiling. A learned/LLM classifier can
//! slot in behind the same `route_model` signature later.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Exact Antigravity (`agy`) model strings (from `agy models`). Flash = fast; Pro = huge
/// context window (great for whole-codebase / large-document work).
const GEMINI_FLASH: &str = "Gemini 3.5 Flash (High)";
const GEMINI_PRO: &str = "Gemini 3.1 Pro (High)";

/// A routing decision: which backend/harness + model to use, with a UI label + reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub backend: String,
    pub model: String,
    pub label: String,
    pub reason: String,
}

/// One editable routing rule: matches when any keyword is present OR the prompt is at most
/// `max_words` long. First matching rule (in order) wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Rule {
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    max_words: Option<usize>,
    backend: String,
    model: String,
    label: String,
    reason: String,
}

/// The full routing config: ordered rules + the fallback when nothing matches, plus an
/// optional classifier that refines AMBIGUOUS tasks (those that hit no rule).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    rules: Vec<Rule>,
    default_backend: String,
    default_model: String,
    default_label: String,
    default_reason: String,
    /// When true, a fast model classifies tasks that match no rule (instead of just falling
    /// back) — the "both" mode: editable presets + a classifier. Off by default (no latency).
    #[serde(default)]
    use_classifier: bool,
    #[serde(default = "default_classifier_backend")]
    classifier_backend: String,
    #[serde(default = "default_classifier_model")]
    classifier_model: String,
}

fn default_classifier_backend() -> String {
    "claude-code".into()
}
fn default_classifier_model() -> String {
    "haiku".into()
}

const VISION: &[&str] = &[
    "image",
    "screenshot",
    "screen shot",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
    "photo",
    "diagram",
    "mockup",
    "what's in this",
    "look at this image",
    "ocr",
];
const DESKTOP: &[&str] = &[
    "remote desktop",
    "control the desktop",
    "click at",
    "keystroke",
    "keypress",
    "xdotool",
    "wmctrl",
    "open the app",
    "launch the app",
    "switch window",
    "scroll down",
    "scroll up",
];
const HEAVY: &[&str] = &[
    "architect",
    "architecture",
    "design ",
    "redesign",
    "plan ",
    "planning",
    "refactor",
    "debug",
    "root cause",
    "root-cause",
    "investigate",
    "diagnose",
    "figure out",
    "optimi",
    "performance",
    "algorithm",
    "security",
    "vulnerab",
    "race condition",
    "concurren",
    "deadlock",
    "trace through",
    "reason about",
    "complex",
    "rewrite the",
    "overhaul",
];
const QUICK: &[&str] = &[
    "typo",
    "rename",
    "format",
    "reformat",
    "lint",
    "whitespace",
    "indent",
    "spelling",
    "rephrase",
    "reword",
    "gitignore",
    "bump",
    "one-liner",
    "one liner",
    "small fix",
    "quick ",
    "tweak",
    "comment out",
    "add a comment",
];

/// Large-context / whole-corpus work → Gemini 3.1 Pro (its very large context window shines).
const BIGCTX: &[&str] = &[
    "entire codebase",
    "whole codebase",
    "whole repo",
    "across the codebase",
    "across all files",
    "all the files",
    "every file",
    "large file",
    "huge file",
    "long document",
    "summarize the project",
    "summarize the repo",
    "long context",
    "large context",
    "big context",
    "many files",
];

fn kw(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

/// The built-in defaults — written to `routing.json` on first run, then user-editable.
/// Order is priority: vision → desktop → heavy → quick → (fallback) sonnet.
fn default_config() -> RoutingConfig {
    RoutingConfig {
        rules: vec![
            Rule {
                keywords: kw(VISION),
                max_words: None,
                backend: "antigravity".into(),
                model: GEMINI_FLASH.into(),
                label: "Gemini 3.5 Flash".into(),
                reason: "image / vision task → Gemini Flash".into(),
            },
            Rule {
                keywords: kw(DESKTOP),
                max_words: None,
                backend: "antigravity".into(),
                model: GEMINI_FLASH.into(),
                label: "Gemini 3.5 Flash".into(),
                reason: "fast desktop / command task → Gemini Flash".into(),
            },
            Rule {
                keywords: kw(HEAVY),
                max_words: None,
                backend: "claude-code".into(),
                model: "opus".into(),
                label: "Opus 4.8".into(),
                reason: "deep reasoning / architecture → Opus".into(),
            },
            Rule {
                keywords: kw(BIGCTX),
                max_words: None,
                backend: "antigravity".into(),
                model: GEMINI_PRO.into(),
                label: "Gemini 3.1 Pro".into(),
                reason: "large-context / whole-codebase task → Gemini 3.1 Pro".into(),
            },
            Rule {
                keywords: kw(QUICK),
                max_words: Some(3),
                backend: "claude-code".into(),
                model: "haiku".into(),
                label: "Haiku 4.5".into(),
                reason: "quick, low-complexity task → Haiku".into(),
            },
        ],
        default_backend: "claude-code".into(),
        default_model: "sonnet".into(),
        default_label: "Sonnet 4.6".into(),
        default_reason: "general coding task → Sonnet".into(),
        use_classifier: false,
        classifier_backend: default_classifier_backend(),
        classifier_model: default_classifier_model(),
    }
}

/// Where the editable routing config lives.
pub fn routing_config_path() -> PathBuf {
    let home = crate::home_dir();
    PathBuf::from(home).join(".lectern").join("routing.json")
}

/// Load the routing config fresh from `~/.lectern/routing.json` (read each call, so edits and
/// the in-app classifier toggle take effect live). If missing/invalid, writes the defaults
/// there so the user has something to edit, and uses them. The file is tiny; this isn't hot.
fn config() -> RoutingConfig {
    let path = routing_config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<RoutingConfig>(&text) {
            if !cfg.rules.is_empty() || !cfg.default_model.is_empty() {
                return cfg;
            }
        }
    }
    let cfg = default_config();
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

/// A UI-facing snapshot of the routing config: where the file lives and what it
/// currently says (config visibility; the file self-writes defaults
/// on first read, so after this call it always exists to open/edit).
#[derive(Debug, Clone, Serialize)]
pub struct RoutingSummary {
    pub path: String,
    pub default_label: String,
    pub use_classifier: bool,
    pub rules: Vec<RuleSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleSummary {
    pub label: String,
    pub keywords: Vec<String>,
    pub max_words: Option<usize>,
    pub target: String,
}

pub fn routing_summary() -> RoutingSummary {
    let cfg = config();
    RoutingSummary {
        path: routing_config_path().display().to_string(),
        default_label: cfg.default_label.clone(),
        use_classifier: cfg.use_classifier,
        rules: cfg
            .rules
            .iter()
            .map(|r| RuleSummary {
                label: r.label.clone(),
                keywords: r.keywords.clone(),
                max_words: r.max_words,
                target: format!("{}/{}", r.backend, r.model),
            })
            .collect(),
    }
}

/// Toggle the classifier on/off and persist it to `routing.json` (for the Settings switch).
pub fn set_classifier(on: bool) -> std::io::Result<()> {
    let mut cfg = config();
    cfg.use_classifier = on;
    let path = routing_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&cfg).map_err(std::io::Error::other)?;
    std::fs::write(&path, text)
}

/// Route a prompt per the config, and report whether a rule matched (vs the fallback).
/// `matched == false` means the task is ambiguous — the classifier (if enabled) refines it.
pub fn route_detail(prompt: &str) -> (Route, bool) {
    let cfg = config();
    let p = prompt.to_lowercase();
    let words = p.split_whitespace().count();
    for r in &cfg.rules {
        let kw_hit = r.keywords.iter().any(|k| p.contains(k.as_str()));
        let short = r.max_words.is_some_and(|m| words <= m);
        if kw_hit || short {
            return (
                Route {
                    backend: r.backend.clone(),
                    model: r.model.clone(),
                    label: r.label.clone(),
                    reason: r.reason.clone(),
                },
                true,
            );
        }
    }
    (
        Route {
            backend: cfg.default_backend.clone(),
            model: cfg.default_model.clone(),
            label: cfg.default_label.clone(),
            reason: cfg.default_reason.clone(),
        },
        false,
    )
}

/// Route a prompt to the best harness + model, per the (editable) routing config.
pub fn route_model(prompt: &str) -> Route {
    route_detail(prompt).0
}

/// Whether the optional classifier layer is enabled in the config.
pub fn classifier_enabled() -> bool {
    config().use_classifier
}

/// The (backend, model) to run the classifier on (a fast model).
pub fn classifier_target() -> (String, String) {
    let c = config();
    (c.classifier_backend.clone(), c.classifier_model.clone())
}

/// Map a classifier's one-word verdict to a Route. `None` (e.g. "general") → keep the base.
pub fn classifier_route(word: &str) -> Option<Route> {
    let w = word.to_lowercase();
    if w.contains("heavy") {
        Some(Route {
            backend: "claude-code".into(),
            model: "opus".into(),
            label: "Opus 4.8".into(),
            reason: "classified as deep work → Opus".into(),
        })
    } else if w.contains("quick") {
        Some(Route {
            backend: "claude-code".into(),
            model: "haiku".into(),
            label: "Haiku 4.5".into(),
            reason: "classified as quick → Haiku".into(),
        })
    } else {
        None
    }
}

fn route_to(backend: &str, model: &str, label: &str, reason: String) -> Route {
    Route {
        backend: backend.into(),
        model: model.into(),
        label: label.into(),
        reason,
    }
}

/// The offline fallback when no agent CLI is connected.
fn mock_route() -> Route {
    route_to(
        "mock",
        "",
        "Mock",
        "no agent connected — offline demo pipeline".into(),
    )
}

/// Make a routing decision RUNNABLE on this machine: if its preferred provider isn't
/// connected, remap to the best AVAILABLE provider while preserving the task tier (quick vs
/// capable). This is what lets Lectern work with whatever a user has — Claude Code only,
/// Antigravity/Gemini only, both, or (degraded) neither.
pub fn available_route(r: Route, claude_ok: bool, agy_ok: bool) -> Route {
    let backend_ok = match r.backend.as_str() {
        "claude-code" => claude_ok,
        "antigravity" => agy_ok,
        _ => true, // auto / mock resolve elsewhere
    };
    if backend_ok {
        return r;
    }
    // A "quick" tier model (Haiku / Flash) maps to the other provider's fast model; anything
    // else (Sonnet / Opus / Pro / GPT-OSS) maps to the other provider's capable model.
    let m = r.model.to_lowercase();
    let quick = m.contains("haiku") || m.contains("flash");
    let note = |to: &str| format!("{} not connected → {to}", r.label);

    match r.backend.as_str() {
        "claude-code" if agy_ok => {
            if quick {
                route_to(
                    "antigravity",
                    "Gemini 3.5 Flash (High)",
                    "Gemini 3.5 Flash",
                    note("Gemini 3.5 Flash"),
                )
            } else {
                route_to(
                    "antigravity",
                    "Gemini 3.1 Pro (High)",
                    "Gemini 3.1 Pro",
                    note("Gemini 3.1 Pro"),
                )
            }
        }
        "antigravity" if claude_ok => {
            if quick {
                route_to("claude-code", "haiku", "Haiku 4.5", note("Haiku 4.5"))
            } else {
                route_to("claude-code", "sonnet", "Sonnet 4.6", note("Sonnet 4.6"))
            }
        }
        _ => mock_route(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tests exercise the DEFAULT config directly (no file I/O), so they stay
    // deterministic regardless of any routing.json on the machine.
    fn route_with(cfg: &RoutingConfig, prompt: &str) -> Route {
        let p = prompt.to_lowercase();
        let words = p.split_whitespace().count();
        for r in &cfg.rules {
            if r.keywords.iter().any(|k| p.contains(k.as_str()))
                || r.max_words.is_some_and(|m| words <= m)
            {
                return Route {
                    backend: r.backend.clone(),
                    model: r.model.clone(),
                    label: r.label.clone(),
                    reason: r.reason.clone(),
                };
            }
        }
        Route {
            backend: cfg.default_backend.clone(),
            model: cfg.default_model.clone(),
            label: cfg.default_label.clone(),
            reason: cfg.default_reason.clone(),
        }
    }

    #[test]
    fn routes_vision_and_desktop_to_gemini() {
        let c = default_config();
        assert_eq!(
            route_with(&c, "describe this screenshot").backend,
            "antigravity"
        );
        assert_eq!(
            route_with(&c, "what's in this image?").backend,
            "antigravity"
        );
        assert_eq!(
            route_with(&c, "open the app and click at 100,200").backend,
            "antigravity"
        );
    }

    #[test]
    fn routes_heavy_to_opus() {
        let c = default_config();
        let r = route_with(&c, "Refactor the auth module and explain the design");
        assert_eq!(
            (r.backend.as_str(), r.model.as_str()),
            ("claude-code", "opus")
        );
        assert_eq!(
            route_with(&c, "debug why the worktree merge deadlocks").model,
            "opus"
        );
    }

    #[test]
    fn routes_quick_to_haiku() {
        let c = default_config();
        assert_eq!(route_with(&c, "fix a typo in the README").model, "haiku");
        assert_eq!(route_with(&c, "add a button").model, "haiku"); // terse
    }

    #[test]
    fn routes_general_to_sonnet() {
        let c = default_config();
        let r = route_with(
            &c,
            "add a settings page with a dark mode toggle and persistence",
        );
        assert_eq!(
            (r.backend.as_str(), r.model.as_str()),
            ("claude-code", "sonnet")
        );
    }

    #[test]
    fn routes_large_context_to_gemini_pro() {
        let c = default_config();
        let r = route_with(&c, "summarize the whole codebase and how it fits together");
        assert_eq!(r.backend, "antigravity");
        assert_eq!(r.model, "Gemini 3.1 Pro (High)");
    }

    #[test]
    fn config_roundtrips_json() {
        let c = default_config();
        let json = serde_json::to_string(&c).unwrap();
        let back: RoutingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rules.len(), c.rules.len());
        assert_eq!(back.default_model, "sonnet");
        assert!(!back.use_classifier); // off by default — no added latency
    }

    #[test]
    fn classifier_maps_verdicts() {
        assert_eq!(classifier_route("heavy").unwrap().model, "opus");
        assert_eq!(
            classifier_route("looks quick to me").unwrap().model,
            "haiku"
        );
        assert!(classifier_route("general").is_none()); // keep the preset/fallback
    }

    #[test]
    fn available_route_remaps_to_connected_provider() {
        let opus = Route {
            backend: "claude-code".into(),
            model: "opus".into(),
            label: "Opus 4.8".into(),
            reason: "x".into(),
        };
        let haiku = Route {
            backend: "claude-code".into(),
            model: "haiku".into(),
            label: "Haiku 4.5".into(),
            reason: "x".into(),
        };
        let flash = Route {
            backend: "antigravity".into(),
            model: "Gemini 3.5 Flash (High)".into(),
            label: "Gemini 3.5 Flash".into(),
            reason: "x".into(),
        };
        // both connected → unchanged
        assert_eq!(available_route(opus.clone(), true, true).model, "opus");
        // no Claude, Antigravity present → Claude tiers map to Gemini tiers
        assert_eq!(
            available_route(opus.clone(), false, true).backend,
            "antigravity"
        );
        assert_eq!(
            available_route(opus.clone(), false, true).model,
            "Gemini 3.1 Pro (High)"
        );
        assert_eq!(
            available_route(haiku.clone(), false, true).model,
            "Gemini 3.5 Flash (High)"
        );
        // no Antigravity, Claude present → Gemini fast → Haiku
        assert_eq!(available_route(flash.clone(), true, false).model, "haiku");
        // neither → mock
        assert_eq!(available_route(opus, false, false).backend, "mock");
    }

    #[test]
    fn old_config_without_classifier_fields_still_parses() {
        // A routing.json written before the classifier existed must still load.
        let json = r#"{"rules":[],"default_backend":"claude-code","default_model":"sonnet","default_label":"Sonnet 4.6","default_reason":"x"}"#;
        let c: RoutingConfig = serde_json::from_str(json).unwrap();
        assert!(!c.use_classifier && c.classifier_model == "haiku");
    }

    #[test]
    fn always_has_label_and_reason() {
        let c = default_config();
        for p in [
            "",
            "refactor",
            "fix typo",
            "build a normal feature",
            "screenshot this",
        ] {
            let r = route_with(&c, p);
            assert!(!r.label.is_empty() && !r.reason.is_empty() && !r.backend.is_empty());
        }
    }
}
