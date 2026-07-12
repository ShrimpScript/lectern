# Convention suite — measuring persistent memory honestly

These tasks isolate one thing: **Lectern's brain carries project knowledge from past
sessions that a stateless bare agent doesn't have.** Each task's correctness depends
on a project convention (exact error-catalog codes, a required return shape) that the
prompt never states. It lives only in a seeded skill — "what the team taught Lectern
last week."

Three ways to run the same model:
- **bare** (`raw-opencode` / `raw-claude`): prompt + workspace only → uses a generic
  approach → fails the convention-specific grader check.
- **Lectern brain-on** (`single` / `conductor`): recalls the seeded skill → follows
  the convention → passes.
- **Lectern brain-off** (`LECTERN_NO_BRAIN=1`): has no access to the skill → fails
  like bare. This is the clean control: the only variable is the brain.

## Why this is a fair test, not a rigged one
- The prompt is identical across arms and never contains the convention. This is the
  retrieval-augmentation experiment: one arm has the relevant knowledge, the others
  don't, model held constant.
- The workspace ships the *mechanism* (a shared `PolicyError` / `Result` type) but not
  the *policy* (which codes, when). Reading the files isn't enough; the catalog
  strings the grader checks are brain-only.
- **Always report next to the neutral suite** (`bench/tasks`, no seeded convention) to
  show the brain is neutral where no convention applies — never present a convention
  win as general superiority. The claim is narrow and true: *persistent memory pays
  off on convention-dependent work.*

## Running
The runner seeds each task's `skill` into an isolated brain (a temp `HOME` with a
fresh `~/.lectern`; backend auth symlinked in; the real brain never touched):

    python3 bench/runner.py --task-dir bench/tasks-convention \
      --backend opencode --model <model> \
      --arms raw-opencode,single,conductor

Then the brain-off control:

    python3 bench/runner.py --task-dir bench/tasks-convention \
      --backend opencode --model <model> --arms single,conductor --brain off

Note: a weak/free model may fail to apply the convention even when given it, and a
throttled free tier produces timeouts that confound pass rates. Run this on a capable
model (or an un-throttled tier) for a clean signal.
