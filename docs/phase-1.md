# Phase 1 — task board

Working document for the phase-1 split described in [`AGENTS.md`](../AGENTS.md).
Delete it when phase 1 closes; the invariants stay in AGENTS.md, the tasks do not.

Live status is the [phase 1 milestone](https://github.com/netsirius/promptfs/milestone/1).
Task numbers here predate the issues and are offset by one — use this table:

| Task | Issue | Status |
|---|---|---|
| 1 · Workspace skeleton | — (before the tracker existed) | done |
| 2 · `PromptError` and `PromptMeta` | [#1](https://github.com/netsirius/promptfs/issues/1) for the remaining variants | done |
| 3 · Frontmatter split | [#2](https://github.com/netsirius/promptfs/issues/2) | done |
| 4 · The compiled prompt | [#3](https://github.com/netsirius/promptfs/issues/3) | todo — next |
| 5 · Undefined and missing-input policy | [#4](https://github.com/netsirius/promptfs/issues/4) | todo |
| 6 · Router | [#5](https://github.com/netsirius/promptfs/issues/5) | todo |
| 7 · git2 | [#6](https://github.com/netsirius/promptfs/issues/6) | todo |
| 8 · axum | [#7](https://github.com/netsirius/promptfs/issues/7) | todo |
| 9 · Fixtures and tests | [#8](https://github.com/netsirius/promptfs/issues/8) | todo |

## Protocol

**A task marked `Owner: Héctor` is scaffolded to the edge and then stopped.** The agent
writes the types, the signatures, the doc comments and a failing test, leaves exactly one
`TODO(human)` in the gap, and does not fill it in. It then stops and says so.

A task marked `Owner: agent` is implemented end to end, its status flipped to `done`, and
committed.

Status values: `todo` · `scaffolded` (gap waiting on Héctor) · `done`.

**Why the split is where it is.** Gaps are placed one per *concept*, not one per task. Once
the ownership lesson lands in task 4, the `Arc<AppState>` in task 8 is the same pattern and
the agent writes it. If a gap costs more time than it teaches, collapse it — say so and the
agent finishes the task.

---

### 1 · Workspace skeleton

**Owner:** agent, with a small gap for Héctor  ·  **Status:** done
**Crate:** — · **Invariants:** 8 · **Depends on:** —

Root `Cargo.toml` as a workspace, `crates/promptfs-core` and `crates/promptfs-server`,
`cargo check` green on both.

**Gap:** you write `crates/promptfs-core/[dependencies]` — the whole list — from invariant 8
alone, without looking at what the server needs.

**Guidance.** The question behind it: *this crate is going to be compiled into a stranger's
Python process. What is it allowed to link?* Work from what the core actually does — take
bytes, parse YAML frontmatter, compile a Jinja template, render it, pick a weighted target —
and add nothing for what it might need later. `check-invariants.sh` rejects `git2`, `tokio`,
`axum`, `hyper`, `reqwest`, `async-std` and `moka` here, so a wrong answer fails loudly.

**Done when:** `cargo check --workspace` is green and the hook passes.

---

### 2 · `PromptError` and `PromptMeta`

**Owner:** Héctor  ·  **Status:** done
**Crate:** `promptfs-core` · **Invariants:** 2 · **Depends on:** 1

The error type every core function returns, and the struct the frontmatter deserializes into.

**Gap:** the variants of `PromptError`.

**Guidance.** `thiserror` gives you `#[derive(Error)]` and `#[error("…")]` on each variant;
the format string is what `Display` prints. The design questions:

- Which failures are actually distinct? Missing file, unterminated frontmatter, malformed
  YAML, missing required key, template compile error, missing render input. Each one the
  caller handles differently is a variant; each one they do not is noise.
- **Invariant 2 lives here.** A missing-input error names *which* input was missing and never
  its value. Once the variant is `MissingInput { name: String }` rather than
  `MissingInput { name: String, got: Value }`, no `?` downstream can leak a value — the type
  makes it impossible rather than the reviewer catching it.
- Carry `file` and `line` where the contract promises a 4xx with a location.

**Rust you will meet:** `#[from]` on a variant field makes `?` convert automatically — that
is the whole trick behind `serde_yaml_ng::Error` turning into a `PromptError` at a `?`.

**Done when:** the failing test in `error.rs` passes and every variant's `Display` names a
location or an input name without printing a value.

---

### 3 · Frontmatter split

**Owner:** Héctor  ·  **Status:** done
**Crate:** `promptfs-core` · **Invariants:** 6 · **Depends on:** 2

Split a `.prompt.md` into its YAML frontmatter and its Jinja body.

**Gap:** `fn split_frontmatter(src: &str) -> Result<(&str, &str), FormatError>`

`FormatError`, not `PromptError`: this function never sees the file path, so wrapping the error
into `PromptError::InvalidFormat` belongs to whoever read the bytes.

**Guidance.** Deliberately a warm-up for task 4: the signature returns two `&str` **borrowed
from the input**, no `String`, no allocation. Invariant 6 says the core does not allocate per
render beyond the output — this is where that habit starts.

- The file opens with `---` on line 1 and the block ends at the next `---` on its own line.
- Slicing: `&src[a..b]` where `a` and `b` are byte offsets, not character counts. `find` and
  `strip_prefix` return byte offsets already, which is why they compose.
- An unterminated block is an error with a line number, not a panic.

**Trap:** `&src[a..b]` panics if a boundary is not on a UTF-8 character boundary. Frontmatter
is ASCII delimiters so you will not hit it here, but the reflex — Rust strings index by byte
and refuse to split a character — is worth internalising now.

**Done when:** the round-trip test passes, and the signature still has no `String` in it.

---

### 4 · The compiled prompt — ownership lesson

**Owner:** Héctor, guided step by step  ·  **Status:** todo
**Crate:** `promptfs-core` · **Invariants:** 3, 6, 8 · **Depends on:** 3

Own the minijinja `Environment`, compile a prompt source to an AST once, and store the
result so it can be rendered many times.

**Gap:** the `CompiledPrompt` struct's fields and the constructor's body.

**Guidance — read this before writing anything.**

This is the hard one, and it is hard for a reason worth understanding. `minijinja`'s
`Environment<'source>` **borrows** the template source you hand it. So the obvious struct:

```rust
struct CompiledPrompt {
    source: String,
    env: Environment<'?>,   // borrows `source` — from itself
}
```

is a self-referential struct, which Rust does not allow. There is no lifetime you can write
in that `'?` that means "borrows the field next to me". The compiler will tell you it cannot
infer an appropriate lifetime, or that `source` does not live long enough, and both messages
are the same fact wearing different clothes.

Two ways out, and both are worth understanding before you pick:

1. **Give the environment owned templates.** Check whether the crate version exposes
   `add_template_owned` — an owned source has no lifetime to borrow, so the struct becomes
   `Environment<'static>`. `'static` here does **not** mean "lives until the program ends";
   it means "borrows nothing", which is the only reason it can be stored anywhere.
2. **Share the source behind a pointer.** `Arc<str>` gives you cheap clones (a refcount
   bump, not a copy) and lets several holders keep the same bytes alive. This is the shape
   the `moka` cache wants on the server, since a cache hit should hand back a pointer rather
   than a string.

**First:** open the actual `minijinja` docs for the version we pin and confirm what exists.
Do not take the API names above on trust — check them. That reflex matters more than the
answer.

**Invariant 3 is also yours here:** autoescape is off, set explicitly on the environment, in
the core, once. Not in the server, not in the SDK. minijinja picks escaping from the template
*name's* extension, so a prompt registered under a name ending `.html` would silently start
turning `&` into `&amp;`.

**Done when:** a prompt compiles once and renders twice with different variables; the
environment is constructed in exactly one place in the workspace; the autoescape test passes.

---

### 5 · Undefined and missing-input policy

**Owner:** Héctor decides, agent implements  ·  **Status:** todo
**Crate:** `promptfs-core` · **Invariants:** 2, 6 · **Depends on:** 4

What happens when a variable the template uses was not supplied.

**The decision.** minijinja has `UndefinedBehavior::Lenient` (undefined renders empty),
`Strict` (undefined is an error) and `Chainable`. Separately, our frontmatter declares
`inputs:`, which is *our* contract, not the engine's.

The trade-off is real in both directions: `Lenient` means a typo'd variable ships a mutilated
prompt to the model and nobody notices — the request succeeds. `Strict` means a template that
references an optional field breaks a production render for a caller who did nothing wrong.
And declared `inputs` can be validated before rendering at all, which produces a better error
than either.

Tell the agent which, and why. It writes the code.

---

### 6 · Router — weighted, seeded, deterministic

**Owner:** Héctor  ·  **Status:** todo
**Crate:** `promptfs-core` · **Invariants:** 4 · **Depends on:** 2

Given a ruleset, a prompt, an environment and a routing key, pick a target ref.

**Gap:** the bucketing function.

**Guidance.** Less Rust than the others, more correctness. The property: the same routing key
must always select the same target, and across many distinct keys the selection must land on
the declared weights.

- Hash the key to a number, map it into `0..sum_of_weights`, walk the targets accumulating
  until you pass it. `std::collections::hash_map::DefaultHasher` is in the standard library —
  but read what its documentation guarantees about stability across Rust releases before you
  rely on it, because "the same key resolves to the same version" is a promise that has to
  survive a compiler upgrade.
- Weights are declared per environment and need not sum to 100.
- No `rand()`, in any form. `check-invariants.sh` rejects it.

**The trap this task exists for:** an aggregate distribution test passes with unseeded
`rand()` *and* with a correct implementation. Only the stability assertion — same key, same
ref, repeated — separates them. Write that assertion first.

**Done when:** stability holds over repeated calls; a 90/10 split lands within tolerance over
10k distinct keys; a non-50/50 split is covered so off-by-one bucketing is visible.

---

### 7 · git2 — bare repo reads and ref resolution

**Owner:** Héctor, agent writes everything around it  ·  **Status:** todo
**Crate:** `promptfs-server` · **Invariants:** 1 · **Depends on:** 1

Open a bare repo, resolve a fully-qualified ref to a commit, read a blob by path.

**Gap:** `fn resolve_ref(repo: &Repository, r: &str) -> Result<Commit, ServerError>`

**Guidance.** This is the Git-internals lesson. libgit2's object model: a ref points at an
object; an object is a commit, tree, blob or **tag**; a tree maps paths to entries; a blob is
bytes. Reading `prompts/support/classifier.prompt.md` at `tags/v1.2.0` is: ref → commit →
tree → entry → blob.

Three traps, in the order you will hit them:

1. **Our contract says `tags/v1.2.0`. Git wants `refs/tags/v1.2.0`.** The managed-repo
   contract deliberately uses the short-but-qualified form; the mapping to a real ref name is
   ours to do, and doing it wrong fails only for tags, not branches.
2. **Annotated tags are not commits.** A lightweight tag points straight at a commit; an
   annotated tag points at a *tag object* that points at the commit. Resolving one and not the
   other is the single most common libgit2 bug, and a fixture that only creates branches will
   never show it. Look at `peel_to_commit`.
3. **Bare repos have no working tree.** Anything path-based that reads from disk rather than
   from the object database will work locally and fail in production.

**Done when:** the same function resolves a branch, a lightweight tag, an annotated tag and a
raw SHA, with a fixture for each.

---

### 8 · axum — the render endpoint

**Owner:** Héctor writes the handler, agent writes the wiring  ·  **Status:** todo
**Crate:** `promptfs-server` · **Invariants:** 2, 6 · **Depends on:** 4, 6, 7

`POST /v1/prompts/render` — request in, rendered result out.

**Gap:** the handler body.

**Guidance.** The async lesson. The agent will have set up `Router`, `AppState` and the
request/response types; you write what happens between them.

- Extractors: `State(state): State<Arc<AppState>>` and `Json(req): Json<RenderRequest>` in the
  handler's arguments. The argument list *is* the parsing — that is the axum idea worth taking
  away.
- **`git2` is blocking, and `Repository` is `Send` but not `Sync`.** Verify that against the
  version we pin, because it decides the shape of everything: a blocking call inside an async
  handler stalls the executor thread, and a type that is not `Sync` cannot simply live in an
  `Arc` shared across handlers. `tokio::task::spawn_blocking` is the usual answer to the
  first half. Work out the second half deliberately rather than reaching for a `Mutex` on
  reflex — a mutex around every repo read serialises the whole server.
- Invariant 6 says a cache hit does no Git I/O at all, so the interesting path here should
  never reach `git2`.
- Invariant 2: the response carries `resolved_ref` and `commit`; the logs carry neither the
  variables nor their values.
- The server binds `127.0.0.1` only. Authentication arrives in phase 2, and a server with none
  configured must not listen publicly (D-026) — so this endpoint needs no auth code yet.

**Done when:** a request renders end to end against a fixture repo, and a malformed prompt
returns a 4xx naming the file and line — never a 500, never a panic.

---

### 9 · Fixtures and tests

**Owner:** shared  ·  **Status:** todo
**Crate:** both · **Invariants:** all · **Depends on:** 7

Fixture repos built with `git2` in-test, per the `fixture-repo` skill. Run `/contract-check`
against each generated fixture and `git2-fixture-reviewer` over the test modules.

Router unit tests live in `promptfs-core` and need no repo at all — the router takes a
deserialized ruleset. Only the path from a real `.promptfs/deployments.yaml` in a tree to a
resolved ref belongs in a Git fixture.
