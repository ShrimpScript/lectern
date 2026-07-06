# Study: single model vs the Conductor, on free models

- **Date:** 2026-07-06
- **Model:** `opencode/deepseek-v4-flash-free` (opencode free tier, $0)
- **Suite:** 8 custom tasks (feature, refactor, bugfix, cross-file), 2 repetitions each
- **Arms:** `single` (`lectern run`) vs `conductor` (`lectern conduct`), same model, brain on
- **Grading:** deterministic (each task's grader must exit 0), run after the agent
- Reproduce: `python3 bench/runner.py --backend opencode --model opencode/deepseek-v4-flash-free --arms single,conductor --repeat 2`

## Headline

| arm | runs | pass rate | mean tokens | mean wall |
|---|---|---|---|---|
| single | 16 | **16/16 (100%)** | 13,993 | 7.4 s |
| conductor | 16 | **16/16 (100%)** | 18,524 (**+32%**) | 26.2 s (~3.5×) |

## Per-task token cost

| task | steps in plan | single | conductor | overhead |
|---|---|---|---|---|
| temp-convert | 1 | 13,782 | 13,761 | 0% |
| json-config | 1 | 13,891 | 13,835 | 0% |
| dedup-list | 1 | 13,796 | 13,887 | 1% |
| fizzbuzz | 1 | 13,830 | 13,951 | 1% |
| stack-class | 1 | 13,896 | 14,152 | 2% |
| refactor-counter | 2 | 14,194 | 21,252 | 50% |
| fix-off-by-one | 2 | 14,220 | 21,622 | 52% |
| cross-file-slug | 2 | 14,336 | 35,732 | 149% |

Split by plan size: **single-step tasks +1%**, **multi-step tasks +84%**.

## What this shows

- **On tasks a single call already solves, the Conductor adds no success and real cost.**
  Both arms pass everything; the Conductor spends ~32% more tokens and ~3.5× the time
  because it plans, executes each step, and reviews.
- **The overhead is proportional to how much the Conductor decomposes.** Single-step
  tasks are ~free (it plans one step and proceeds); multi-step tasks (refactor, bugfix,
  cross-file) cost 50-150% more, since each step is executed and then reviewed.
- **The instrumentation is honest and machine-readable.** Every number here comes from
  `lectern --metrics-out`, not a claim.

## What this does NOT show (read before citing)

- **It does not demonstrate a Conductor success advantage.** These tasks are within a
  single call's reach for this model, so there is no headroom for planning + review to
  win. Showing the Conductor's benefit needs tasks where a single shot *fails* - harder
  multi-step work, or a stronger backend where the failure mode is subtler. That is the
  next phase (and, for cross-vendor routing/review, the paid phase).
- **`review_steps` under-reports.** The Conductor's cross-review step does run on
  file-modifying tasks (observed directly in run traces, and reflected in the higher
  token cost of the 2-step tasks), but the review step does not emit a routing event, so
  the automatic `review_steps` counter reads 0. Fixing that counter is a tracked follow-up.
- **Model labels are the classifier's intent, not the executing model.** With a pinned
  `--model`, every step runs on the free model regardless of the "Sonnet 4.6" routing
  label the classifier prints, so `distinct_models` here reflects labels, not real
  distinct models.
- **`tool_calls`/`changes` are 0 for opencode.** opencode edits files in place rather
  than through Lectern's change pipeline, so those counters do not populate for this
  backend. Token count and grader success are accurate.
- **Small N and weak model.** 2 reps on a free model - directional, not definitive.

See `bench/METHODOLOGY.md` for the full method and fairness controls.
