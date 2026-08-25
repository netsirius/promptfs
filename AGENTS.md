# AGENTS.md — PromptFS

Guidance for AI coding agents working in this repository.

## What PromptFS is

A Git-native, **DB-less** prompt manager. Prompts live as Markdown files with YAML
frontmatter + Jinja2 bodies inside a user's Git repo. PromptFS resolves, renders and
canary-routes them, and ships the React Studio UI embedded in a single Rust binary.

Consuming apps reach prompts two ways, and both run the same compiled core:

- **Hot path** — the SDK syncs a bundle once and does routing + rendering **inside the
  app's own process**. No network per call, render variables never leave the customer's
  process, and the app keeps serving while PromptFS is down.
- **Universal path** — `POST /v1/prompts/render` on the server, for Studio, `curl`, the CI
  eval runner and languages with no SDK.

Two constraints drive every design decision:

- **Git is the only source of truth.** No database. No persistent app state.
- **< 5 ms p99** on the server's cached render path; **< 50 MB** RSS idle. An in-process
  SDK render is microseconds: the bundle is already resident and the AST already compiled,
  so the *call* does no I/O. The SDK still does I/O at startup and while syncing.

If a change conflicts with either, it is the wrong change.

## Status

Greenfield. **Current phase: 1.** Nothing below is implemented yet — this file records
decisions already made so agents don't re-litigate them. Full spec, pipelines and
rationale: [`docs/architecture.md`](docs/architecture.md) — read it on demand, not by default.

Phase-1 work is broken down in [`docs/phase-1.md`](docs/phase-1.md). **Read it before
starting any phase-1 task**: it carries the owner of each task, and a task owned by Héctor is
scaffolded to the edge with a single `TODO(human)` and left unimplemented.

| Phase | Scope |
|---|---|
| 1 | Workspace split (`promptfs-core` + `promptfs-server`), `git2` bare-repo reads, `minijinja` rendering, basic `axum` REST |
| 2 | `deployments.yaml` router **in the core**, `moka` cache, canary weighting, bundle build + SSE push |
| 3 | Studio UI (React + Vite + Tailwind + Monaco), embedded via `rust-embed`; preview runs the core in wasm |
| 4 | `promptfs-py` (pyo3/abi3 wheels), `promptfs pull` snapshot subcommand, CI eval runner. TypeScript via wasm in 4b |

## Layout

```
promptfs/
├── Cargo.toml            # workspace
├── crates/
│   ├── promptfs-core/    # parse · compile · render · router · shared types
│   │                     # NO git2 · NO tokio · NO network · sync and pure
│   ├── promptfs-server/  # core + git2 + axum + moka + webhooks + SSE
│   │                     # binary also hosts `promptfs pull` (HTTP client mode)
│   └── promptfs-py/      # core + pyo3 → abi3 wheel
├── studio/               # React 18 + TS + Vite + Tailwind + Monaco
├── sdks/
│   ├── python/           # idiomatic wrapper over promptfs-py
│   └── typescript/       # phase 4b, core via wasm
└── fixtures/             # sample prompt repos used by tests
```

**Why a workspace from phase 1.** The earlier guidance was to stay single-crate until
something outside the server binary needed to link the core. The Python SDK embeds the
core, so that trigger has fired — and the split has to land before the core grows roots
into `git2` and `tokio`. It costs ~20 lines of `Cargo.toml` now and a real refactor later.

The core's dependency list is a hard boundary, not a preference: it ships inside
customers' Python processes. `libgit2` in that wheel would be ~3 MB and a system
dependency, for code that never reads a repo.

## Stack — decided, do not substitute

| Concern | Choice | Notes |
|---|---|---|
| Async runtime / HTTP | `tokio` + `axum` | `promptfs-server` only — never in the core |
| Git access | `git2` (libgit2) | `promptfs-server` only. Never shell out to the `git` binary |
| Templating | `minijinja` | Not `tera`, not `handlebars` |
| Cache | `moka` | Concurrent, TinyLFU |
| Serialization | `serde` + `serde_yaml` + `serde_json` | |
| Static assets | `rust-embed` | Studio build embedded at compile time |
| Python bindings | `pyo3` + `maturin`, `abi3` | One wheel per platform covers Python 3.8+ |
| Wheel CI | `cibuildwheel` | ~5 artifacts per release |
| Frontend | React 18, TypeScript, Vite, Tailwind | |
| Editor | `@monaco-editor/react` | Diff view included |
| Client state | TanStack Query (server) + Zustand (UI) | |

**No database crate belongs in any `Cargo.toml`.** No `sqlx`, `diesel`, `redis`, `sled`,
`rocksdb`. If persistence feels necessary, the design is wrong — say so instead of
adding one.

## Invariants

Break these and the product stops being what it is:

1. **No source of truth outside Git.** The server's bare clones and the SDKs' bundles are
   both derived caches: rebuildable from Git alone, and disposable. Any instance must come
   up cold and serve correctly. Instances never coordinate.
2. **No secrets and no render variables on disk or in logs.** Git PATs, SSH keys and LLM
   provider keys come from env vars or request headers. Render variables are caller data,
   potentially PII: never log them, never put them in an error message, never attach them
   to a span. An error names *which* input was missing, never its value.
