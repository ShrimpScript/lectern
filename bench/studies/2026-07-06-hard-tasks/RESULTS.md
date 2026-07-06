# Study: harder tasks — free vs subscription agents, with and without Lectern

- **Date:** 2026-07-06
- **Suite:** 6 harder tasks (multi-file pipeline, failing-test bugfix, config
  migration, exact-cents CSV report, API-shim refactor, edge-case word wrap);
  graders validated against reference solutions before any model ran
- **Arms:**
  | arm | what it is | model | reps |
  |---|---|---|---|
  | free single | `lectern run` | deepseek-v4-flash-free | 2 |
  | free conductor | `lectern conduct` | deepseek-v4-flash-free | 2 |
  | raw-claude | `claude -p` directly — **no Lectern** | Claude Code default | 1 |
  | lectern-claude | `lectern run --backend claude-code` | Claude Code default | 1 |
  | conductor-auto | `lectern conduct --backend auto` (routed) | Haiku/Sonnet per step | 1 |
- Subscription CLIs only (no API keys). Grading deterministic, after the agent.

## Headline

| arm | pass | mean tokens | mean wall |
|---|---|---|---|
| free single ×2 | 11/12 | 15,252 | 40.8 s |
| free conductor ×2 | 11/12 | 32,655 (+114%) | 76.7 s |
| **raw-claude (no Lectern)** | **6/6** | **11,557** | **34.4 s** |
| **lectern-claude** | **6/6** | **11,671 (+1.0%)** | **35.1 s** |
| conductor-auto | 6/6 | *(not comparable — see below)* | 113.4 s |

## Finding 1 — Lectern adds ~1% overhead on top of the same agent

The core head-to-head: the same tasks, the same Claude Code subscription, with
and without Lectern in the middle. Success is identical (6/6), token cost is
within +1.0% (per-task deltas −80…+398), wall time within a second. Lectern's
engine layer — workspace indexing, brain recall, session capture, the Apply
pipeline — costs essentially nothing on top of the agent it drives.

## Finding 2 — the Conductor's routing demonstrably fires

`conductor-auto` routed steps to different models by classified complexity —
e.g. hard-pipeline: `parser.py → Haiku ("quick, low-complexity")`,
`validator.py → Haiku`, `pipeline.py → Sonnet ("general coding task")` — two
distinct models inside one task, chosen to spend less on easy steps. All six
tasks passed fully routed. Wall cost of the plan→route→execute→review structure:
~3.2× a single call.

## Finding 3 — orchestration still shows cost, not success gain, at this difficulty

Free single already passes 11/12; free conductor also 11/12 but at +114% tokens
(and its one failure was an over-decomposed wrong answer, costing 2× tokens).
Claude passes 6/6 in every configuration. These tasks — precision-spec,
single-session — are within a strong single call's reach, so planning + review
can only add cost. The Conductor's success case remains unproven here; it needs
task classes where single calls genuinely fail (long-horizon, large-repo,
cross-session work).

## Read before citing

- **Cross-backend token totals are not comparable.** Claude Code reports
  `usage.input_tokens` *excluding* prompt-cache reads; opencode reports fuller
  totals. Within-backend comparisons (raw vs lectern-claude; free single vs free
  conductor) are valid. `conductor-auto`'s 4,425 mean is an artifact of
  cache-served steps — read its cost from wall time, not tokens, until the
  instrumentation absorbs cache fields (tracked follow-up).
- **One free-single run timed out** (hard-migration, 240 s) — free-tier
  flakiness; recorded as a failure.
- **1 repetition on the subscription arms** (bounded deliberately to respect
  plan limits) — directional.
- Claude Code ran with `--dangerously-skip-permissions` in throwaway sandboxes;
  Lectern arms ran `--apply --yolo`. Same autonomy on both sides.

## Reproduce

```sh
python3 bench/runner.py --arms single,conductor --backend opencode \
  --model opencode/deepseek-v4-flash-free --tasks hard-... --repeat 2
python3 bench/runner.py --arms raw-claude --tasks hard-... --repeat 1
python3 bench/runner.py --arms single --backend claude-code --tasks hard-... --repeat 1
python3 bench/runner.py --arms conductor --backend auto --tasks hard-... --repeat 1
```
