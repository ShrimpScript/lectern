//! Zero-token skill self-regulation. Every run that auto-applied skills records
//! an outcome per skill in a sidecar (`<data_dir>/skill-stats.json`, no schema
//! migration). A skill that keeps failing pauses itself — it stops auto-applying
//! until the user re-enables it (which clears its record). Deliberately NOT the
//! autonomous-refinement loop: no background model calls, no self-rewriting.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStats {
    #[serde(default)]
    pub ok: u32,
    #[serde(default)]
    pub err: u32,
    #[serde(default)]
    pub last_err_ts: i64,
}

pub fn stats_path() -> PathBuf {
    crate::data_dir().join("skill-stats.json")
}

pub fn load() -> HashMap<String, SkillStats> {
    std::fs::read_to_string(stats_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(map: &HashMap<String, SkillStats>) {
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let path = stats_path();
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Err(e) = std::fs::write(&path, json) {
            crate::diag::log("skills", &format!("stats write failed: {e}"));
        }
    }
}

/// Record one run's outcome for every skill that was auto-applied to it.
pub fn record_outcome(names: &[String], ok: bool, now: i64) {
    if names.is_empty() {
        return;
    }
    let mut map = load();
    for n in names {
        let s = map.entry(n.clone()).or_default();
        if ok {
            s.ok += 1;
        } else {
            s.err += 1;
            s.last_err_ts = now;
        }
    }
    save(&map);
}

/// The pause rule: at least 3 recorded uses AND failures outnumber successes.
/// (Strictly more than half — a 50/50 skill keeps running.)
pub fn is_paused(s: &SkillStats) -> bool {
    let uses = s.ok + s.err;
    uses >= 3 && s.err * 2 > uses
}

/// Re-enable a paused skill by clearing its record (a fresh start).
pub fn reset(name: &str) {
    let mut map = load();
    if map.remove(name).is_some() {
        save(&map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_rule_boundaries() {
        let s = |ok, err| SkillStats {
            ok,
            err,
            last_err_ts: 0,
        };
        assert!(!is_paused(&s(0, 2)), "under 3 uses never pauses");
        assert!(!is_paused(&s(2, 2)), "50/50 keeps running");
        assert!(is_paused(&s(1, 2)), "2 of 3 failing pauses");
        assert!(is_paused(&s(0, 3)));
        assert!(!is_paused(&s(10, 3)), "mostly-good skill keeps running");
    }

    #[test]
    fn stats_roundtrip_serde() {
        let mut m: HashMap<String, SkillStats> = HashMap::new();
        m.insert(
            "x".into(),
            SkillStats {
                ok: 2,
                err: 1,
                last_err_ts: 42,
            },
        );
        let j = serde_json::to_string(&m).unwrap();
        let back: HashMap<String, SkillStats> = serde_json::from_str(&j).unwrap();
        assert_eq!(back["x"].ok, 2);
        assert_eq!(back["x"].err, 1);
    }
}
