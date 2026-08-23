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
- Multi-repository support, to separate projects or business domains.

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
- Live execution panel against LLM providers (OpenAI, Anthropic, Azure OpenAI, Ollama).
- Commit & Push straight to a Git branch from the PromptFS UI.

**SDKs and REST/gRPC API**
- Low-latency REST API for resolving and rendering compiled prompts.
- Official Python and TypeScript SDKs with client-side TTL cache and resilient fallback.

### 2.2 Non-functional (NFR)

**DB-less architecture**
- Zero persistent state in the application; the single source of truth is the Git repo.
- Stateless restarts and instances (Kubernetes / container deployment).

**Ultra-low latency**
- Prompt resolution endpoint: **< 5 ms p99** for cached prompts.

**Efficient resource use**
- **< 50 MB** RSS at idle; final distributable is a single static binary.

**Security**
- No local storage of secrets or API keys; everything via environment variables or
  injected headers.

## 3. Stack

```
┌─────────────────────────────────────────────────────────────┐
│                 FRONTEND (PromptFS Studio)                  │
│         React 18 + TypeScript + Vite + Tailwind CSS         │
│          Monaco Editor + TanStack Query + Zustand           │
└──────────────────────────────┬──────────────────────────────┘
                               │ API REST / SSE
┌──────────────────────────────▼──────────────────────────────┐
│                BACKEND (PromptFS Core Engine)               │
│                        Rust (Axum)                          │
│                                                             │
│  ┌───────────────────────┐      ┌────────────────────────┐  │
│  │     git2 (libgit2)    │      │       MiniJinja        │  │
│  │ (Bare Git Repos VFS)  │      │ (AST Prompt Renderer)  │  │
│  └──────────┬────────────┘      └───────────┬────────────┘  │
│             │                               │               │
│  ┌──────────▼────────────┐      ┌───────────▼────────────┐  │
│  │      Moka Cache       │      │   rust-embed (Static)  │  │
│  │ (In-memory Concurrent)│      │  (Single Binary Asset) │  │
│  └───────────────────────┘      └────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### 3.1 Backend (Rust core)

| Component | Crate | Rationale |
|---|---|---|
| Runtime & HTTP server | `tokio` + `axum` | Industry standard in Rust for high-performance async APIs, built by the Tokio team. |
| Git integration | `git2` (libgit2 bindings) | libgit2 is the C library behind GitHub, GitLab and Azure Repos — the most mature, fast and stable way to work with Git repos without invoking the CLI. |
| Template engine | `minijinja` | By Armin Ronacher (author of Jinja2 and Flask). No heavy dependencies, ultra-fast, safe, 100% Jinja2-syntax compatible. |
| In-memory caching | `moka` | Inspired by Caffeine (Java). High-performance concurrent cache with TinyLFU eviction, thread-safe, optimized for read-heavy loads. |
| Serialization | `serde`, `serde_yaml`, `serde_json` | The de facto serialization standard in Rust. |
| UI distribution | `rust-embed` | Embeds the React frontend's static HTML/JS/CSS into the Rust binary at compile time. |

### 3.2 Frontend (PromptFS Studio)

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

```markdown
---
name: classifier
description: Clasifica la intención del usuario en soporte técnico.
model: gpt-4o-mini
temperature: 0.1
inputs:
  - user_input
  - customer_tier
---
Eres un asistente de clasificación.
Cliente Tier: {{ customer_tier }}

Analiza el siguiente texto y responde en JSON:
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

## 5. Engine architecture (DB-less core)

### 5.1 Read pipeline

1. **Request** — the SDK or client app calls `POST /v1/prompts/render` with the prompt
   name (`support/classifier`), the environment (`production`) and the input variables.
2. **Canary evaluation** — the Router module reads the strategy from the in-memory cache
   of `.promptfs/deployments.yaml` and picks a version stochastically (e.g. `tags/v1.2.0`).
3. **VFS resolution**
   - Look up the parsed AST and template in the Moka cache.
   - **Cache miss:** `git2` reads the object from the local bare Git repo (kept in sync via
     webhooks/polling), parses the YAML frontmatter, and compiles the Jinja2 template into
     a `minijinja` AST. The result is cached.
4. **Render & respond** — `minijinja` renders the variables and the model configuration
   (`model`, `temperature`) is injected into the JSON response.

### 5.2 Write pipeline (from the UI)

1. The user edits a prompt in the Monaco Editor inside PromptFS Studio.
2. On **Commit & Push**:
   - The frontend sends the changes and commit message to the backend.
   - The backend calls the GitHub / Azure Repos REST API (or performs an atomic push with
     `git2`) to create a new commit / branch / PR.
   - The Git provider fires a webhook back to PromptFS.
   - The backend invalidates the matching Moka cache entry and forces a fetch of the bare
     Git repo on ephemeral disk.

## 6. Implementation phases

**Phase 1 — Rust core engine (CLI & basic API)**
- Read bare Git repositories with `git2`.
- Integrate `minijinja` for variable rendering.
- Basic REST server with `axum`.

**Phase 2 — Router, canary and caching**
- Read and deserialize `.promptfs/deployments.yaml`.
- Integrate the `moka` concurrent cache.
- Implement weighting logic for canary deployments.

**Phase 3 — PromptFS Studio UI (React + Monaco)**
- Frontend in React + Vite + Tailwind CSS.
- Monaco Editor integration with diff view.
- Live prompt-testing module against LLM providers.
- Embed the static UI into the Rust binary via `rust-embed`.

**Phase 4 — SDKs and CI/CD ops**
- Lightweight SDKs for Python (`pip install promptfs`) and TypeScript (`npm install promptfs`).
- GitHub Actions / Azure Pipelines to run `eval.yaml` in CI/CD.

## Open decisions

Made while writing this up, not present in the original spec — revisit if wrong:

- **Autoescape is disabled in minijinja.** Prompt bodies are not HTML; extension-based
  autoescape defaults would corrupt them silently.
- **Canary weighting is seeded per request** from a caller-supplied key, so a retry
  resolves to the same version and traces stay attributable.
- **gRPC is deferred.** The spec mentions "REST/gRPC"; only REST is planned through phase 4.
