# Lectern TUI

The full Lectern experience in your terminal — same engine, same brain, same
sessions as the desktop app, over the `lecternd` daemon.

## Install

- **One binary**: `cd apps/tui && bun build --compile src/index.tsx --outfile dist/lectern-tui`,
  then put `dist/lectern-tui` on your PATH (or next to the `lectern` CLI binary).
- **Launch**: `lectern tui` (resolves PATH → sibling → dev checkout) or run the
  binary directly. `--path <dir>` targets a workspace; `--backend mock` is a free
  test drive; `--once "<prompt>"` runs headless (scripting/CI).
- The TUI starts/uses `lecternd` automatically and rejects stale daemons.

## Everything is reachable three ways

Type `/command`, press **^X** then a letter, or use the fuzzy dialogs. `/help`
lists every command and key, generated from the registry itself.

| Command | Chord | What |
|---|---|---|
| /sessions | ^X s | fuzzy session switcher (★ pinned first, ● running) |
| /models | ^X m (also ^P) | fuzzy model picker across providers |
| /new · /rename · /pin · /export | ^X n · — · ^X p · — | session management |
| /plan /apply /conduct /one-shot | — · ^X a · ^X c · ^X o | sticky run modes (tinted pill) |
| /diffs | ^X d | full-screen tinted diff viewer |
| /clean | ^X v | hide machinery lines |
| /theme | ^X t | built-in + desktop theme files (~/.lectern/themes) |
| /brain /skills /usage /mcp-servers | ^X b · ^X k · ^X u · — | read-only panels |
| /quit | ^X q | exit |

Preferences (model, theme, clean) persist in `~/.lectern/tui.json`.

## Desktop parity (command level)

| Desktop feature | TUI |
|---|---|
| Sessions, streaming anatomy, modes, models | ✓ full |
| Pin / rename / export / folders | ✓ pin+rename+export · folders N/A (desktop organization) |
| Diff viewing | ✓ /diffs overlay |
| Clean/verbose toggle | ✓ /clean |
| Themes | ✓ same files, read-only |
| Usage page | ✓ /usage summary (charts stay desktop) |
| MCP management | read-only list (/mcp-servers); add/remove stays desktop |
| Brain / skills | read-only panels (recording stays desktop) |
| Embedded terminal | N/A by design — use your multiplexer (documented in /help) |
| Marketplace, GUI-skill replay, channels | N/A (desktop/engine surfaces) |

## Verification

`scripts/tui-drive.sh` drives every command and dialog in a private tmux server
and writes captures to `docs/tui/captures/` — run it after TUI changes.
