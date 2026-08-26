#!/usr/bin/env bash
# Validates a directory against the PromptFS managed-repo contract.
#
#   ./validate.sh [path]      validate a repo (default: cwd)
#   ./validate.sh --self-test run the built-in checks
#
# The managed-repo layout is a PUBLIC contract — changing it breaks every existing user.
# Today it lives only in prose in AGENTS.md; this is the executable copy.
#
# ponytail: line-oriented grep/awk, not a YAML parser. pyyaml is not in the stdlib and a
# skill helper should not need a pip install. It assumes the canonical 2-space indentation
# from docs/architecture.md §4. Once the Rust crate exists, serde_yaml can do this properly
# in a #[test] and this script becomes the pre-commit convenience copy.
set -u

fail=0
err()  { printf '  \033[31mFAIL\033[0m %s\n' "$*"; fail=1; }
warn() { printf '  \033[33mWARN\033[0m %s\n' "$*"; }
ok()   { printf '  \033[32m ok \033[0m %s\n' "$*"; }

# ---------------------------------------------------------------------------
# Weight validation — the one rule with several defensible answers.
# ---------------------------------------------------------------------------

# validate_weights <label> <weight>...
#
# Called once per <prompt, environment> pair with that environment's target weights.
# Print a reason and `return 1` to reject; `return 0` to accept.
#
# TODO(human)
validate_weights() {
  local label="$1"; shift
  warn "$label: weight rule not implemented yet (weights: $*)"
  return 0
}

# ---------------------------------------------------------------------------
# Structure checks
# ---------------------------------------------------------------------------

check_layout() {
  local root="$1"
  [ -d "$root/prompts" ] \
    && ok "prompts/ present" \
    || err "prompts/ missing — the contract requires prompts/<namespace>/<name>.prompt.md"
  [ -f "$root/.promptfs/deployments.yaml" ] \
    && ok ".promptfs/deployments.yaml present" \
    || err ".promptfs/deployments.yaml missing — nothing can be routed without it"
  [ -f "$root/config.yaml" ] || warn "config.yaml missing (optional, holds provider options)"
}

check_prompts() {
  local root="$1" found=0 f stem name

  while IFS= read -r f; do
    found=$((found + 1))
    stem=$(basename "$f" .prompt.md)

    # Addressed as <namespace>/<name>: the file needs a namespace directory above it.
    case "${f#"$root"/prompts/}" in
      */*) : ;;
      *) err "${f#"$root"/}: not namespaced — must be prompts/<namespace>/<name>.prompt.md" ;;
    esac

    # Frontmatter is delimited by --- and must open on line 1.
    if [ "$(sed -n '1p' "$f")" != "---" ]; then
      err "${f#"$root"/}:1: missing opening --- frontmatter delimiter"
      continue
    fi
    if ! sed -n '2,$p' "$f" | grep -qx -- '---'; then
      err "${f#"$root"/}: unterminated frontmatter — no closing ---"
      continue
    fi

    # Required keys, read only from inside the frontmatter block.
    local fm
    fm=$(sed -n '2,$p' "$f" | sed -n '1,/^---$/p')
    local key
    for key in name description model; do
      printf '%s\n' "$fm" | grep -qE "^${key}:[[:space:]]*[^[:space:]]" \
        || err "${f#"$root"/}: frontmatter missing required key '${key}'"
    done

    # The addressable name must match the filename, or <namespace>/<name> lookups miss.
    name=$(printf '%s\n' "$fm" | sed -n 's/^name:[[:space:]]*//p' | head -1 | tr -d '"'"'"' ')
    if [ -n "$name" ] && [ "$name" != "$stem" ]; then
      err "${f#"$root"/}: frontmatter name '${name}' does not match filename '${stem}'"
    fi
  done <<EOF
$(find "$root/prompts" -type f -name '*.prompt.md' 2>/dev/null | sort)
EOF

  [ "$found" -gt 0 ] && ok "$found prompt file(s) checked" || warn "no *.prompt.md files found"
}

