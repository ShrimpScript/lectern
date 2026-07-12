# Brain isolation — persistent memory changes the outcome (2026-07-12, Claude/Sonnet)

**Question.** Does Lectern's persistent brain actually make the *same* model succeed
where it otherwise fails? Not "is Lectern low-overhead" (the neutral suite answered
that — yes), but "does the memory change the answer?"

**Design.** Convention tasks (`bench/tasks-convention`): the grader requires the
project's arbitrary error codes (`ACCT-4417`, `CFG-118`, …) that appear *only* in a
seeded skill — never in the prompt, never in the workspace, unguessable. Same model
(Sonnet) across three conditions; the only variable is whether the brain supplies the
convention.

## Result

| condition | error-catalog | validate-result | total |
|---|---|---|---|
| **bare** (`raw-claude`, no Lectern) | 0/2 | 0/2 | **0/4** |
| **Lectern, brain ON** (`single`) | 2/2 | 2/2 | **4/4** |
| **Lectern, brain ON** (`conductor`) | 2/2 | 2/2 | **4/4** |
| **Lectern, brain OFF** (`single`) | 0/2 | 0/2 | **0/4** |
| **Lectern, brain OFF** (`conductor`) | 0/2 | 0/2 | **0/4** |

Zero timeouts. brain-on `recalls` = 2–3 (skill injected); brain-off `recalls` = 0.

**Same model, same tasks: brain-on 8/8, brain-off 0/8, bare 0/4.** The only thing that
changes the outcome is whether Lectern's memory carries the project convention. Bare
Claude and brain-off both default to generic `ValueError` and miss the project's
`PolicyError` codes; brain-on applies the exact skill-supplied codes and passes. This
is the retrieval-augmentation result, cleanly isolated: the knowledge is the variable,
the model is fixed.

## What it took to get a valid control (and the bug it found)
Two earlier iterations were confounded, and fixing each was the point of the exercise:

1. **Guessable codes.** v1 used `INSUFFICIENT_FUNDS` etc. — Sonnet guesses those, so
   brain-off passed by guessing. Fixed: arbitrary codes (`ACCT-4417`) nothing can guess.
2. **A real Lectern bug — an incomplete kill-switch.** Even with arbitrary codes,
   brain-off still passed. Cause: `LECTERN_NO_BRAIN` disabled recall and skill
   *matching*, but `sync_skills_to_claude` still wrote every learned skill into the
   workspace's `.claude/skills/`, which Claude Code reads natively — so the skill
   leaked to the agent through the filesystem. The benchmark's control caught it.
   Fixed by gating materialization on `brain_disabled()` (the kill-switch now clears
   stale skills and syncs nothing when off). This table is from the fixed build.

## Honest scope
- N is small: 2 conventions × 2 reps × 2 arms. The effect is categorical (8/0), not
  marginal, so it reads clearly, but breadth (more conventions, more domains) is the
  next step.
- This measures **persistent-memory value**, a specific product capability — not a
  general "Lectern is smarter" claim. Report it beside the neutral suite (where the
  brain is correctly neutral) so the claim stays narrow and true.
- A separate, also-real finding from the same runs: **bare Claude Code fails these
  convention tasks 0/4 while Lectern-driven Claude passes** — the harness surfaces the
  project's own error type where the bare CLI defaults to generic idioms. That holds
  independent of the brain (it was true brain-off before the leak fix); with the fix,
  the convention codes specifically require the brain.

Raw data: `brain-on/`, `brain-off/` (`summary.{json,md}`, `rows.jsonl`).
