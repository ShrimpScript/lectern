#!/usr/bin/env bash
# Generate the Tauri v2 updater manifest (latest.json) from a signed AppImage, so an
# installed app can discover and verify a newer release. See RELEASING.md.
#
# Usage:
#   scripts/make-latest-json.sh <version> <appimage-path> <sig-path> [notes-file] > latest.json
#
# The URL points at the release download for that version (a GitHub release asset). The
# signature is the exact contents of the AppImage's .sig file (Tauri verifies it against
# the public key baked into the app).
set -euo pipefail

version="${1:?usage: make-latest-json.sh <version> <appimage> <sig> [notes-file]}"
appimage="${2:?missing AppImage path}"
sig="${3:?missing .sig path}"
notes_file="${4:-}"

[ -f "$appimage" ] || { echo "no AppImage at $appimage" >&2; exit 1; }
[ -f "$sig" ] || { echo "no signature at $sig" >&2; exit 1; }

base="$(basename "$appimage")"
url="https://github.com/ShrimpScript/lectern/releases/download/v${version}/${base}"
signature="$(cat "$sig")"
notes="See the release notes for v${version}."
if [ -n "$notes_file" ] && [ -f "$notes_file" ]; then
  notes="$(cat "$notes_file")"
fi
pub_date="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# python does the JSON escaping (notes and signature can contain newlines).
python3 - "$version" "$signature" "$url" "$notes" "$pub_date" <<'PY'
import json, sys
version, signature, url, notes, pub_date = sys.argv[1:6]
print(json.dumps({
    "version": version,
    "notes": notes,
    "pub_date": pub_date,
    "platforms": {
        "linux-x86_64": {"signature": signature, "url": url},
    },
}, indent=2))
PY