3. **Autoescape OFF, configured in `promptfs-core`.** These are LLM prompts, not HTML.
   Autoescape silently corrupts prompt bodies (`&` → `&amp;`) and passes every naive test.
   The core owns the minijinja `Environment`; no caller builds its own, so server, SDK,
   Studio preview and eval runner inherit the setting instead of each repeating it.
4. **Canary routing is per-request stable *and* observable.** Weighting is seeded from a
   routing key, not bare `rand()`, so a retry resolves to the same version. The SDK derives
   that key itself — OTel trace id → contextvar → explicit argument → random with a logged
   warning, never silently. Every render result carries `resolved_ref` and `commit`. Stable
   without being observable is useless: the caller still cannot attribute a trace to a
   prompt version.
5. **Invalidation is webhook-driven, end to end.** Git webhook → invalidate the affected
   `moka` entries → fetch the bare repo → rebuild the bundle → push to connected SDKs over
   SSE. Polling is the fallback at both hops, never the primary path. The published SLA
   covers only the hop we own — webhook receipt to SDK serving — because webhook delivery
   latency belongs to the Git provider.
6. **The render path is hot and runs in someone else's process.** Frontmatter is parsed and
   the Jinja AST compiled exactly once — at cache fill on the server, at bundle load in an
   SDK — and the call path renders an already-compiled AST. Inside `promptfs-core`: no I/O,
   no blocking, no async runtime, and no allocation per render beyond the output string.
   Reading a bundle or a snapshot file belongs to the caller: `promptfs-server` and
   `sdks/python` do the reading and hand the core bytes.
7. **Studio builds before the release binary.** `rust-embed` snapshots `studio/dist/` at
   compile time. A stale `dist/` ships a stale UI silently.
8. **One render implementation.** Nothing turns a prompt file into a string except
   `promptfs-core`. Server, SDKs, the Studio preview and the CI eval runner all link the
   same compiled code, which is what makes "what Studio shows is what your app sends" a
   structural property rather than a promise checked by tests. It follows that the core
   links neither `git2` nor `tokio` — it runs inside processes that are not ours. A second
   implementation, even "just for tests", is forbidden.

## Managed-repo contract

PromptFS reads this structure from the user's repo. It is a public contract — changing
it breaks every existing user.

```
├── .promptfs/deployments.yaml
├── prompts/<namespace>/<name>.prompt.md
├── prompts/<namespace>/<name>.eval.yaml
└── config.yaml
```

Prompt file: YAML frontmatter (`name`, `description`, `model`, `temperature`, `inputs`)
delimited by `---`, then a Jinja2 body.

Deployment file: `deployments.<namespace>/<name>.environments.<env>` with `strategy` and
weighted `targets`, each target a Git ref (`tags/v1.2.0`, `heads/main`).

Prompts are addressed as `<namespace>/<name>`, refs are always fully qualified
(`tags/…`, `heads/…`) — never bare names.

## Consumer contract

Equally public, and equally unchangeable once a wheel is on PyPI. Full detail in
[`docs/architecture.md`](docs/architecture.md) §5.3; the parts worth knowing by heart:

- `GET /v1/bundle?env=<env>&format=<n>` returns the ruleset plus the source of every active
  target for that environment. The server compiles each one first; what fails to compile
  ships as an `error` with no `source`, so one broken prompt cannot take down the rest.
- A render result always carries `body`, `model`, `temperature`, `resolved_ref`, `commit`,
  `bundle_version` and `source`.
- The SDK never fails a render because PromptFS is unreachable. It fails only with no
  bundle at all, and that failure happens when `Client()` is constructed, at startup.

## Commands

Target shape. None of these run yet in phase 1.

```bash
cargo run -p promptfs-server      # dev server
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt

cd studio && npm run dev          # Studio dev server (proxies to :8080)
cd studio && npm run build        # produces studio/dist/, consumed by rust-embed
```

## Conventions

- Errors: `thiserror` for library errors, `anyhow` at the binary edge. A malformed prompt
  file in a user's repo is a 4xx with the file path and line, never a 500 and never a panic.
- Tests: prefer fixture repos under `fixtures/` created via `git2` in-test over mocking
  `git2` itself. The Git interaction is the part most likely to be wrong.
- The bundle format is additive within a major version: fields get added, never removed or
  repurposed, and SDKs ignore fields they don't know. A wheel already on PyPI cannot be
  made to upgrade.
- Every non-trivial module leaves one runnable check behind. No test frameworks beyond
  `#[test]` / `#[tokio::test]`.
- Commit messages are plain. No AI attribution trailers or footers.

## Working style

Reach for the smallest change that actually works, but read the whole path before
choosing it. Prefer the standard library, then an already-present dependency; a new crate
needs a reason stated in the PR — and a new dependency in `promptfs-core` needs a stronger
one than elsewhere, because it ships inside customers' processes. Delete rather than add.

Anything marked with a `ponytail:` comment is a deliberate shortcut with a named ceiling —
read the comment before "improving" it.
