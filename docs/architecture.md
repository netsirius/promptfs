# PromptFS — Technical Specification & Architecture

> Reference document. The always-loaded summary of decisions and invariants lives in
> [`AGENTS.md`](../AGENTS.md); this file carries the detail behind them.

## 1. Overview

PromptFS is an open-source, Git-native and DB-less prompt manager for engineering and
AI-product teams. It is a high-performance middleware layer for managing, versioning,
testing, deploying and canary-releasing prompts straight out of Git repositories
(GitHub, Azure Repos, GitLab, …) with no traditional database (PostgreSQL, MongoDB, …).

## 2. Requirements

### 2.1 Functional (FR)

**Git-native prompt management**
- Prompts stored as text with Jinja2 syntax and YAML frontmatter metadata.
- Native compatibility with any remote Git provider via personal access tokens (PAT)
  or SSH keys.
- Multi-repository support, to separate projects or business domains: one PromptFS
  instance per repository (D-024).

**Versioning and immutability**
- Resolve prompts by Git commit, Git tag (SemVer) or branch.
- Change history inherited directly from Git tooling.

**Canary deployments and dynamic routing**
- Deployment strategies declared in `.promptfs/deployments.yaml` inside the repo.
- Stochastic traffic split (e.g. 90% to `tags/v1.0.0`, 10% to `heads/feature-prompt`).
- Environment isolation (`production`, `staging`, `canary`).

**Playground and web UI**
- Visual prompt editor with syntax highlighting (Markdown / Jinja2).
- Diff viewer across commits and tags.
- Live execution panel against LLM providers (OpenAI, Anthropic, Azure OpenAI, Ollama),
  with the provider key supplied per request and never persisted.
- Propose changes from the PromptFS UI as a branch, a commit and a pull request, under the
  signed-in user's own Git provider identity (D-027).

**SDKs and REST API**
- SDKs sync a bundle and resolve + render **in the consuming application's own process**,
  linking the same compiled core the server runs. No network call per prompt, no render
  variables crossing the network, and the app keeps working while PromptFS is down.
- A REST API (`POST /v1/prompts/render`) serves the same result server-side for Studio,
  `curl`, the CI eval runner and languages without an SDK.
- Python first (`pip install promptfs`), TypeScript after.

### 2.2 Non-functional (NFR)

**DB-less architecture**
- Zero persistent state in the application; the single source of truth is the Git repo.
- Stateless restarts and instances (Kubernetes / container deployment).
- One active instance per repository, restarted by its orchestrator on failure (D-028).
  Applications stay available through an outage because of the SDK's resident bundle, not
  because of server replicas.

**Ultra-low latency**
- Server render endpoint: **< 5 ms p99** for cached prompts.
- In-process SDK render: microseconds. It performs no I/O — the bundle is already resident
  and the Jinja AST already compiled.

**Efficient resource use**
- **< 50 MB** RSS at idle; final distributable is a single static binary.
- The Python wheel carries `promptfs-core` only — no libgit2, no async runtime.

**Security**
- No local storage of secrets or API keys; everything via environment variables or
  injected headers.
- Render variables are caller data and are treated as potentially PII: never logged, never
  echoed in an error message, never attached to a span. On the SDK path they never leave
  the customer's process at all.
- Bundle and render endpoints require a bearer token scoped to the environment. With no
  authentication configured the server listens on loopback only (D-026).
- Webhook deliveries are verified against a configured secret; without one the webhook
  route does not exist and the server polls (D-025).

**Availability**
- PromptFS is never a hard runtime dependency of a consuming application. An SDK with a
  bundle keeps serving indefinitely while the server is unreachable.

## 3. Stack

`promptfs-core` is the load-bearing artifact: one compiled implementation of parse,
compile, render and route, linked by every consumer.

