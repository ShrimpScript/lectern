//! The normalized agent event stream — the lingua franca every backend maps to,
//! and the thing the UI/CLI render. See Lectern-Brain/03-Architecture/Backend Adapter Layer.md.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The model is reasoning (render as a spinner / dots).
    Thinking,
    /// A short reasoning summary, optionally citing recalled memory.
    Thought {
        summary: String,
        recalls: Vec<String>,
    },
    /// A learned skill was auto-applied to this turn (its conventions/recipe injected).
    SkillApplied { name: String, why: String },
    /// A snapshot of the workspace was captured before this turn wrote to disk, so the
    /// user can rewind here. `id` is the checkpoint id; `label` is the prompt.
    Checkpoint { id: String, label: String },
    /// Auto-routing picked a model for this task because it excels at this kind of work.
    ModelRouted { model: String, reason: String },
    /// A proposed plan of steps.
    Plan { steps: Vec<PlanStep> },
    /// A proposed file edit (NOT yet applied — held behind the Apply gate).
    FileEdit {
        path: String,
        added: u32,
        removed: u32,
        preview: Vec<DiffLine>,
    },
    /// A command the agent ran (in the sandbox) and its output.
    Terminal {
        command: String,
        output: String,
        exit_code: i32,
    },
    /// Assistant prose (a complete block).
    Message { text: String },
    /// A streamed chunk of assistant prose — the UI appends it to the live message so text
    /// types out in real time instead of appearing all at once.
    MessageDelta { text: String },
    /// Running token/cost usage.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    /// The active backend hit a usage/rate limit (triggers fallback).
    LimitHit { reason: String },
    /// A terminal error.
    Error { message: String },
    /// The turn is complete.
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Add,
    Remove,
    Context,
}
