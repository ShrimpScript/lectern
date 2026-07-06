#!/usr/bin/env python3
"""Lectern benchmark runner.

Runs each benchmark task under one or more "arms" (single model vs the
Conductor, brain on vs off), captures the machine-readable run report emitted by
`lectern --metrics-out`, grades the result deterministically, and writes a
results table.

Fairness controls (see METHODOLOGY.md):
  * every run gets a fresh, identical workspace (isolated temp dir or Docker
    container), seeded from the task's setup/ and a single git commit;
  * every arm runs the same prompt with the same timeout and the same apply/yolo
    settings; the only variable is the arm (run vs conduct, backend, brain);
  * grading is a deterministic command (exit 0 = pass) run in the workspace
    after the agent, never the agent grading itself.

Standard library only — no third-party deps, nothing to install.
"""
import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

BENCH = Path(__file__).resolve().parent
REPO = BENCH.parent
DEFAULT_LECTERN = os.environ.get("LECTERN_BIN", str((REPO / "target/debug/lectern").resolve()))

# Each arm maps to a CLI invocation. `{backend}` and `{model}` are filled per run.
# `--apply --yolo` so the agent actually writes changes the grader can check.
ARMS = {
    "single": ["run", "--backend", "{backend}", "--model", "{model}",
               "--apply", "--yolo", "--fast"],
    "conductor": ["conduct", "--backend", "{backend}", "--model", "{model}",
                  "--apply", "--yolo"],
}


def load_tasks(only_ids):
    tasks = []
    tdir = BENCH / "tasks"
    if not tdir.exists():
        return tasks
    for d in sorted(tdir.iterdir()):
        meta = d / "task.json"
        if not meta.exists():
            continue
        t = json.loads(meta.read_text())
        t["_dir"] = d
        if only_ids and t["id"] not in only_ids:
            continue
        tasks.append(t)
    return tasks


def seed_workspace(task, ws):
    """Copy the task's setup/ into a fresh workspace and make it a git repo."""
    setup = task["_dir"] / "setup"
    if setup.exists():
        shutil.copytree(setup, ws, dirs_exist_ok=True)
    subprocess.run(["git", "init", "-q"], cwd=ws, check=True)
    subprocess.run(["git", "add", "-A"], cwd=ws, check=True)
    subprocess.run(
        ["git", "-c", "user.email=bench@lectern", "-c", "user.name=bench",
         "commit", "-qm", "seed", "--allow-empty"],
        cwd=ws, check=True,
    )


def run_arm(lectern, task, arm, backend, model, ws, metrics_path, env, timeout):
    """Invoke the Lectern CLI for one arm; return (exit_code, wall_seconds, stderr_tail)."""
    tmpl = ARMS[arm]
    argv = [lectern]
    i = 0
    while i < len(tmpl):
        a = tmpl[i]
        if a == "--model" and not model:  # drop --model when no model given
            i += 2
            continue
        argv.append(a.format(backend=backend, model=model))
        i += 1
    argv += [task["prompt"], "--metrics-out", str(metrics_path)]
    t0 = time.time()
    try:
        p = subprocess.run(argv, cwd=ws, capture_output=True, text=True,
                           timeout=timeout, env=env)
        rc, err = p.returncode, p.stderr[-500:]
    except subprocess.TimeoutExpired:
        rc, err = 124, "TIMEOUT"
    return rc, round(time.time() - t0, 2), err


def grade(task, ws, timeout):
    """Run the task's deterministic grader in the workspace. True=pass, None=no grader."""
    cmd = task.get("grade")
    if not cmd:
        return None
    try:
        p = subprocess.run(cmd, shell=True, cwd=ws, capture_output=True,
                           text=True, timeout=timeout)
        return p.returncode == 0
    except subprocess.TimeoutExpired:
        return False