```
                  ┌──────────────────────────────┐
                  │        promptfs-core         │
                  │  parse · compile · render    │
                  │  route · no git2 · no tokio  │
                  └──────────────┬───────────────┘
                                 │ linked by all three
        ┌────────────────────────┼────────────────────────┐
        │                        │                        │
┌───────▼────────┐   ┌───────────▼───────────┐   ┌────────▼────────┐
│ promptfs-server│   │ promptfs-py           │   │ Studio preview  │
│ + git2 + axum  │   │ pyo3 wheel, inside    │   │ core in wasm,   │
│ + moka + SSE   │   │ the customer process  │   │ in the browser  │
└────────────────┘   └───────────────────────┘   └─────────────────┘
```

The CI eval runner is a fourth consumer and links the core the same way. That is
invariant 8: a prompt file becomes a string in exactly one place in this product.

### 3.1 Core (`promptfs-core`)

| Component | Crate | Rationale |
|---|---|---|
| Template engine | `minijinja` | By Armin Ronacher (author of Jinja2 and Flask). No heavy dependencies, ultra-fast, safe, Jinja2-syntax compatible. Owns the `Environment`, with autoescape explicitly off. |
| Serialization | `serde`, `serde_yaml_ng`, `serde_json` | The de facto serialization standard in Rust. |

Nothing else. The core ships inside customers' processes, so every dependency added here is
a dependency added to their application. It is synchronous, does no I/O and needs no runtime.

### 3.2 Server (`promptfs-server`)

| Component | Crate | Rationale |
|---|---|---|
| Runtime & HTTP server | `tokio` + `axum` | Industry standard in Rust for high-performance async APIs, built by the Tokio team. |
| Git integration | `git2` (libgit2 bindings) | libgit2 is the C library behind GitHub, GitLab and Azure Repos — the most mature, fast and stable way to work with Git repos without invoking the CLI. |
| In-memory caching | `moka` | Inspired by Caffeine (Java). High-performance concurrent cache with TinyLFU eviction, thread-safe, optimized for read-heavy loads. |
| UI distribution | `rust-embed` | Embeds the React frontend's static HTML/JS/CSS into the Rust binary at compile time. |

### 3.3 Python bindings (`promptfs-py`)

| Component | Choice | Rationale |
|---|---|---|
| Bindings | `pyo3` + `maturin` | The standard path for shipping a Rust core as a Python extension — the same one `pydantic-core`, `ruff`, `polars` and `tokenizers` take. |
| ABI | `abi3` | One wheel per platform covers Python 3.8+, cutting release artifacts from ~25 to ~5. |
| Wheel CI | `cibuildwheel` | manylinux x86_64 · manylinux aarch64 · macOS arm64 · macOS x86_64 · Windows x86_64. |

### 3.4 Frontend (PromptFS Studio)

| Component | Technology | Rationale |
|---|---|---|
| UI library | React 18 + TypeScript | The most widely used, documented and stable combination in the web ecosystem. |
| Build tool | Vite | Ultra-fast build tool, de facto standard for React SPAs. |
| Styling | Tailwind CSS | Utility-class styling, heavily optimized in production builds. |
| Code editor | Monaco Editor (`@monaco-editor/react`) | The editing core of VS Code. Syntax highlighting, side-by-side visual diffs, autocompletion. |
| State & server sync | TanStack Query + Zustand | React Query for API sync/cache, Zustand for lightweight global UI state. |

## 4. Managed repository structure

Every project managed by PromptFS follows this layout:

```
my-prompt-repository/
├── .promptfs/
│   └── deployments.yaml       # Canary strategies and branch/tag assignment
├── prompts/
│   ├── support/
│   │   ├── classifier.prompt.md
│   │   └── classifier.eval.yaml
│   └── sales/
│       └── email_writer.prompt.md
└── config.yaml                # Global provider options (OpenAI, Anthropic)
```

### Prompt format — `classifier.prompt.md`

The same file the core's tests parse (`CLASSIFIER` in `parse.rs`) and the `fixture-repo`
skill writes into Git, so the three cannot drift apart unnoticed.

