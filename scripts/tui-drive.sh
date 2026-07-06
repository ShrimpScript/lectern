#!/usr/bin/env bash
# TUI-v3 T14: scripted drive of every command/dialog on a PRIVATE tmux server
# (never the user's). Captures land in docs/tui/captures/.
set -euo pipefail
cd "$(dirname "$0")/.."
T="tmux -L lectest-drive"
WS="${TUI_WS:-/tmp/lectern-tui-drive-ws}"
OUT=docs/tui/captures
mkdir -p "$WS" "$OUT"; [ -f "$WS/readme.md" ] || echo drive > "$WS/readme.md"
cap() { $T capture-pane -t d -p > "$OUT/$1.txt"; }
key() { $T send-keys -t d "$@"; }
$T kill-server 2>/dev/null || true
$T new-session -d -s d -x 110 -y 30 "cd apps/tui && bun run src/index.tsx --backend mock --path $WS 2>/dev/null"
sleep 4;                          cap 01-boot
key "build a settings page" Enter; sleep 5; cap 02-run-anatomy
key "/help" Enter; sleep 1;        cap 03-help;      key Escape; sleep 0.4
key C-s; sleep 1; key "set"; sleep 0.6; cap 04-sessions-filter; key Escape; sleep 0.4
key C-p; sleep 1;                  cap 05-models;    key Escape; sleep 0.4
key "/theme" Enter; sleep 1;       cap 06-theme;     key Escape; sleep 0.4
key "/diffs" Enter; sleep 1; key Enter; sleep 1; cap 07-diff-overlay; key Escape; sleep 0.4
key "/clean" Enter; sleep 0.8;     cap 08-clean
key "/pin" Enter; sleep 1;         cap 09-pin
key "/usage" Enter; sleep 1.2;     cap 10-usage;     key Escape; sleep 0.4
key "/mcp-servers" Enter; sleep 1.2; cap 11-mcp;     key Escape; sleep 0.4
key C-x; sleep 0.3;                cap 12-leader-hint
key Escape; sleep 0.4
# session management (T5) + rename-in-dialog (#188)
key "/rename drive suite session" Enter; sleep 1; cap 13-rename
key "/pin" Enter; sleep 1;         cap 14-pin
key "/export md" Enter; sleep 1.2; cap 15-export
key "/apply" Enter; sleep 0.6;     cap 16-mode-apply
key "/plan" Enter; sleep 0.6
key C-s; sleep 1; key C-r; sleep 0.6; cap 17-dialog-rename-mode; key Escape; sleep 0.3; key Escape; sleep 0.3
key C-x; sleep 0.3; key q
$T kill-server 2>/dev/null || true
echo "captures → $OUT/"
ls "$OUT"
