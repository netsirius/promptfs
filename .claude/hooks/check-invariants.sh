set -u
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0

# Rust lives under crates/*/src in the workspace layout. The pre-workspace `src/` is still
# accepted so this guard works before the phase-1 split lands. Discovering the roots rather
# than assuming one is the point: a guard that cannot find the code must not look like a
# guard that found nothing wrong.
rs_dirs=""
for d in src crates/*/src; do
  [ -d "$d" ] && rs_dirs="$rs_dirs $d"
done

manifests=""
for f in Cargo.toml crates/*/Cargo.toml; do
  [ -f "$f" ] && manifests="$manifests $f"
done

if [ -n "$manifests" ] && [ -z "$rs_dirs" ]; then
  echo 'check-invariants.sh: a Cargo.toml exists but no src/ tree was found under' >&2
  echo 'src/ or crates/*/src. The source checks below are skipping everything — fix the' >&2
  echo 'layout or fix this hook, but do not read a pass here as a clean bill of health.' >&2
fi

# ── Invariant 1 — no database crate, in any manifest ────────────────────────────────────
if [ -n "$manifests" ]; then
  hits=$(grep -nE '^[[:space:]]*(sqlx|diesel|redis|sled|rocksdb|rusqlite|mongodb|deadpool-postgres)[[:space:]]*=' $manifests)
  if [ -n "$hits" ]; then
    printf 'AGENTS.md invariant 1 violated — database crate in a manifest:\n%s\n' "$hits" >&2
    echo 'PromptFS is DB-less: the bare Git clones and the SDK bundles are derived caches,' >&2
    echo 'rebuildable from Git alone. Remove the crate, or say out loud that the design is' >&2
    echo 'wrong instead of adding persistence.' >&2
    exit 2
  fi
fi

# ── Invariant 8 — promptfs-core ships inside customers' processes ───────────────────────
if [ -f crates/promptfs-core/Cargo.toml ]; then
  dep_hits=$(grep -nE '^[[:space:]]*(git2|tokio|axum|hyper|reqwest|async-std|moka)[[:space:]]*=' crates/promptfs-core/Cargo.toml)
  if [ -n "$dep_hits" ]; then
    printf 'AGENTS.md invariant 8 violated — forbidden dependency in promptfs-core:\n%s\n' "$dep_hits" >&2
    echo 'The core is linked into customers Python processes via the pyo3 wheel. libgit2' >&2
    echo 'alone is ~3 MB plus a system dependency, for code that never reads a repo. Git' >&2
    echo 'access, HTTP and the async runtime belong in promptfs-server; the core takes bytes' >&2
    echo 'from its caller and stays synchronous.' >&2
    exit 2
  fi
fi

if [ -d crates/promptfs-core/src ]; then
  io_hits=$(grep -rnE 'std::fs::|std::net::|File::open|File::create|\.await\b' --include='*.rs' crates/promptfs-core/src \
            | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' \
            | grep -v 'ponytail:')
  if [ -n "$io_hits" ]; then
    printf 'AGENTS.md invariant 6 violated — I/O or async inside promptfs-core:\n%s\n' "$io_hits" >&2
    echo 'The core does no I/O, blocks on nothing and needs no runtime. Reading a bundle or a' >&2
    echo 'snapshot file belongs to the caller: promptfs-server and sdks/python do the reading' >&2
    echo 'and hand the core bytes.' >&2
    exit 2
  fi
fi

# ── Stack rule — never shell out to the git binary ──────────────────────────────────────
if [ -n "$rs_dirs" ]; then
  git_hits=$(grep -rnE 'Command::new\([[:space:]]*"[^"]*git"' --include='*.rs' $rs_dirs \
             | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//')
  if [ -n "$git_hits" ]; then
    printf 'AGENTS.md stack rule violated — shelling out to the git binary:\n%s\n' "$git_hits" >&2
    echo 'Use git2 (libgit2). Spawning a process per request blows the < 5 ms p99' >&2
    echo 'budget, and the git CLI text output is not a stable API.' >&2
    exit 2
  fi
fi

# ── Invariant 4 — canary weighting is seeded, never bare rand() ─────────────────────────
if [ -n "$rs_dirs" ]; then
  rng_hits=$(grep -rnE 'rand::random|thread_rng\(\)|rand::rng\(\)|OsRng' --include='*.rs' $rs_dirs \
             | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' \
             | grep -v 'ponytail:')
  if [ -n "$rng_hits" ]; then
    printf 'AGENTS.md invariant 4 violated — unseeded RNG:\n%s\n' "$rng_hits" >&2
    echo 'Canary weighting must be seeded from the routing key (OTel trace id, contextvar or' >&2
    echo 'explicit argument). A retried request has to resolve to the same version or traces' >&2
    echo 'become unattributable — and the aggregate weights look correct either way, so no' >&2
    echo 'test will catch this.' >&2
    echo 'Jittering a fallback poll interval is the one legitimate use: mark that line with a' >&2
    echo 'trailing `// ponytail: <reason>` comment to opt out.' >&2
    exit 2
  fi
fi

[ -n "$rs_dirs" ] && command -v cargo >/dev/null && cargo fmt --quiet 2>/dev/null

exit 0
