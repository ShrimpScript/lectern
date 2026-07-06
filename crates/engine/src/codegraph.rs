//! Read a graphify code knowledge-graph (`graphify-out/graph.json`, NetworkX node-link
//! format) and surface it inside Lectern — a summary for the Brain view, and symbol matches
//! that get folded into recall so agents start with code-structure context (Phase B of the
//! graphify integration). Read-only: Lectern consumes a graph the user built with graphify.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    community: Option<i64>,
    #[serde(default)]
    source_file: Option<String>,
}

#[derive(Deserialize)]
struct Link {
    source: String,
    target: String,
}

#[derive(Deserialize)]
struct Graph {
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    links: Vec<Link>,
}

/// A high-level summary of a workspace's code graph (for the Brain view).
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct CodeGraphSummary {
    pub built: bool,
    pub nodes: usize,
    pub edges: usize,
    pub communities: usize,
    /// "God nodes" — the most-connected symbols (a quick map of what matters).
    pub top: Vec<String>,
}

fn graph_path(root: &Path) -> std::path::PathBuf {
    root.join("graphify-out").join("graph.json")
}

fn load(root: &Path) -> Option<Graph> {
    let text = std::fs::read_to_string(graph_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Summarize the code graph for `root` (a workspace). `built: false` when no graph exists.
pub fn summary(root: &Path) -> CodeGraphSummary {
    let Some(g) = load(root) else {
        return CodeGraphSummary::default();
    };
    let mut deg: HashMap<&str, usize> = HashMap::new();
    for l in &g.links {
        *deg.entry(l.source.as_str()).or_default() += 1;
        *deg.entry(l.target.as_str()).or_default() += 1;
    }
    let communities: HashSet<i64> = g.nodes.iter().filter_map(|n| n.community).collect();
    let labels: HashMap<&str, &str> = g
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.label.as_str()))
        .collect();
    let mut ranked: Vec<(&str, usize)> = deg.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let top: Vec<String> = ranked
        .into_iter()
        .filter_map(|(id, _)| labels.get(id).map(|l| l.to_string()))
        .filter(|l| !l.is_empty())
        .take(8)
        .collect();
    CodeGraphSummary {
        built: true,
        nodes: g.nodes.len(),
        edges: g.links.len(),
        communities: communities.len(),
        top,
    }
}

/// Symbols whose label matches a prompt token — code-structure hints folded into recall so
/// the agent knows the relevant functions/types/files up front. Empty when no graph exists.
pub fn recall_symbols(root: &Path, prompt: &str, limit: usize) -> Vec<String> {
    let Some(g) = load(root) else {
        return vec![];
    };
    let p = prompt.to_lowercase();
    let toks: Vec<&str> = p
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 4)
        .collect();
    if toks.is_empty() {
        return vec![];
    }
    let mut hits: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for n in &g.nodes {
        let nl = n.label.to_lowercase();
        if toks.iter().any(|t| nl.contains(t)) && seen.insert(n.label.clone()) {
            let loc = n.source_file.clone().unwrap_or_default();
            hits.push(if loc.is_empty() {
                n.label.clone()
            } else {
                format!("{} ({loc})", n.label)
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_graph_is_not_built() {
        let s = summary(Path::new("/nonexistent-xyz"));
        assert!(!s.built && s.nodes == 0);
        assert!(recall_symbols(Path::new("/nonexistent-xyz"), "anything", 4).is_empty());
    }

    #[test]
    fn summary_and_recall_on_a_small_graph() {
        // Write a tiny NetworkX node-link graph to a temp workspace, then exercise
        // the populated path: degree ranking, community count, and token matching.
        let dir = std::env::temp_dir().join(format!("lectern-cg-{}", std::process::id()));
        let gdir = dir.join("graphify-out");
        std::fs::create_dir_all(&gdir).unwrap();
        let graph = r#"{
          "nodes": [
            {"id": "a", "label": "parse_config", "community": 1, "source_file": "src/config.rs"},
            {"id": "b", "label": "ConfigError", "community": 1},
            {"id": "c", "label": "main", "community": 2}
          ],
          "links": [
            {"source": "a", "target": "b"},
            {"source": "c", "target": "a"}
          ]
        }"#;
        std::fs::write(gdir.join("graph.json"), graph).unwrap();

        let s = summary(&dir);
        assert!(s.built);
        assert_eq!(s.nodes, 3);
        assert_eq!(s.edges, 2);
        assert_eq!(s.communities, 2);
        // "a" (parse_config) has degree 2 → the top god-node.
        assert_eq!(s.top.first().map(String::as_str), Some("parse_config"));

        // Token "config" (>= 4 chars) matches "parse_config" and "ConfigError";
        // the source_file is appended when present.
        let hits = recall_symbols(&dir, "fix the config parsing", 4);
        assert!(hits.iter().any(|h| h == "parse_config (src/config.rs)"));
        assert!(hits.iter().any(|h| h == "ConfigError"));
        // Only short tokens (< 4 chars) → no matches.
        assert!(recall_symbols(&dir, "a b c go", 4).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
