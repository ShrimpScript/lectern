# Lectern benchmark

A transparent, reproducible harness for measuring Lectern as an *orchestrator* —
not raw model IQ, but **execution, trajectory, and resource management**: does a
task succeed, how many tokens and tool calls did it take, and does the Conductor
(multi-model routing + cross-review) and the persistent brain actually earn their
overhead versus a single model?

## What it measures

Per run, `lectern --metrics-out` emits a machine-readable report; the runner adds
a deterministic grade:

| metric | meaning |
|---|---|
| `passed` | deterministic grader exit 0 (the task's real success criterion) |
| `total_tokens` | context overhead — the SaaS cost axis |
| `tool_calls` / `plan_steps` | trajectory efficiency |
| `distinct_models` / `review_steps` | did multi-model routing + cross-review actually fire |
| `recalls` | brain signals (memory recalls + applied skills) used |
| `wall_s` | wall-clock time |

## Arms

- **single** — one model, one shot: `lectern run --backend <b> --model <m>`
- **conductor** — plan → routed fan-out → cross-review: `lectern conduct`
- **brain on/off** — same task with the persistent brain enabled vs `LECTERN_NO_BRAIN=1`,
  to measure the self-learning delta (run 1 vs a similar run 2).

## Fairness controls

Every run gets a **fresh, identical workspace** seeded from the task's `setup/`
and a single git commit; every arm runs the **same prompt, timeout, and
apply/yolo settings**; grading is a **deterministic command run after the agent**
(never the agent grading itself). See `METHODOLOGY.md`.

## Run it

```sh
cargo build -p lectern                      # build the CLI the runner drives
python3 bench/runner.py --backend mock      # smoke-test the harness ($0, instant)

# the real $0 study (free models via opencode — see METHODOLOGY.md):
python3 bench/runner.py --arms single,conductor --backend opencode \
  --model opencode/deepseek-v4-flash-free --repeat 3
```

Isolation is a fresh temp git repo per run (stdlib only, nothing to install).
`Dockerfile` provides a fully pinned environment for external reproducibility.

## Tasks

`tasks/<id>/task.json` (`prompt` + deterministic `grade` command) plus a `setup/`
folder that becomes the initial workspace. Tasks are chosen to exercise Lectern's
claimed differentiators: multi-step refactors, cross-file changes that reward the
code-graph brain, and error recovery.