```markdown
---
name: classifier
description: Classifies user intent for technical support.
model: gpt-4o-mini
temperature: 0.1
inputs:
  - user_input
  - customer_tier
---
You are a classification assistant.
Customer tier: {{ customer_tier }}

Classify the following and answer in JSON:
{{ user_input }}
```

### Routing format — `.promptfs/deployments.yaml`

```yaml
deployments:
  support/classifier:
    environments:
      production:
        strategy: canary
        targets:
          - ref: "tags/v1.2.0"
            weight: 85
          - ref: "heads/canary-gpt4o"
            weight: 15
      staging:
        targets:
          - ref: "heads/main"
            weight: 100
```

This file is read at the **control ref** — `heads/main` unless configured — while each
target's prompt source is read at the target's own ref (D-022). The environments that exist
are the ones the file names, and a prompt with no entry for an environment is not served
there: deploying is always explicit (D-023).

## 5. Engine architecture (DB-less core)

### 5.1 Read pipeline — in-process (SDK, the hot path)

Once, at application startup:

1. **Bundle load** — the SDK reads a build-time snapshot if one is vendored, then calls
   `GET /v1/bundle?env=production&format=1` with its environment-scoped token (D-026). It
   compiles every prompt source in the bundle into a `minijinja` AST and holds the result
   resident.
2. **Subscribe** — the SDK opens the SSE stream so the server can push change notices.

Per call, with no I/O whatsoever:

3. **Canary evaluation** — the router picks a target from the bundle's strategy, seeded by
   the routing key (invariant 4).
4. **Render** — the already-compiled AST for that target renders the variables.
5. **Return** — body plus `model`, `temperature`, `resolved_ref`, `commit` and
   `bundle_version`.

### 5.2 Read pipeline — server-side (`POST /v1/prompts/render`)

For Studio, `curl`, the CI eval runner and languages with no SDK.

1. **Request** — the caller posts the prompt name (`support/classifier`), the environment
   (`production`), the input variables and a routing key, with a bearer token scoped to that
   environment (D-026).
2. **Canary evaluation** — the Router (in `promptfs-core`) reads the strategy from the
   in-memory cache of `.promptfs/deployments.yaml` and picks a target. An environment the
   file does not name, or a prompt with no entry in it, is a 404 naming what was not found
   (D-022, D-023).
3. **Source lookup**
   - Look up the compiled prompt (frontmatter plus Jinja AST) for that target in the Moka
     cache.
   - **Cache miss:** `git2` reads the blob from the local bare Git repo (kept in sync via
     webhooks, polling as fallback), the core splits the frontmatter and compiles the Jinja
     body into a `minijinja` AST. The result is cached.
4. **Render & respond** — the same core renders, and the response carries the same fields
   as the in-process path.

Steps 2 and 4 are literally the same code as steps 3 and 4 above.

### 5.3 Consumer contract

Public and effectively frozen once a wheel is on PyPI.

#### The bundle

`GET /v1/bundle?env=<env>&format=<n>` returns the ruleset for one environment together with
the source of every **active target** in it. Whole environment at once — typically under
200 KB — rather than lazily per prompt: it makes cold start trivially correct and makes the
build-time snapshot a single file. An optional namespace filter exists for large repos.
The request carries a bearer token scoped to that environment (D-026), and an environment
`deployments.yaml` does not name is a 404 (D-022).

```jsonc
{
  "format": 1,
  "env": "production",
  "version": "sha256:…",          // content hash; served as ETag
  "from_commit": "a1b2c3d",       // the control ref's commit (D-022)
  "prompts": {
    "support/classifier": {
      "strategy": "weighted",
      "targets": [
        { "ref": "tags/v1.2.0", "weight": 90, "commit": "…", "source": "---\nname:…" },
        { "ref": "heads/exp",   "weight": 10, "commit": "…", "source": "---\nname:…" }
      ]
    },
    "support/broken": {
      "error": { "file": "prompts/support/broken.prompt.md", "line": 12,
                 "message": "unexpected end of template" }
    }
  }
}
```

