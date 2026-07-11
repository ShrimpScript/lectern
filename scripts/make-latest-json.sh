#!/usr/bin/env bash
# Generate the Tauri v2 updater manifest (latest.json) from signed release artifacts, so an
# installed app can discover and verify a newer release. See RELEASING.md.
#
# Usage:
#   # Linux only (back-compatible positional form):
#   scripts/make-latest-json.sh <version> <appimage> <sig> [notes-file] > latest.json
#
#   # Any subset of platforms, via flags (a version is always first):
#   scripts/make-latest-json.sh <version> \
#     [--linux   <appimage>     <sig>] \
#     [--win     <nsis-exe>     <sig>] \
#     [--mac-arm <app.tar.gz>   <sig>] \
#     [--mac-x64 <app.tar.gz>   <sig>] \
#     [--notes   <notes-file>] > latest.json
#
# Each platform's URL points at that release's GitHub asset; each signature is the exact
# contents of the artifact's .sig file (Tauri verifies it against the public key baked into
# the app). Platform keys are Tauri's OS-ARCH form: linux-x86_64, windows-x86_64,
# darwin-aarch64, darwin-x86_64. Windows uses the NSIS .exe; macOS uses the .app.tar.gz
# (NOT the .dmg).
set -euo pipefail

version="${1:?usage: make-latest-json.sh <version> [--linux a s] [--win e s] [--mac-arm t s] [--mac-x64 t s] [--notes f]}"
shift

notes_file=""
# key|url|sig-path triples, one per platform.
plat_args=()

add_platform() {
  local key="$1" artifact="$2" sig="$3"
  [ -f "$artifact" ] || { echo "no artifact at $artifact" >&2; exit 1; }
  [ -f "$sig" ] || { echo "no signature at $sig" >&2; exit 1; }
  local base
  base="$(basename "$artifact")"
  plat_args+=("$key" "https://github.com/ShrimpScript/lectern/releases/download/v${version}/${base}" "$sig")
}

# Back-compat: a bare `<appimage> <sig> [notes]` (no leading flag) means Linux.
if [ "$#" -ge 2 ] && [ "${1#--}" = "$1" ]; then
  add_platform "linux-x86_64" "$1" "$2"
  shift 2
  if [ "$#" -ge 1 ] && [ "${1#--}" = "$1" ]; then
    notes_file="$1"
    shift
  fi
fi

while [ "$#" -gt 0 ]; do
  case "$1" in
    --linux) add_platform "linux-x86_64" "$2" "$3"; shift 3 ;;
    --win) add_platform "windows-x86_64" "$2" "$3"; shift 3 ;;
    --mac-arm) add_platform "darwin-aarch64" "$2" "$3"; shift 3 ;;
    --mac-x64) add_platform "darwin-x86_64" "$2" "$3"; shift 3 ;;
    --notes) notes_file="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

[ "${#plat_args[@]}" -gt 0 ] || { echo "no platforms given" >&2; exit 1; }

notes="See the release notes for v${version}."
if [ -n "$notes_file" ] && [ -f "$notes_file" ]; then
  notes="$(cat "$notes_file")"
fi
pub_date="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# python reads each .sig by path (they can be multi-line) and does the JSON escaping.
python3 - "$version" "$notes" "$pub_date" "${plat_args[@]}" <<'PY'
import json, sys

version, notes, pub_date = sys.argv[1:4]
rest = sys.argv[4:]
platforms = {}
for i in range(0, len(rest), 3):
    key, url, sig_path = rest[i], rest[i + 1], rest[i + 2]
    with open(sig_path) as f:
        platforms[key] = {"signature": f.read().strip(), "url": url}

print(json.dumps({
    "version": version,
    "notes": notes,
    "pub_date": pub_date,
    "platforms": platforms,
}, indent=2))
PY
