# Run sandbox (bubblewrap) — design

Status: design (mission slice S1). No code yet; this pins the sandbox model and the seam the
implementation slices build against.

## Why

When you let an agent run with `--apply` (edits land) or `--yolo` (also runs commands), an
**indirect prompt injection** — a malicious instruction hidden in a repo file, a dependency's
code comment, an MCP tool result, or an installed skill — can turn into real damage: files
written outside the repo, secrets read and exfiltrated, a branch pushed. The 2026 guidance is
"constrain the consequences": run the agent so that an instruction which *does* land becomes a
**bad transcript**, not an escape.

Lectern is local-first and Linux-native, so the right tool is **bubblewrap** (`bwrap`) — the
unprivileged, no-daemon sandbox that Flatpak uses — not Docker. It's already installed here
(`bubblewrap 0.9.0`). The sandbox is **opt-in** and targets the higher-risk `--apply`/`--yolo`
runs; default behaviour is byte-for-byte unchanged.

## What the sandbox guarantees (and what it doesn't)

**Filesystem confinement (the primary deliverable):** the agent's backend process sees the host
filesystem **read-only**, except the workspace, which is bound **read-write**, plus a private
`/tmp`. So an injected write outside the repo fails; edits to the repo still work.

**Network isolation (a secondary, clearly-caveated opt-in):** `--unshare-net` gives the process a
fresh network namespace with no connectivity — which cuts **all** network, including `localhost`.
That breaks every model backend's API call (cloud *and* Ollama-over-loopback), so it is **not** the
default. It's a niche opt-in for runs that must touch no network at all (file-only tooling, replay,
paranoid review). The everyday value is filesystem confinement, which works with every backend.

Non-goals: this is not a security boundary against a kernel exploit, and it does not stop the agent
from doing damage *inside* the workspace (that's what checkpoints/rewind + the apply gate are for).
It shrinks the blast radius of an escaped instruction to "the repo, offline-optional".

## The `bwrap` command model

The sandbox wraps the backend's own command. Bind the workspace at the **same path** on both sides
so no path translation is needed (`--add-dir <ws>`, `--chdir`, and the agent's own file paths all
stay valid):

```
bwrap \
  --die-with-parent \
  --unshare-user --unshare-ipc --unshare-pid --unshare-uts --unshare-cgroup \
  # (NO --unshare-net by default — cloud/Ollama model APIs need the network) \
  --ro-bind /usr /usr \
  --ro-bind-try /bin /bin  --ro-bind-try /sbin /sbin \
  --ro-bind-try /lib /lib  --ro-bind-try /lib64 /lib64  --ro-bind-try /lib32 /lib32 \
  --ro-bind-try /etc /etc \
  --proc /proc  --dev /dev  --tmpfs /tmp \
  --bind <workspace> <workspace> \
  # provider auth/config, read-only (see below) \
  --ro-bind-try <resolved-bin-dir> <resolved-bin-dir> \
  --ro-bind-try $HOME/.claude $HOME/.claude \
  --ro-bind-try $HOME/.config $HOME/.config \
  --ro-bind-try $HOME/.local/share $HOME/.local/share \
  --setenv HOME <home> \
  --chdir <workspace> \
  -- <bin> <original args…>
```

Notes:
- `--ro-bind-try` (not `--ro-bind`) tolerates paths that don't exist on a given box.
- Env is **inherited** (bwrap does not clear it), so provider tokens in the environment still reach
  the CLI. We do not `--clearenv`; confinement here is filesystem/network, not env.
- On non-Linux, or when `bwrap` is absent, the sandbox is simply not applied — see the graceful
  path below.

### Provider auth under the sandbox

The provider CLIs authenticate from config in `$HOME` and from their install location:
- **Claude Code** — `~/.claude` (login/session), plus the resolved `claude` binary's directory
  (npm global, nvm, or homebrew — `resolve_claude` already returns an absolute path, so bind its
  parent read-only).
- **Antigravity / OpenCode** — their config under `~/.config` / `~/.local/share`, and their
  binaries' directories.

S4 verifies per backend that an authenticated run still works with these read-only binds. If an
authed run can't be made to work under the sandbox for a given backend, that's a documented
limitation, not a silent breakage.

## The seam in the engine

Every backend builds its command the same way in `crates/engine/src/backend.rs`:

```rust
let mut cmd = Command::new(&bin);
cmd.arg(..).arg(..).current_dir(ctx.workspace_root).stdin(..).stdout(..).stderr(..);
// … more args …
let child = cmd.spawn()?;
```

(Claude ~L438/spawn L491, Antigravity ~L1170/L1202, OpenCode ~L1320/L1344.)

**Minimal wrap:** replace `Command::new(&bin)` with a shared helper

```rust
let mut cmd = crate::sandbox::command(&bin, ctx.workspace_root, &policy);
```

which returns either `Command::new(bin)` (no sandbox) or
`Command::new("bwrap").args(bwrap_flags).arg("--").arg(bin)` (sandboxed). Because `bwrap … -- bin`
puts `bin` last, **every existing `.arg(x)` after it is appended as the sandboxed program's
argument, unchanged** — so the three call sites change by one line each, and all the per-backend
args, `current_dir`, stdio, cancellation, and `scrub_appimage_env` keep working (they apply to the
`bwrap` process, which forwards them). `--chdir <ws>` inside the sandbox mirrors the existing
`current_dir(ws)`.

## Opt-in surface (off by default)

- **`LECTERN_SANDBOX=1`** — enable filesystem confinement for runs. Read in the engine at spawn
  time, so it works uniformly for the CLI, the daemon, and the desktop without threading a flag
  through every layer.
- **`LECTERN_SANDBOX_NET=off`** — additionally add `--unshare-net` (fully offline; see the caveat).
- **`lectern run --sandbox`** — CLI sugar that sets the above for that run.
- Unset (default) → no `bwrap`, identical behaviour to today.

## Graceful degradation

If the sandbox is requested but `bwrap` is not available (not installed, or non-Linux), a run
**fails with a clear, actionable message** ("sandbox requested but bubblewrap isn't installed —
`sudo apt install bubblewrap`, or unset LECTERN_SANDBOX") rather than silently running unconfined.
`sandbox::available()` probes `bwrap --version` once. The default (no sandbox requested) never
touches `bwrap`.

## Testing without a live backend

The sandbox is verified with **synthetic commands** under the real `bwrap` — never a live agent
(no tokens):
- argv builder: unit-test that `wrap` produces the expected flags (workspace bound rw, roots ro,
  net flag present only when asked).
- confinement: run `bwrap … -- /bin/sh -c 'touch <ws>/ok; touch /etc/nope'` and assert the first
  succeeds and the second is denied.
- net isolation: `bwrap --unshare-net … -- /bin/sh -c 'curl -sm2 …'` fails to connect.

## Slice map (mirrors MISSION.md)

S2 `sandbox` module + argv builder + `available()` · S3 wire filesystem confinement into the spawn
behind the opt-in · S4 network isolation + provider-config binds + graceful fallback · S5 surface
(`doctor`/run output) + README/CHANGELOG + audit.

## Sources

- bubblewrap — https://github.com/containers/bubblewrap
- AI agent sandboxing (2026) — https://amux.io/guides/ai-agent-sandboxing/
- Sandboxes for coding agents — https://www.penligent.ai/hackinglabs/sandboxes-for-coding-agents/