**Validated before it ships.** The server compiles every target while building the bundle.
Anything that fails to compile travels as `error` with no `source`; the SDK raises only if
that specific prompt is used, so one broken prompt cannot take down the rest. Studio and the
webhook response surface it immediately.

The SDK compiles the source again on load. That does not double the risk, because it is the
same compiler: if it compiled on the server it compiles in the SDK, byte for byte. With a
different engine per language the server's validation would be advisory only — this is the
concrete payoff of invariant 8.

**Additive format.** Within a major version the bundle only gains fields; nothing is removed
or repurposed, and SDKs ignore fields they do not recognise. The `format` integer on request
and response exists for the day a real break is needed: the client asks for the highest it
understands, the server serves the highest both support. A wheel already on PyPI cannot be
made to upgrade.

#### Freshness

```
git push → webhook → server invalidates moka + fetches the bare repo
                   → server rebuilds the bundle
                   → SSE: push to connected SDKs
                   → SDK downloads the new bundle and swaps it atomically
```

- **The webhook is a hint.** The server fetches and compares its refs itself; deliveries are
  verified against a secret and coalesce into one fetch, and without a secret the route does
  not exist (D-025).
- **Fallback** where SSE cannot pass (proxies, firewalls): `GET /v1/bundle` with
  `If-None-Match` every 30 s.
- **Atomic swap.** Never a half-updated state. A call that started on bundle v1 finishes
  on v1.
- **Published SLA, for our hop only:** p99 < 2 s from *webhook receipt* to connected SDKs
  serving the new version. The `git push → webhook delivery` leg belongs to the Git
  provider and is documented as out of scope — GitHub makes no commitment there. The SLA is
  exact because one instance serves each repository, so the instance that receives the
  webhook is the one every SDK is connected to (D-028).

#### Cold start and degradation

Three layers, in order:

1. **Build-time snapshot.** `promptfs pull --env production -o promptfs.bundle.json` in CI,
   vendored into the image. Instant, offline startup. `pull` is a subcommand of the existing
   `promptfs-server` binary running in HTTP client mode — it talks to a PromptFS instance,
   with a token scoped to the environment it pulls (D-026), and needs no `git2`, so it is not
   a fourth crate. The SDK reads the file and hands the bytes
   to the core, which never touches a filesystem (invariant 6).
2. **Live sync.** Bundle fetched at startup, kept fresh over SSE.
3. **Last known good.** If the connection drops, the last bundle keeps serving
   indefinitely. It never expires to empty.

**Policy: the SDK never fails a render because PromptFS is unreachable.** It fails only when
it has no bundle at all — and that failure happens when `Client()` is constructed, at
startup, not on the first render at 3 a.m.

An optional `max_staleness=` exists, defaulting to `None`. Bundle age is always exposed as
a metric.

#### Routing key (invariant 4)

The SDK derives it, in order:

1. Explicit `routing_key=` argument
2. **The active OpenTelemetry trace id**, if a trace context exists
3. A contextvar set by the application (`promptfs.set_routing_key()`)
4. A random UUID, with a warning logged once — never silently

The trace id is the right default: a retry inside the same trace keeps the trace id and so
resolves to the same version, and trace attribution falls out for free.

#### Result shape

```python
p = client.render("support/classifier", user_input=msg, customer_tier="gold")

p.body            # the final text
p.model           # "gpt-4o-mini"
p.temperature     # 0.1
p.resolved_ref    # "tags/v1.2.0"   ┐
p.commit          # "a1b2c3d"       ├ attribution triple
p.bundle_version  # "sha256:…"      ┘
p.source          # raw template — escape hatch, no guarantees past this point
```

Without `resolved_ref` and `commit` the canary split is invisible from the application and
traces cannot be attributed to a prompt version.

### 5.4 Write pipeline (from the UI)

