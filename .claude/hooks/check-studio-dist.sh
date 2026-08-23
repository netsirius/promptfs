#!/usr/bin/env bash
# PreToolUse(Bash): enforces AGENTS.md invariant 7 — "Studio builds before the release binary."
#
# rust-embed is a proc macro: it snapshots studio/dist/ at COMPILE time. In debug it reads
# from disk at runtime, so this never bites during development. In release it embeds, and a
# stale dist/ ships a stale UI with no error, no warning and no failing test — the Rust tests
# cover the API, not the embedded bytes, and the old UI works fine. It is just old.
#
# Requires: jq (present on macOS by default at /usr/bin/jq).
# Exit 2 = message on stderr goes back to Claude. Exit 0 = silent pass.
set -u
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0

# Inert until phase 3 creates studio/. Checked first so the jq probe stays quiet on a tree
# that has no Studio yet.
[ -d studio/src ] || exit 0

if ! command -v jq >/dev/null; then
  echo 'check-studio-dist.sh: jq not found — cannot read the command, skipping the' >&2
  echo 'invariant 7 check. Install jq, or this guard is silently inert.' >&2
  exit 0
fi

cmd=$(jq -r '.tool_input.command // ""' 2>/dev/null)
case "$cmd" in
  *"cargo build"*--release*|*"cargo install"*|*"cargo dist"*) ;;
  *) exit 0 ;;
esac

if [ ! -f studio/dist/index.html ]; then
  echo 'AGENTS.md invariant 7 — studio/dist/index.html is missing.' >&2
  echo 'rust-embed would embed an empty (or absent) asset directory and the release binary' >&2
  echo 'would serve no UI at all. Run: cd studio && npm run build' >&2
  exit 2
fi

# ponytail: mtime heuristic against studio/dist/index.html. Covers the common case (editing
# studio/src) but misses build inputs that live outside src/ — package.json, vite.config.ts,
# tailwind.config.js, studio/index.html. Widen the -newer sweep if one of those ever ships stale.
stale=$(find studio/src -type f -newer studio/dist/index.html 2>/dev/null | head -3)
if [ -n "$stale" ]; then
  printf 'AGENTS.md invariant 7 violated — studio/src is newer than studio/dist:\n%s\n' "$stale" >&2
  echo 'rust-embed snapshots dist/ at compile time, so this build would embed the previous' >&2
  echo 'UI silently: it starts, it serves, the tests pass, and the UI is old.' >&2
  echo 'Run: cd studio && npm run build' >&2
  exit 2
fi

exit 0