check_deployments() {
  local root="$1" dep="$1/.promptfs/deployments.yaml"
  [ -f "$dep" ] || return 0

  # Refs are ALWAYS fully qualified — tags/…, heads/… or a raw SHA. Never bare names.
  local bad
  bad=$(grep -nE '^[[:space:]]*-?[[:space:]]*ref:' "$dep" \
        | sed 's/[[:space:]]*$//' \
        | grep -vE 'ref:[[:space:]]*"?(tags|heads)/[^"]+"?$' \
        | grep -vE 'ref:[[:space:]]*"?[0-9a-f]{7,40}"?$' || true)
  if [ -n "$bad" ]; then
    printf '%s\n' "$bad" | while IFS= read -r line; do
      err "deployments.yaml:${line%%:*}: ref is not fully qualified — use tags/… or heads/…"
    done
    fail=1
  else
    ok "all refs fully qualified"
  fi

  # Group weights by <prompt, environment>, then hand each set to validate_weights.
  local grouped
  grouped=$(awk '
    { line = $0; sub(/#.*$/, "", line)
      match(line, /^ */); ind = RLENGTH
      key = line; sub(/^ */, "", key); sub(/[[:space:]]+$/, "", key) }
    key == "" { next }
    ind == 2 && key ~ /:$/ { prompt = substr(key, 1, length(key) - 1); next }
    ind == 6 && key ~ /:$/ { env    = substr(key, 1, length(key) - 1); next }
    key ~ /weight:/ {
      v = key; sub(/^.*weight:[[:space:]]*/, "", v)
      agg[prompt "|" env] = agg[prompt "|" env] " " v
      next
    }
    END { for (k in agg) print k agg[k] }
  ' "$dep")

  [ -n "$grouped" ] || { warn "no weights found in deployments.yaml"; return 0; }

  while IFS= read -r row; do
    [ -n "$row" ] || continue
    local label="${row%% *}" weights="${row#* }"
    # shellcheck disable=SC2086
    validate_weights "${label/|/ → }" $weights || fail=1
  done <<EOF
$grouped
EOF
}

# ---------------------------------------------------------------------------

run() {
  local root="${1%/}"
  [ -d "$root" ] || { echo "not a directory: $root" >&2; exit 1; }
  echo "contract-check: $root"
  check_layout "$root"
  check_prompts "$root"
  check_deployments "$root"
  if [ "$fail" -eq 0 ]; then
    printf '\n\033[32mcontract ok\033[0m\n'
  else
    printf '\n\033[31mcontract violations found\033[0m\n'
  fi
  return "$fail"
}

# ---------------------------------------------------------------------------
# Self-test — AGENTS.md: "Every non-trivial module leaves one runnable check behind."
# ---------------------------------------------------------------------------

self_test() {
  local tmp good bad rc failures=0
  tmp=$(mktemp -d)
  # Expand now: $tmp is local and out of scope by the time the trap fires.
  trap "rm -rf '$tmp'" EXIT
  good="$tmp/good"; bad="$tmp/bad"

  mkdir -p "$good/.promptfs" "$good/prompts/support"
  cat >"$good/config.yaml" <<'Y'
providers:
  openai:
    base_url: https://api.openai.com/v1
Y
  cat >"$good/prompts/support/classifier.prompt.md" <<'P'
---
name: classifier
description: Classifies user intent.
model: gpt-4o-mini
temperature: 0.1
---
Tier: {{ customer_tier }}
P
  cat >"$good/.promptfs/deployments.yaml" <<'D'
deployments:
  support/classifier:
    environments:
      production:
        strategy: canary
        targets:
          - ref: "tags/v1.0.0"
            weight: 85
          - ref: "heads/main"
            weight: 15
D

  # Same tree, three seeded defects: bare ref, name/filename mismatch, missing 'model'.
  cp -R "$good" "$bad"
  sed -i.bak 's|"tags/v1.0.0"|"v1.0.0"|' "$bad/.promptfs/deployments.yaml" && rm -f "$bad"/.promptfs/*.bak
  cat >"$bad/prompts/support/classifier.prompt.md" <<'P'
---
name: not-the-filename
description: Classifies user intent.
---
Tier: {{ customer_tier }}
P

  echo "== self-test: valid repo should pass =="
  ( fail=0; run "$good" >/dev/null 2>&1 ); rc=$?
  [ "$rc" -eq 0 ] && echo "  ok  valid repo accepted" \
                  || { echo "  FAIL valid repo rejected (rc=$rc)"; failures=1; }

  echo "== self-test: broken repo should fail =="
  local out; out=$( fail=0; run "$bad" 2>&1 ); rc=$?
  [ "$rc" -ne 0 ] && echo "  ok  broken repo rejected" \
                  || { echo "  FAIL broken repo accepted"; failures=1; }
  for expect in "not fully qualified" "does not match filename" "missing required key 'model'"; do
    printf '%s' "$out" | grep -q "$expect" \
      && echo "  ok  detected: $expect" \
      || { echo "  FAIL undetected: $expect"; failures=1; }
  done

  [ "$failures" -eq 0 ] && echo "self-test passed" || echo "self-test FAILED"
  return "$failures"
}

case "${1:---}" in
  --self-test) self_test ;;
  --) run "." ;;
  *)  run "$1" ;;
esac