1. The user signs in to Studio with their Git provider's OAuth and edits a prompt in the
   Monaco Editor. What they may see or change is what the provider lets them see or change
   in the repository (D-027).
2. On **Propose change**:
   - The frontend sends the changes and a message to the backend.
   - The backend creates a branch, a commit and a pull request through the provider's API —
     GitHub first — with the signed-in user's own token. Branch protection and review apply
     as they would to any pull request.
   - Once the pull request is merged, the Git provider fires a webhook back to PromptFS.
   - The backend invalidates the affected `moka` entries, fetches the bare Git repo on
     ephemeral disk, rebuilds the affected bundles and pushes them to connected SDKs: the
     same path as a change pushed from a terminal. The server learns about its own writes
     the way it learns about anyone else's.

### 5.5 Provider keys in Studio

The live-execution panel sends the user's provider key in a **request header**. The server
uses it for that one call and nothing else: never written, never logged, never cached.
Invariant 2 already permits exactly this shape ("injected headers").

Rejected: calling the provider directly from the browser. Anthropic allows it with
`anthropic-dangerous-direct-browser-access`, but it exposes the key to every script on the
page and drags in per-provider CORS handling. The ephemeral header is cleaner.

**In production PromptFS never calls an LLM.** It resolves and renders; the application owns
the provider call.

## 6. Implementation phases

**Phase 1 — Workspace and core engine**
- Split the workspace: `promptfs-core` + `promptfs-server`. Doing this now costs ~20 lines
  of `Cargo.toml`; doing it in phase 4 means prising `git2` and `tokio` back out of the core.
- `promptfs-core`: frontmatter parsing, `minijinja` compile and render, autoescape off.
- `promptfs-server`: read bare Git repositories with `git2`, basic REST server with `axum`.

**Phase 2 — Router, canary, caching and bundle**
- Read and deserialize `.promptfs/deployments.yaml`.
- Weighting logic for canary deployments — **in the core**, so both paths run it.
- Integrate the `moka` concurrent cache.
- Bundle build with per-target validation, ETag, and SSE push on invalidation.

**Phase 3 — PromptFS Studio UI (React + Monaco)**
- Frontend in React + Vite + Tailwind CSS.
- Monaco Editor integration with diff view.
- Live prompt-testing module against LLM providers, key by request header.
- Preview runs `promptfs-core` compiled to wasm — no round trip, guarantee intact.
- Embed the static UI into the Rust binary via `rust-embed`.

**Phase 4 — SDKs and CI/CD ops**
- `promptfs-py`: pyo3 + abi3, ~5 wheels per release via maturin + cibuildwheel.
- `sdks/python`: idiomatic wrapper — bundle sync, SSE, routing-key derivation, last-good.
- `promptfs pull` subcommand on the server binary, for the build-time snapshot.
- GitHub Actions / Azure Pipelines to run `eval.yaml` in CI/CD.
- **Phase 4b:** TypeScript SDK, core via wasm.

## Deferred and non-goals

- **gRPC is deferred.** The original spec mentioned "REST/gRPC"; only REST is planned.
- **PromptFS is never an LLM gateway.** Proxying provider calls would buy automatic latency,
  cost and version attribution, but it would mean custodying provider keys in production
  (against invariant 2) and making PromptFS a mandatory hard dependency (against the
  availability NFR). The application makes its own provider call.
- **No `to_anthropic()` / `to_openai()` helpers in the SDKs.** `body`, `model` and
  `temperature` are returned; wiring them to a provider is the application's job. Provider
  request shapes change on someone else's schedule.
- **Lazy per-prompt bundle fetching** is not built. Whole-environment bundles are simpler
  and correct on cold start; the namespace filter covers large repos until measurements say
  otherwise.
- **Server replicas are deferred** (D-028). Without a shared database, replicas lag each
  other between polls, and a retry that lands on a lagging one breaks invariant 4. When they
  come they need routing-key affinity at the load balancer and an SDK that never swaps to an
  older bundle.
