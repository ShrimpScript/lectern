# Scaffolding delta — same model, bare vs Lectern (2026-07-11, $0 free tier)

**Question.** Lectern is a scaffolding layer. Does wrapping the *same* model in
Lectern change the outcome versus that model bare? Model held constant
(`opencode/deepseek-v4-flash-free`); the only variable is the arm.

**Arms.** `raw-opencode` (bare CLI, no Lectern) · `single` (Lectern one call) ·
`conductor` (Lectern plan/execute/self-review). 14 neutral tasks × 2 reps. A second
pass repeated `single`/`conductor` with `LECTERN_NO_BRAIN=1` (brain-off control).

## Result 1 — the neutral suite has no headroom (the headline)
Across **110 completed runs, zero produced a wrong answer.** Every run that finished
passed its grader. The free model already solves these 14 tasks; wrapping it in
Lectern cannot raise a success rate that is already 100%.

| arm | pass rate (completed) | mean tokens | vs bare |
|---|---|---|---|
| raw-opencode (bare) | 28/28 | 13,608 | — |
| single (Lectern) | 28/28 | 14,729 | **+8.2% tokens** |
| conductor (Lectern) | 28/28 | 27,176 | **+100% tokens** |

So on this suite Lectern is *free-ish* (single) to *2× the tokens* (conductor) and
buys nothing — because there is nothing to buy. This is not a Lectern failure; it is a
**benchmark limitation**: these tasks can't show a quality/success delta. Confirmed
empirically what was suspected from design review.

## Result 2 — the brain on/off comparison is confounded (discard it)
The raw summary looks dramatic — brain-off "pass rate" drops to ~0.5. It is an
artifact. **Every brain-off failure was a 150 s timeout (exit 124); zero were wrong
answers.** The brain-off pass ran second and hit free-tier rate-limiting: e.g.
`temp-convert` timed out at 150 s in brain-off but ran in **4.1 s** in brain-on — a
30–40× slowdown, i.e. throttling, not the brain.

Lesson for methodology: sequential phases on a shared free tier are unreliable.
Interleave arms, add rate-limit backoff, or use a paid tier. The brain's value cannot
be read from these tasks anyway (no headroom) — that is what the **convention suite**
(`bench/tasks-convention`) is for, where the difference is *correctness*, not latency.

## Instrumentation notes (this study's runner)
- `raw-opencode` tool-call counts are all tool invocations (read/edit/bash); Lectern's
  `tool_calls` counts shell commands only — not cross-comparable (see METHODOLOGY).
  Compare arms on pass rate, tokens within a backend, and wall time.
- `cost_usd` is null here (free tier reports no cost).

## What this changes
1. **Publish nothing as a "quality win" from the neutral suite** — it can't show one.
   The website's benchmark page should say Lectern is *low-overhead* here, not
   *better*, and point at the headroom experiments for the quality claim.
2. **Run the brain test on the convention suite** (built alongside this study) on a
   capable model — that is where brain-on should separate from bare/brain-off by
   correctness.
3. **Get headroom** for the quality claim: SWE-bench Verified subset + calibrated-hard
   tasks on a strong model (paid phase). Free model + hard tasks + throttling = mush.

Raw data: `brain-on/` and `brain-off/` (`summary.{json,md}`, `rows.jsonl`).
