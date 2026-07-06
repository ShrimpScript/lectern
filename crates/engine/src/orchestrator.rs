//! The Conductor — an Omnigent/Polly-style multi-model orchestrator. It decomposes a
//! task into ordered sub-tasks, then HANDS EACH OFF to the model that excels at it
//! (via [`crate::route`]), streaming the plan + per-step progress + a summary.
//!
//! v1 (C1) is sequential. Parallel git-worktree fan-out (C2) and cross-model review
//! (C3) layer on top of this without changing the shape.

use serde::Deserialize;

/// One sub-task in a Conductor plan.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ConductorStep {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// "code" | "command" | "research" — informs review (C3) and routing hints.
    #[serde(default)]
    pub kind: String,
    /// Safe-by-default parallelism (C2): true ONLY when this step is independent of its
    /// neighbours and safe to run at the same time. Consecutive parallel steps run
    /// concurrently in isolated git worktrees; everything else stays sequential.
    #[serde(default)]
    pub parallel: bool,
}

/// Group steps for execution: a maximal run of consecutive `parallel` steps becomes one
/// group (run concurrently when len > 1); every other step is its own sequential group.
/// Order is preserved, so dependent work still runs after the work it builds on.
pub fn parallel_groups(steps: &[ConductorStep]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        if steps[i].parallel {
            let mut g = vec![i];
            let mut j = i + 1;
            while j < steps.len() && steps[j].parallel {
                g.push(j);
                j += 1;
            }
            groups.push(g);
            i = j;
        } else {
            groups.push(vec![i]);
            i += 1;
        }
    }
    groups
}

/// The instruction handed to the planner model. Asks for a strict JSON array so the
/// plan is machine-parseable regardless of which backend produced it.
pub const PLAN_INSTRUCTION: &str = "You are the planner for a multi-model orchestrator. \
FIRST decide whether the request even needs a plan. ONLY if it is conversational (a greeting \
or chit-chat) or a question that just wants a direct text answer with NO file changes, reply \
with exactly: NO_PLAN — and nothing else. For ANY task that creates, edits, runs, fixes, or \
investigates code or files (even a small one), do NOT say NO_PLAN — make a plan. \
Break the task below into a SHORT ordered list of 1–5 concrete sub-tasks. \
Reply with ONLY a JSON array (no prose, no markdown fences) of objects with keys: \
\"title\" (short), \"detail\" (one sentence of what to do), \"kind\" (one of \
\"code\", \"command\", \"research\"), and \"parallel\" (boolean). Set \"parallel\": true \
on consecutive steps that touch DIFFERENT files and do NOT depend on each other's output \
— they will run at the same time, which is faster (e.g. creating two separate modules). \
If a step needs an earlier step's result, or edits the same file, keep it false. When \
truly unsure, leave it false — steps run in order by default. \
Example: [{\"title\":\"Make a.py\",\"detail\":\"Create a.py\",\"kind\":\"code\",\"parallel\":true},{\"title\":\"Make b.py\",\"detail\":\"Create b.py\",\"kind\":\"code\",\"parallel\":true}]";

/// Extract the ordered sub-task list from a planner's reply. Tolerates markdown fences
/// and surrounding prose by scanning for the outermost JSON array. Falls back to a
/// single step (the whole task) when nothing parses, so the Conductor always proceeds.
/// The planner declined to produce a plan (the request is conversational / a single direct
/// answer). True when it emitted the NO_PLAN sentinel and no JSON array of steps.
pub fn no_plan(text: &str) -> bool {
    let t = text.trim();
    t.to_uppercase().contains("NO_PLAN") && !t.contains('[')
}

pub fn parse_plan(text: &str, fallback_task: &str) -> Vec<ConductorStep> {
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if end > start {
            if let Ok(steps) = serde_json::from_str::<Vec<ConductorStep>>(&text[start..=end]) {
                let steps: Vec<ConductorStep> = steps
                    .into_iter()
                    .filter(|s| !s.title.trim().is_empty())
                    .collect();
                if !steps.is_empty() {
                    return steps;
                }
            }
        }
    }
    let title: String = fallback_task.chars().take(80).collect();
    vec![ConductorStep {
        title,
        detail: fallback_task.to_string(),
        kind: "code".into(),
        parallel: false,
    }]
}

/// Pick a CROSS-PROVIDER reviewer for a step that ran on `step_backend` (Polly's
/// cross-vendor review): a Claude step is reviewed by Gemini and vice-versa. Returns
/// (backend_id, model, label).
pub fn reviewer_for(step_backend: &str) -> (String, String, String) {
    if step_backend == "antigravity" {
        ("claude-code".into(), "sonnet".into(), "Sonnet 4.6".into())
    } else {
        // A capable cross-vendor reviewer for Claude's work — Pro, not Flash.
        (
            "antigravity".into(),
            "Gemini 3.1 Pro (High)".into(),
            "Gemini 3.1 Pro".into(),
        )
    }
}

