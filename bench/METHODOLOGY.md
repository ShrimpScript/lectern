# Methodology

This benchmark measures Lectern as an *orchestrator*, not as a model. Traditional
LLM benchmarks (MMLU, HumanEval) score raw model intelligence. An orchestrator is
different: the model is a given, and what varies is how the system plans, routes,
remembers, and spends tokens to get a task done. So the questions here are:

1. Does a task actually reach its goal state? (binary success)
2. How many tokens did that cost? (context overhead - the real SaaS cost axis)
3. How efficient was the path? (tool calls, plan steps, wall time)
4. Does the Conductor's structure (plan then execute then review) earn its overhead
   versus a single model call?
5. Does the persistent brain (memory recall + learned skills) make a measurable
   difference versus running with no memory?

## Arms

- **single** - one model, one call: `lectern run --backend <b> --model <m>`.
- **conductor** - Lectern plans the task, executes the steps, and self-reviews:
  `lectern conduct --backend <b> --model <m>`.
- **brain on / brain off** - the same arm with the persistent brain enabled versus
  `LECTERN_NO_BRAIN=1`, which skips memory recall and learned-skill matching. This
  isolates the brain's contribution.

### Bare-agent baselines (the scaffolding delta)

Lectern *is* scaffolding: harness choices are known to swing agent scores by 10-20
points on the same model. So the load-bearing comparison is the same agent, same
model, **bare vs through Lectern** - what Lectern's context, brain, and orchestration
add on top. Each bare arm drives the underlying CLI directly, with no Lectern:

- **raw-claude** - `claude -p` in `stream-json` mode. Tool calls are counted for
  real (one per assistant `tool_use` block); tokens, turns, and `cost_usd` come from
  the terminal `result` event.
- **raw-opencode** - `opencode run --format json`. Tokens are summed from
  `step_finish` parts (same source the engine reads), tool events counted directly.
- **raw-agy** - `agy -p` autonomous. Antigravity's CLI reports no usage, so this arm
  contributes pass/fail and wall time only.

All three use subscription auth, not API keys. `raw-opencode` on a free model is $0;
`raw-claude` and `raw-agy` spend subscription usage and are budget-gated.

## Metrics

Each run emits a machine-readable report via `lectern --metrics-out`; the runner
adds the grade:

| field | meaning |
|---|---|
| `passed` | the task's deterministic grader exited 0 |
| `total_tokens` | input + output tokens the run reported |
| `tool_calls`, `plan_steps` | trajectory shape |
| `num_turns` | conversation turns (bare arms) - distinct from `tool_calls` |
| `cost_usd` | dollar cost the CLI reported - the one cross-vendor-comparable figure |
| `distinct_models`, `review_steps` | whether multi-model routing / cross-review fired |
| `recalls` | brain signals used (memory recalls + applied skills) |
| `wall_s` | wall-clock seconds |

Token accounting is *not* comparable across vendors (cache handling differs; agy
reports nothing), so `mean_tokens` is only meaningful within one backend. For
cross-vendor comparison use `mean_cost_usd`, which the summary reports per arm.

## Fairness controls

- **Identical fresh workspace per run.** Every run starts from the task's `setup/`
  copied into a new temporary directory and committed once with git. State never
  leaks between runs.
- **Same everything but the arm.** The prompt, timeout, and apply/yolo settings are
  identical across arms; only the arm (run vs conduct, backend, brain) changes.
- **Grading is external and deterministic.** After the agent finishes, a fixed
  command runs in the workspace; exit 0 is a pass. The agent never grades itself.
- **Isolation.** The default runner uses a fresh temp git repo per run. `Dockerfile`
  provides a pinned container image for fully reproducible external runs.

## Cost stance

Runs use **free models only** (opencode's built-in free tier), so a full study
costs nothing and burns no paid tokens. The model used for the published study is
recorded in each study's `summary.json`.

## Honest limitations

These matter for reading the results, and are stated plainly rather than buried:

- **Free models are weak.** Absolute pass rates are low compared to frontier models.
  The signal is the *difference between arms on the same tasks and model*, not the
  absolute score. Do not read these as Lectern's ceiling.
- **Lectern-driven opencode under-reports tool activity.** The bare `raw-opencode`
  arm counts tool calls and file edits fine (its stream carries tool events). But
  when Lectern drives opencode, the engine's stream reader only handles text and
  step events, dropping the tool events - so `tool_calls`/`changes` read 0 for the
  Lectern opencode arms while `raw-opencode` shows the real count. That is an engine
  reader gap, tracked and being fixed, not an opencode limitation (token count and
  grader success are unaffected either way).
- **`review_steps` under-reports.** The Conductor's cross-review step does run on
  file-modifying tasks (bugfix, refactor, cross-file - observed in run traces and
  reflected in those tasks' higher token cost), but the review step does not emit a
  routing event, so the automatic `review_steps` counter reads 0. Fixing that counter
  is a tracked follow-up; until then, read review cost from the token delta, not the
  counter.
- **Routing labels are classifier intent, not the executing model.** With a pinned
  `--model`, every step runs on that model regardless of the model label the classifier
  prints, so `distinct_models` reflects labels rather than truly distinct models.
- **Cross-vendor routing and review are not exercised at $0.** Routing sub-tasks to
  *different vendors* and having them review each other requires the paid backends
  (Claude Code, Antigravity). That is a separate, budget-gated phase, documented but
  not run in this free-model study. The Conductor arm here runs its plan/execute/
  self-review structure on a single free model.
- **Small N.** The published study uses a handful of repetitions per (arm, task).
  Treat the numbers as directional, not statistically definitive.

## External anchor: SWE-bench

The custom suite is tuned to what an orchestrator should be good at (multi-step and
cross-file work), but it is our own. For an external, comparable reference we use
**SWE-bench Verified**: real GitHub issues graded by whether the produced patch
passes the repo's own unit tests. A SWE-bench adapter plugs a task's setup and
grader into the same runner. Free models score near zero on real SWE-bench, so that
run is done separately with a stronger backend rather than in this free-model study.

## Reproduce

```sh
cargo build -p lectern
python3 bench/runner.py --backend mock                 # harness smoke test
python3 bench/runner.py --backend opencode \
  --model opencode/deepseek-v4-flash-free \
  --arms single,conductor,raw-opencode --repeat 2       # the $0 scaffolding delta
LECTERN_NO_BRAIN=1 python3 bench/runner.py ...          # the brain-off control

# Budget-gated (spends subscription usage): bare vs Lectern on a strong model.
python3 bench/runner.py --backend claude --model <strong> \
  --arms raw-claude,single,conductor --repeat 3
python3 bench/runner.py --backend antigravity --arms raw-agy,single --repeat 3
```
