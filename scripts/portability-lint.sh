#!/usr/bin/env bash
# Mission ports P1b: fail CI when unix-only patterns sneak in outside their
# sanctioned homes. Cheap grep gate — the point is catching drift the day it
# happens, not perfection.
set -euo pipefail
fail=0

# Raw HOME reads must go through lectern_engine::home_dir() (USERPROFILE fallback).
hits=$(grep -rn 'env::var("HOME")' crates/ apps/desktop/src-tauri/src 2>/dev/null \
  | grep -v "crates/engine/src/lib.rs" || true)
if [ -n "$hits" ]; then
  echo "✗ raw HOME reads (use lectern_engine::home_dir()):"; echo "$hits"; fail=1
fi

# std::os::unix is allowed only in lecternd (unix-only by design until P2a),
# cfg(unix)-gated CLI probe, and cloud.rs (already gated).
hits=$(grep -rln "std::os::unix" crates/engine/src crates/lectern/src apps/desktop/src-tauri/src 2>/dev/null \
  | grep -v "crates/lectern/src/main.rs" | grep -v "crates/engine/src/cloud.rs" || true)
if [ -n "$hits" ]; then
  echo "✗ unix-only APIs outside sanctioned files:"; echo "$hits"; fail=1
fi

[ "$fail" = 0 ] && echo "✓ portability lint clean"
exit $fail
