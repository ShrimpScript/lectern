# Changelog

All notable Lectern changes, newest first. Dates are ship dates on `main`.

## 2026-07-03

### TUI v3 — OpenCode-class terminal shell (PRs #175–#184)
- New single-focus layout: header · full-width conversation · always-focused
  input · status bar (model · mode · ctx · run state).
- One command registry → slash commands (prefix+fuzzy), `^X` leader chords,
  and a self-generating `/help`.
- Fuzzy dialog kit: sessions (★ pinned first, ● running, `^R` inline rename),
  models, themes (reads the desktop theme manager's files).
- Sticky run modes `/plan /apply /conduct /one-shot` with a tinted pill.
- Full-screen tinted diff viewer (`/diffs`), `/clean` machinery toggle,
  `/usage` + `/mcp-servers` read-only panels.
- Preferences persist in `~/.lectern/tui.json`; real mouse support.
- Ships as a single binary; `lectern tui` launches it from anywhere.
- Verification: `scripts/tui-drive.sh` (17 scripted steps, committed captures).

### Desktop-verified GUI run (PRs #193–#197)
- Tile drag-resize + Share "Copied ✓" feedback (#193); real `/skill` + `/mcp`
  commands (#194); preview rail v1 as a work-panel tab (#195).
- **Cross-surface sessions shipped**: desktop chats dual-write into the engine
  store (#196), and the store is authoritative at boot — TUI/CLI sessions
  appear in the desktop with lazy-loaded history, renames from any surface
  win by recency (#197).

### Continued development (PRs #185–#191)
- Entitlement tokens are EdDSA-signed JWTs when `LECTERN_SIGNING_KEY` is set
  (`npm run gen-signing-key`); honest `signed:false` otherwise (#185).
- README + site truth pass: Linux-first wording, dual-transport daemon,
  TUI sections (#186).
- Session-unification phase 1: engine store carries desktop session metadata
  (`meta` + `updated_at`, validated writes) + tauri surface; one shared
  `Store::migrate()` for file-backed and in-memory stores (#187).
- Sessions-dialog inline rename; preview-rail design doc (#188).
- Drive suite caught + fixed fresh-session adoption; securebundle
  tamper/truncation tests (#189).
- Docker/SSH terminal-backend design doc; Lectern-Brain vault synced (#190).
- Injectable-path MCP overview + tests; `lectern-tui --version` (#191).

## 2026-07-02 → 2026-07-03

### Ports — Windows + macOS first-class program (PRs #170–#174)
- **Keep-it-green rule**: every PR touching engine/CLI/TUI proves Windows
  compile+test before merge; weekly macOS sweep; portability lint in main CI.
- **Cross-platform daemon transport**: unix socket unchanged on Linux/macOS;
  Windows uses 127.0.0.1 + a per-boot token required on every request.
- Full daemon session loop proven on real Windows AND macOS runners;
  **the desktop app launched on real Windows and macOS machines for the
  first time** (exit-code launch smoke, both green).
- GUI skill replay states plainly it's Linux-only for now.

### Interactive fixes (PRs #160–#169)
- Embedded terminal: PTY output moved to tauri Channels (works in release
  builds) + blended inset redesign; live-proven in the real app (#166–#167).
- TUI stale-daemon capability probe — a pinging-but-ancient daemon is
  rejected with a clear message (#168).
- Lag-spike fixes: 55× cheaper context meter, MCP probing off the boot path,
  workspace re-index throttling (1.85s → 0.07s per message) (#160).
- Connect library (25 verified MCP servers + channels), GitHub-style usage
  activity grid, boot splash, typewriter streaming, machinery-row icons,
  menu dismissal fix (#161–#165).
- Pinned-chat icons, Hermes-teardown research, tileable sessions (tmux-style
  splits), session terminal button (#166).

## Earlier — (PRs #121–#159, 2026-07-02)
Cockpit clarity (custom selects/switches, provider truth rows, routing
config), MCP ecosystem (12-server catalog, cross-harness registration w/
truthful support matrix, Channels split), power features (clean/verbose
output, chat folders/export/themes/usage/context meter), marketplace hub
(official tier, docs viewer, $0 AI audit gate), TUI v2 chat-core parity,
Win/Mac CI foundations + unsigned installers, E2EE session export/import.