def main():
    ap = argparse.ArgumentParser(description="Lectern benchmark runner")
    ap.add_argument("--arms", default="single,conductor",
                    help="comma list: " + ",".join(ARMS))
    ap.add_argument("--backend", default="mock",
                    help="backend passed to the CLI (mock | opencode | auto | ...)")
    ap.add_argument("--model", default="", help="model id for the single arm")
    ap.add_argument("--tasks", default="", help="comma list of task ids (default: all)")
    ap.add_argument("--repeat", type=int, default=1, help="runs per (arm,task)")
    ap.add_argument("--timeout", type=int, default=300, help="per-run seconds")
    ap.add_argument("--grade-timeout", type=int, default=120)
    ap.add_argument("--brain", default="on", choices=["on", "off"],
                    help="off sets LECTERN_NO_BRAIN=1 to disable recall/skills")
    ap.add_argument("--lectern", default=DEFAULT_LECTERN)
    ap.add_argument("--out", default="", help="results dir (default: bench/results/<ts>)")
    args = ap.parse_args()

    arms = [a.strip() for a in args.arms.split(",") if a.strip()]
    only = {t.strip() for t in args.tasks.split(",") if t.strip()}
    tasks = load_tasks(only)
    if not tasks:
        raise SystemExit("no tasks found under bench/tasks/")
    if not Path(args.lectern).exists():
        raise SystemExit(f"lectern binary not found: {args.lectern} "
                         "(build with `cargo build -p lectern` or set LECTERN_BIN)")

    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    outdir = Path(args.out) if args.out else BENCH / "results" / ts
    outdir.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    if args.brain == "off":
        env["LECTERN_NO_BRAIN"] = "1"

    rows = []
    for task in tasks:
        for arm in arms:
            for rep in range(args.repeat):
                with tempfile.TemporaryDirectory(prefix="lectern-bench-") as ws:
                    ws = Path(ws)
                    seed_workspace(task, ws)
                    mfile = outdir / f"{task['id']}__{arm}__r{rep}.metrics.json"
                    rc, wall, err = run_arm(args.lectern, task, arm, args.backend,
                                            args.model, ws, mfile, env, args.timeout)
                    passed = grade(task, ws, args.grade_timeout)
                    metrics = json.loads(mfile.read_text()) if mfile.exists() else {}
                    row = {
                        "task": task["id"], "category": task.get("category", ""),
                        "arm": arm, "rep": rep, "backend": args.backend,
                        "brain": args.brain, "exit_code": rc, "wall_s": wall,
                        "passed": passed,
                        "total_tokens": metrics.get("total_tokens"),
                        "tool_calls": metrics.get("tool_calls"),
                        "plan_steps": metrics.get("plan_steps"),
                        "distinct_models": metrics.get("distinct_models"),
                        "review_steps": metrics.get("review_steps"),
                        "recalls": metrics.get("recalls"),
                        "changes": metrics.get("changes"),
                        "stderr": err if rc not in (0,) else "",
                    }
                    rows.append(row)
                    mark = {True: "PASS", False: "FAIL", None: "----"}[passed]
                    print(f"  [{mark}] {task['id']:<22} {arm:<10} "
                          f"tok={row['total_tokens']} tools={row['tool_calls']} "
                          f"{wall}s")

    (outdir / "rows.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n")
    write_summary(outdir, rows)
    print(f"\nresults → {outdir}")


def write_summary(outdir, rows):
    """Aggregate per-arm success rate + mean tokens/tool-calls into summary.{json,md}."""
    arms = sorted({r["arm"] for r in rows})
    agg = {}
    for arm in arms:
        rs = [r for r in rows if r["arm"] == arm]
        graded = [r for r in rs if r["passed"] is not None]
        passes = [r for r in graded if r["passed"]]
        toks = [r["total_tokens"] for r in rs if r["total_tokens"] is not None]
        tools = [r["tool_calls"] for r in rs if r["tool_calls"] is not None]
        agg[arm] = {
            "runs": len(rs),
            "graded": len(graded),
            "pass_rate": round(len(passes) / len(graded), 3) if graded else None,
            "mean_tokens": round(sum(toks) / len(toks)) if toks else None,
            "mean_tool_calls": round(sum(tools) / len(tools), 2) if tools else None,
        }
    (outdir / "summary.json").write_text(json.dumps(agg, indent=2))
    lines = ["# Benchmark summary", "",
             "| arm | runs | pass rate | mean tokens | mean tool calls |",
             "|---|---|---|---|---|"]
    for arm, a in agg.items():
        lines.append(f"| {arm} | {a['runs']} | {a['pass_rate']} | "
                     f"{a['mean_tokens']} | {a['mean_tool_calls']} |")
    (outdir / "summary.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