/// Greetings / small-talk / very short non-task input — the Conductor answers these
/// directly instead of running a full plan→execute (so "hello" doesn't trigger
/// "planning with Haiku…"). Conservative: a real task ("build x", "fix y") returns false.
pub fn is_conversational(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    if p.is_empty() {
        return true;
    }
    let bare = p.trim_end_matches(['!', '.', '?', ' ']);
    const GREET: &[&str] = &[
        "hi",
        "hello",
        "hey",
        "yo",
        "sup",
        "hiya",
        "howdy",
        "thanks",
        "thank you",
        "ty",
        "thx",
        "ok",
        "okay",
        "cool",
        "nice",
        "lol",
        "gm",
        "gn",
        "good morning",
        "good evening",
        "good night",
        "how are you",
        "what's up",
        "whats up",
        "who are you",
        "what can you do",
    ];
    if GREET.contains(&bare) {
        return true;
    }
    // Chit-chat / "about you" questions that just want an answer, not a build — match by
    // prefix so variations ("how is your day going?", "how are you doing today") all count.
    const CONVO_PREFIX: &[&str] = &[
        "how are you",
        "how is your day",
        "how's your day",
        "hows your day",
        "how was your day",
        "how's it going",
        "hows it going",
        "how is it going",
        "how have you been",
        "what's up",
        "whats up",
        "who are you",
        "what are you",
        "what can you do",
        "what do you do",
        "tell me about yourself",
        "what do you think",
        "do you like",
        "are you ",
        "what's your",
        "whats your",
        "nice to meet",
        "good to meet",
    ];
    if CONVO_PREFIX.iter().any(|c| bare.starts_with(c)) {
        return true;
    }
    // Very short with no task signal → treat as conversational.
    const TASKY: &[&str] = &[
        "create",
        "build",
        "add",
        "fix",
        "refactor",
        "write",
        "make",
        "implement",
        "run",
        "test",
        "debug",
        "update",
        "delete",
        "remove",
        "change",
        "generate",
        "install",
        "deploy",
        "open",
        "find",
        "explain",
        "review",
        "analyze",
        "optimize",
        "set up",
    ];
    p.split_whitespace().count() <= 2 && !TASKY.iter().any(|t| p.contains(t))
}

/// Whether a finished step warrants cross-review: code/command steps, or any step that
/// produced file changes. Research steps with no edits are skipped (keeps cost down).
pub fn should_review(kind: &str, produced_changes: bool) -> bool {
    kind.eq_ignore_ascii_case("code") || kind.eq_ignore_ascii_case("command") || produced_changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(parallel: bool) -> ConductorStep {
        ConductorStep {
            title: "t".into(),
            detail: String::new(),
            kind: "code".into(),
            parallel,
        }
    }

    #[test]
    fn groups_consecutive_parallel_steps() {
        // [seq, par, par, seq] → [[0],[1,2],[3]]
        let steps = vec![step(false), step(true), step(true), step(false)];
        assert_eq!(parallel_groups(&steps), vec![vec![0], vec![1, 2], vec![3]]);
        // all sequential → singletons
        let seqs = vec![step(false), step(false)];
        assert_eq!(parallel_groups(&seqs), vec![vec![0], vec![1]]);
        // a lone parallel step is just its own (size-1) group → runs sequentially
        assert_eq!(parallel_groups(&[step(true)]), vec![vec![0]]);
    }

    #[test]
    fn parallel_defaults_false_when_missing() {
        let s: ConductorStep = serde_json::from_str(r#"{"title":"x"}"#).unwrap();
        assert!(!s.parallel);
    }

    #[test]
    fn detects_conversational_vs_task() {
        assert!(is_conversational("hello"));
        assert!(is_conversational("Hi!"));
        assert!(is_conversational("thanks"));
        assert!(is_conversational("who are you"));
        // The reported case + variations — chit-chat questions must NOT get planned.
        assert!(is_conversational("How is your day going"));
        assert!(is_conversational("how is your day going?"));
        assert!(is_conversational("how are you doing today?"));
        assert!(is_conversational("how's it going"));
        assert!(is_conversational("what can you do?"));
        assert!(!is_conversational("build me a calculator"));
        assert!(!is_conversational("fix the failing test"));
        assert!(!is_conversational("explain this codebase"));
        assert!(!is_conversational("add a dark mode toggle to settings"));
    }

    #[test]
    fn no_plan_sentinel_detected() {
        assert!(no_plan("NO_PLAN"));
        assert!(no_plan("  no_plan  "));
        assert!(!no_plan("[{\"title\":\"x\"}]"));
        assert!(!no_plan("Here is the plan: [..]"));
    }

    #[test]
    fn reviewer_is_cross_provider() {
        assert_eq!(reviewer_for("claude-code").0, "antigravity");
        assert_eq!(reviewer_for("antigravity").0, "claude-code");
    }

    #[test]
    fn review_gate() {
        assert!(should_review("code", false));
        assert!(should_review("research", true)); // produced changes → review anyway
        assert!(!should_review("research", false));
    }

    #[test]
    fn parses_plain_json_array() {
        let t = r#"[{"title":"A","detail":"do a","kind":"code"},{"title":"B","detail":"do b","kind":"command"}]"#;
        let p = parse_plan(t, "task");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "A");
        assert_eq!(p[1].kind, "command");
    }

    #[test]
    fn tolerates_fences_and_prose() {
        let t =
            "Sure! Here is the plan:\n```json\n[{\"title\":\"Only\",\"detail\":\"d\"}]\n```\nDone.";
        let p = parse_plan(t, "task");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].title, "Only");
        assert_eq!(p[0].kind, ""); // missing kind defaults
    }

    #[test]
    fn falls_back_to_single_step() {
        let p = parse_plan("I cannot produce JSON, sorry.", "refactor the auth module");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].detail, "refactor the auth module");
    }
}
