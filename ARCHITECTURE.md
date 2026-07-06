# Lectern — Architecture

Lectern is the **engine** that wraps the AI coding agent you already pay for
(Claude Code, Antigravity, or any API key) and adds persistent **memory**, learned
**skills**, and **adaptive context**, with multi-backend fallback — **local-first**
and **Linux-native**. The cloud (the Lectern site's API) is optional and content-blind.

## Components
```
crates/
  engine/    The "V8". Workspaces, sessions, the normalized AgentEvent stream,
             backend adapters, the SQLite store (memory + skills + vectors),
             hybrid recall, the Apply gate, and the cloud client.
  lectern/   The `lectern` CLI — the primary client (open/run/context/skills/…).
  lecternd/  The engine daemon — local socket (JSON-RPC) + the scheduler.
apps/
  (web moved to ShrimpScript/lectern-web — site + web dashboard (the cloud
             control plane: auth, billing, usage, device login, sync index).
  desktop/   Tauri v2 + React — the agent workspace GUI (embeds the engine).
```

## The middleman model
The user talks to the agent **through Lectern**; the agent runs **as a child of
Lectern**. Inbound, Lectern augments the prompt with recalled memory + matched
skills + adaptive context. Outbound, it normalizes events, gates file edits behind
the Apply gate, and records episodes back to memory. The external agent keeps its
full freedom (parity-or-better contract) — Lectern adds power on top.

## Memory & skills (subject-keyed)
Memory and skills are keyed by **workspace + scope** (repo/global/team), **never by
backend** — so every agent and model that touches a repo shares one brain
automatically, and learns together. Recall is **hybrid**: SQLite FTS5 (lexical) +
vector cosine (a pluggable embedder; pure-Rust hashing by default, neural drop-in)
fused via reciprocal-rank fusion. Skills are recorded from the session event stream
(`/record`), matched by trigger, and auto-applied.

## Cloud sync (optional, content-blind)
The desktop/CLI authenticate to the Lectern cloud via the **OAuth 2.0 device grant**, then
report content-free usage and sync **end-to-end-encrypted** memory/skills blobs. The
cloud never receives source, prompts, or API keys.

## Data flow (one prompt)
```
prompt → adaptive context (memory + skills, budgeted) → backend adapter (Claude
Code via stream-json / API-key loop) → normalized events → Apply gate → memory
write → (usage telemetry, opt-in) → cloud
```

Full design vault: the Lectern-Brain Obsidian notes (planning) — this file is the
durable summary. Build status: see the brain's `Engine Build Status`.

## Tech
Rust (engine/CLI/daemon) · rusqlite (bundled SQLite + FTS5) · Tauri v2 + React
(desktop) · Next.js + libSQL/Turso (web) · ureq (cloud client). Linux-first
packaging (`.deb`/Flatpak/AppImage), systemd, Secret Service keychain, bubblewrap
sandbox (planned).
