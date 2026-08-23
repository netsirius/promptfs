# AGENTS.md — PromptFS

Guidance for AI coding agents working in this repository.

## What PromptFS is

A Git-native, **DB-less** prompt manager. Prompts live as Markdown files with YAML
frontmatter + Jinja2 bodies inside a user's Git repo. PromptFS resolves, renders and
canary-routes them over a REST API, and ships the React Studio UI embedded in a single
Rust binary.

Two constraints drive every design decision:

- **Git is the only source of truth.** No database. No persistent app state.
- **< 5 ms p99** on the cached render path; **< 50 MB** RSS idle.

If a change conflicts with either, it is the wrong change.

## Status

Greenfield. **Current phase: 1.** Nothing below is implemented yet — this file records
decisions already made so agents don't re-litigate them. Full spec, pipelines and
rationale: [`docs/architecture.md`](docs/architecture.md) — read it on demand, not by default.

| Phase | Scope |
|---|---|
| 1 | Core engine: `git2` bare-repo reads, `minijinja` rendering, basic `axum` REST |
| 2 | `deployments.yaml` router, `moka` cache, canary weighting |
| 3 | Studio UI (React + Vite + Tailwind + Monaco), embedded via `rust-embed` |
| 4 | Python/TypeScript SDKs, CI eval runner |

## Layout

```
promptfs/
├── Cargo.toml            # single crate — see "Splitting the crate" below
├── src/                  # Rust core + server
├── studio/               # React 18 + TS + Vite + Tailwind + Monaco
├── sdks/
│   ├── python/
│   └── typescript/
└── fixtures/             # sample prompt repos used by tests
```

**Splitting the crate:** stay single-crate. Split into a workspace only when something
outside the server binary needs to link the core (a Rust SDK, a separate CLI binary).
Not before.

## Stack — decided, do not substitute

| Concern | Choice | Notes |
|---|---|---|
| Async runtime / HTTP | `tokio` + `axum` | |
| Git access | `git2` (libgit2) | Never shell out to the `git` binary |
| Templating | `minijinja` | Not `tera`, not `handlebars` |
| Cache | `moka` | Concurrent, TinyLFU |
| Serialization | `serde` + `serde_yaml` + `serde_json` | |
| Static assets | `rust-embed` | Studio build embedded at compile time |
| Frontend | React 18, TypeScript, Vite, Tailwind | |
| Editor | `@monaco-editor/react` | Diff view included |
| Client state | TanStack Query (server) + Zustand (UI) | |

**No database crate belongs in `Cargo.toml`.** No `sqlx`, `diesel`, `redis`, `sled`,
`rocksdb`. If persistence feels necessary, the design is wrong — say so instead of
adding one.

## Invariants

Break these and the product stops being what it is:

1. **No persistent state.** The bare Git clones on disk are an ephemeral cache: any
   instance must come up cold and serve correctly. Instances never coordinate.
2. **No secrets on disk.** Git PATs, SSH keys and LLM provider keys come from env vars
   or request headers. Never write them to a file, never log them.
3. **Autoescape OFF in minijinja.** These are LLM prompts, not HTML. Autoescape silently
   corrupts prompt bodies (`&` → `&amp;`) and passes every naive test. Configure the
   environment explicitly; do not rely on extension-based defaults.
4. **Canary routing must be per-request stable.** Weighting is seeded from a caller-supplied
   key (request id / session id / user id), not bare `rand()`. A retried request must
   resolve to the same version, or traces become unattributable.
5. **Cache invalidation is webhook-driven.** Git webhook → invalidate the affected `moka`
   entries → fetch the bare repo. Polling is the fallback, not the primary path.
6. **The render path is hot.** Parse frontmatter and compile the Jinja AST once at cache
   fill; the request path renders an already-compiled AST. No per-request YAML parsing,
   no per-request file I/O, no `String` churn where `&str` works.
7. **Studio builds before the release binary.** `rust-embed` snapshots `studio/dist/` at
   compile time. A stale `dist/` ships a stale UI silently.

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

## Commands

Target shape. None of these run yet in phase 1.

```bash
cargo run                  # dev server
cargo test
cargo clippy -- -D warnings
cargo fmt

cd studio && npm run dev   # Studio dev server (proxies to :8080)
cd studio && npm run build # produces studio/dist/, consumed by rust-embed
```

## Conventions

- Errors: `thiserror` for library errors, `anyhow` at the binary edge. A malformed prompt
  file in a user's repo is a 4xx with the file path and line, never a 500 and never a panic.
- Tests: prefer fixture repos under `fixtures/` created via `git2` in-test over mocking
  `git2` itself. The Git interaction is the part most likely to be wrong.
- Every non-trivial module leaves one runnable check behind. No test frameworks beyond
  `#[test]` / `#[tokio::test]`.
- Commit messages are plain. No AI attribution trailers or footers.

## Working style

Reach for the smallest change that actually works, but read the whole path before
choosing it. Prefer the standard library, then an already-present dependency; a new crate
needs a reason stated in the PR. Delete rather than add.

Anything marked with a `ponytail:` comment is a deliberate shortcut with a named ceiling —
read the comment before "improving" it.
