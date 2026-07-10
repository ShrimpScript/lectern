# Changelog

All notable changes to Lectern are recorded here. This file is the single source of
truth: each GitHub Release, the website changelog, and the in-app "what's new" are
generated from it.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
Lectern follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). See
[RELEASING.md](RELEASING.md) for how a release is cut.

## [Unreleased]

## [0.7.0] - 2026-07-10

### Added
- **Checkpoints & rewind.** Lectern snapshots your workspace before an agent writes to
  it, so you can undo a run you don't like and try a different prompt. Snapshots use a
  private, per-workspace git store that is completely separate from your project's own
  `.git`, so it works on non-git folders and never touches your history. Rewind reverts
  edits and removes files the agent added, and is itself reversible.
  - CLI: `lectern checkpoint list` and `lectern rewind <id>`.
  - Desktop: a checkpoint marker in the chat with an inline **Restore** action that also
    re-fills the composer so you can adjust the prompt and try again.
  - Secrets (`.env`) and the brain store are never snapshotted.
- **In-app auto-updates (Linux).** The desktop app checks for newer signed releases and
  offers a one-click **Restart & update** — download, install, relaunch. Every update is
  verified against Lectern's signing key before it installs.
- **"What's new" on update.** After updating, the app shows what changed in the new
  version, drawn straight from this changelog.

## [0.6.0] - 2026-07-08

### Changed
- **Smarter, leaner memory.** Recall now applies a relevance floor, so small talk recalls
  nothing while genuine matches still surface. When memory content is needed, only the
  most relevant window of a file enters context (roughly 9× less recalled context on a
  typical task). The agentic path passes recalled paths, not file contents.

### Added
- **One-click provider setup.** Each provider in Settings (Claude Code, Antigravity,
  OpenCode, OpenRouter, Ollama) expands an OS-aware panel with the exact install command,
  a copy button, an auth next-step, and links. Vetted user-space installers (OpenCode,
  Ollama) have a one-click install that streams its output.
- **Feature-level documentation** at [getlectern.vercel.app/docs](https://getlectern.vercel.app/docs)
  covering chat commands, the Conductor, the brain, scheduling, and the Hub.

## [0.5.0] - 2026-07-06

### Added
- **First public release.** Local-first agent orchestration with per-task model routing,
  a persistent brain (memory, learned skills, code graph), and multi-provider support
  (Claude Code, Antigravity, OpenCode, OpenRouter, Ollama).
- **Terminal stack** — CLI, a full TUI, and a background daemon, installable with one
  command; also a Nix profile install.
- **Desktop app** — a native cockpit (Tauri) with cross-platform installers for Linux
  (AppImage, `.deb`), Windows (`.exe`), and macOS (`.dmg`); tiled sessions, an embedded
  terminal, live streaming, and a work-panel preview rail.
- **The Conductor** — plans a task, routes each step to the model that fits, and
  cross-reviews the result.
- **MCP + the Hub** — a catalog of MCP servers with cross-harness registration, and a
  community skills hub with an audit gate.
- Sessions are shared across the desktop, TUI, and CLI through one engine store.

[Unreleased]: https://github.com/ShrimpScript/lectern/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/ShrimpScript/lectern/releases/tag/v0.7.0
[0.6.0]: https://github.com/ShrimpScript/lectern/releases/tag/v0.6.0
[0.5.0]: https://github.com/ShrimpScript/lectern/releases/tag/v0.5.0
