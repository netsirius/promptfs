# Decision log

Append-only. One entry per decision that had a **rejected alternative someone will propose
again**. A rule already stated with its reason in [`AGENTS.md`](../AGENTS.md) does not need an
entry here; this file exists for what was considered and discarded, because that is the part
that gets re-litigated.

Every entry carries a *would invalidate this* line. A decision without one is dogma.

Scope: PromptFS the product — architecture, technology, contracts. Nothing about process.

---

## D-001 · No database, ever

**Date:** 2026-08-20 · **Status:** accepted · **Touches:** invariant 1

Git is the only source of truth. The server's bare clones and the SDKs' bundles are derived
caches, rebuildable from Git alone and disposable.

**Rejected:** Postgres or SQLite for metadata, render history or cache persistence. It is the
obvious reflex the first time something needs to be "remembered".

**Why:** the product is *Git-native prompt management*. A database re-creates the thing teams
already have — a second source of truth that drifts from the repo, needs migrations, backups
and a schema, and turns a stateless binary into an operational burden.

**Would invalidate this:** a required feature genuinely unrepresentable in Git. Analytics and
audit are not — they belong in the caller's observability stack, not in ours.

---

## D-002 · `minijinja`, and autoescape explicitly off

**Date:** 2026-08-20 · **Status:** accepted · **Touches:** invariant 3

**Rejected:** `tera` and `handlebars` for the engine. Rejected separately: relying on
minijinja's extension-based autoescape defaults rather than setting the callback.

**Why:** minijinja is by Jinja2's own author, has no heavy dependencies and is small enough to
ship inside a customer's process. On autoescape — these are LLM prompts, not HTML. Escaping
turns `&` into `&amp;` in a prompt body, silently, and passes every naive test. The default
happens to be correct for `.prompt.md` names, which is exactly why it must be set explicitly:
a name derived from repo input that ends `.html` would flip it without a line of code changing.

**Would invalidate this:** nothing plausible. If minijinja were abandoned, the AST boundary is
narrow enough to swap, but the autoescape decision would survive the swap unchanged.

---

## D-003 · The SDK renders in the consuming application's process

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** architecture.md §5.1, §5.3

The SDK syncs a bundle at startup and does routing plus rendering in-process. The server's
`POST /v1/prompts/render` remains for Studio, `curl`, CI and languages with no SDK.

**Rejected:** a thin HTTP client SDK, with the server rendering on every call — which is what
the original spec described. Also rejected: shipping the thin client first and migrating later.

**Why:** three things, in ascending order of weight. Latency is the weakest — 5 ms before a
2000 ms model call is noise. Render variables are caller data and potentially PII; on the
server-side path they cross the network on every request, which is an adoption blocker the
moment there is a hosted offering. And decisively: a server-render SDK makes PromptFS a hard
runtime dependency of the customer's production path. "Adding this can take down your app" is
the single largest objection a prompt manager faces.

Migrating later was rejected because the contract and the SDK surface would have to be
designed for the migration anyway, and the intermediate version teaches users a shape we
intend to withdraw.

**Would invalidate this:** a bundle that cannot be bounded in size for real repositories, or a
deployment model where consuming apps cannot hold state at all (edge functions with no warm
start). The universal path already covers the second case.

---

## D-004 · The SDK embeds the compiled Rust core

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 8, architecture.md §3.3

`promptfs-py` is `promptfs-core` compiled through pyo3 with `abi3`, distributed as ~5 wheels
per release. TypeScript follows in phase 4b via wasm.

**Rejected:** a pure-Python SDK using `jinja2`, with the template language restricted to a
subset both engines agree on and that restriction enforced by the core at parse time.

**Why:** minijinja and Jinja2 are not the same code and diverge at the edges — most sharply on
whitespace handling, which is semantic in a prompt. The pure-Python route is not free: it buys
a *weaker* guarantee and still costs recurring work, in the form of a byte-for-byte conformance
suite maintained forever. Embedding the core costs CI minutes instead, and makes divergence
structurally impossible rather than tested-for.

The payoff shows up somewhere non-obvious: the server can validate every prompt while building
a bundle and *know* the SDK will compile it identically. With two engines that validation
would be advisory.

**Would invalidate this:** a target platform where native wheels are not viable. That would
argue for a restricted-subset fallback for that platform specifically, not for abandoning the
embedded core everywhere.

---

## D-005 · Workspace split lands in phase 1

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 8

`promptfs-core` and `promptfs-server` are separate crates from the first commit of code.

**Rejected:** the earlier guidance — stay single-crate until something outside the server
binary needs to link the core, i.e. phase 4.

**Why:** that guidance named its own trigger, and D-004 fired it. Doing it now costs ~20 lines
of `Cargo.toml`. Doing it in phase 4 means prising `git2` and `tokio` back out of a core that
has had three phases to grow roots into them — and the dependency boundary is the entire point,
since the core is compiled into strangers' Python processes.

**Would invalidate this:** nothing. The cost is already paid.

---

## D-006 · Whole-environment bundles, not lazy per-prompt fetch

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** architecture.md §5.3

`GET /v1/bundle?env=<env>` returns every active target for that environment in one response.

**Rejected:** fetching each prompt lazily on first use, with push invalidation.

**Why:** a typical environment is under 200 KB. Whole-bundle makes cold start trivially correct,
makes the build-time snapshot a single file, and removes a network stall from the first call to
each prompt. An optional namespace filter covers large repos.

**Would invalidate this:** a real repository whose environment exceeds a few MB of active
targets, or measured memory pressure in a customer process.

---

## D-007 · A broken prompt ships inside the bundle, marked

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** architecture.md §5.3

The server compiles every target while building a bundle. What fails to compile travels as an
`error` object with file and line, and no `source`. The SDK raises only if that specific prompt
is used.

**Rejected:** refusing to serve a bundle that contains any broken prompt.

**Why:** refusing the bundle lets one malformed file in one namespace take down every consumer
of that environment — a blast radius wildly out of proportion to the fault. Marking it keeps
the failure scoped to the caller who actually depends on it, while Studio and the webhook
response surface it immediately to whoever pushed it.

**Would invalidate this:** a deployment model where partial correctness is worse than an
outage. Not this product.

---

## D-008 · The bundle format is additive, with version negotiation as the escape hatch

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** architecture.md §5.3

Within a major version the bundle only gains fields; nothing is removed or repurposed, and
SDKs ignore fields they do not recognise. A `format` integer on request and response exists for
a genuine break: the client asks for the highest it understands, the server serves the highest
both support.

**Rejected:** an unversioned format, on the grounds that we control both ends.

**Why:** we do not control both ends. A wheel published to PyPI cannot be made to upgrade, and
the first time a field changes meaning, every pinned client in the wild misreads it silently.
Additive-only removes ~95% of the problem for free; the integer covers the rest.

**Would invalidate this:** nothing before 1.0. After the first public wheel, this is frozen.

---

## D-009 · The published SLA covers only the hop we own

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 5

p99 < 2 s from **webhook receipt** to connected SDKs serving the new bundle. The
`git push → webhook delivery` leg is documented as out of scope.

**Rejected:** an end-to-end number measured from `git push`.

**Why:** webhook delivery latency belongs to GitHub, which makes no commitment about it. An
end-to-end SLA would be a promise about someone else's queue, breached by their incident and
not ours.

**Would invalidate this:** a Git provider offering a delivery guarantee we could inherit.

---

## D-010 · An unreachable PromptFS never fails a render

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** architecture.md §5.3

Three layers: a build-time snapshot vendored into the image, live sync over SSE, and a
last-known-good bundle that never expires. The SDK fails only when it has no bundle at all,
and that failure happens when `Client()` is constructed.

**Rejected:** fail-closed by default, and a TTL that expires the cached bundle.

**Why:** this is the mechanism that makes D-003's availability claim real rather than
aspirational. A TTL that expires to empty converts a PromptFS outage into a customer outage on
a delay — the worst possible timing, since it happens after the incident looks resolved.
Failing at construction moves the only real failure to process startup, where a deploy catches
it, instead of to the first render at 3 a.m.

An opt-in `max_staleness=` exists for teams that genuinely prefer fail-closed.

**Would invalidate this:** a regulated use case where serving a stale prompt is worse than
serving nothing. `max_staleness=` already covers it.

---

## D-011 · The routing key defaults to the OpenTelemetry trace id

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 4

Order: explicit argument → active OTel trace id → contextvar → random UUID with a warning
logged once.

**Rejected:** a fresh random key per call, and requiring the key explicitly on every call.

**Why:** invariant 4's real property is that a *retry* resolves to the same version. A fresh
random key per call breaks exactly that while looking correct — aggregate weights come out
right either way. Requiring it explicitly makes a mandatory parameter forgettable, and its
omission silent. A trace id survives a retry within the same trace by construction, and makes
the trace attributable to a prompt version for free.

**Would invalidate this:** nothing. Where no trace context exists the chain already falls
through, loudly.

---

## D-012 · PromptFS never calls an LLM in production

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 2

It resolves and renders. The application makes its own provider call. Studio's live-execution
panel sends the user's provider key in a request header, used for that one call and never
written, logged or cached.

**Rejected:** an optional LLM gateway mode, which would capture latency, cost and version
attribution automatically. Rejected separately: calling the provider directly from the browser
via `anthropic-dangerous-direct-browser-access`.

**Why:** a gateway means custodying provider keys in production, against invariant 2, and makes
PromptFS a mandatory hard dependency of every model call — undoing D-003 completely. It is also
a different product, competing with Portkey and Helicone rather than with prompt management.
Browser-direct was rejected because it exposes the key to every script on the page and drags in
per-provider CORS handling.

**Would invalidate this:** nothing short of a deliberate decision to become a gateway product,
which would be a new product and not an addition to this one.

---

## D-013 · No provider-shaped helpers in the SDKs

**Date:** 2026-08-25 · **Status:** accepted

A render result carries `body`, `model`, `temperature`, `resolved_ref`, `commit`,
`bundle_version` and `source`. No `to_anthropic()`, no `to_openai()`.

**Rejected:** thin convenience helpers that assemble a provider request.

**Why:** provider request shapes change on someone else's release schedule, and a helper that
lags is worse than no helper — it works until it silently drops a field. Wiring three values
into a provider call is one line the application already knows how to write.

**Would invalidate this:** a stable cross-provider request standard.

---

## D-014 · Python first, TypeScript via wasm afterwards

**Date:** 2026-08-25 · **Status:** accepted

`promptfs-py` in phase 4; the TypeScript SDK in 4b, with the core compiled to wasm.

**Rejected:** shipping both simultaneously, and shipping TypeScript as a thin HTTP client
alongside an in-process Python SDK.

**Why:** pyo3 and wasm are different release pipelines with different failure modes, and
sequencing them means the second is built with the first's contract already proven. Two SDKs
with *different* execution models was rejected outright — it would put D-003's guarantees in
one language and not the other, which is worse than not shipping the second yet.

**Would invalidate this:** TypeScript demand arriving materially ahead of Python demand.

---

## D-015 · `promptfs pull` is a subcommand, not a fourth crate

**Date:** 2026-08-25 · **Status:** accepted

The build-time snapshot tool lives on the existing `promptfs-server` binary, in HTTP client
mode.

**Rejected:** a separate `promptfs-cli` crate.

**Why:** `pull` only talks to a running PromptFS over HTTP. It needs no `git2` and no server
state, so a fourth crate would exist purely to hold one subcommand, and the binary is already
the distributed artifact.

**Would invalidate this:** the CLI growing commands that need neither the server's dependencies
nor its binary size.

---

## D-016 · gRPC is deferred

**Date:** 2026-08-20 · **Status:** accepted

The original spec said "REST/gRPC". Only REST is planned.

**Why:** the hot path is now in-process (D-003), which removes the per-call latency argument
that motivated gRPC. What remains is the universal path, whose whole point is being reachable
from `curl` and from languages with no SDK — precisely where REST wins.

**Would invalidate this:** a consumer that needs streaming renders, which REST handles badly.

---

## D-017 · `serde_yaml_ng`, not `serde_yaml`

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** the stack table in AGENTS.md

YAML parsing — prompt frontmatter and `.promptfs/deployments.yaml` — uses `serde_yaml_ng`.

**Rejected:** `serde_yaml` 0.9.34, which is what the stack table said until today. Also
rejected: `serde_yml`, a different fork with a history of obfuscated code in its releases.

**Why:** `serde_yaml` was archived by its author in March 2024. There will be no 0.9.35, for
a security report or anything else. This crate parses YAML out of a repository PromptFS does
not control, on a path reachable from an unauthenticated webhook, so "unmaintained" is a
supply-chain position rather than a style preference. `serde_yaml_ng` is API-compatible — the
migration is one line in one manifest — and is maintained. Its transitive
`unsafe-libyaml` is libyaml transpiled to Rust, so the wheel still needs no system library.

**Would invalidate this:** `serde_yaml_ng` going quiet in turn, or the ecosystem converging on
a different maintained fork. The API compatibility that made this switch cheap makes the next
one cheap too.

---

## D-018 · minijinja's `debug` feature is off, by feature and not at runtime

**Date:** 2026-08-25 · **Status:** accepted · **Touches:** invariant 2

`promptfs-core` depends on minijinja with `default-features = false` and the default feature
set minus `debug`.

**Rejected:** keeping the defaults and calling `Environment::set_debug(false)`. Also rejected:
hand-picking a narrower feature list.

**Why:** `debug` is on by default and appends the caller's render variables to the error's
`Debug` output — verified, not assumed:

```
Referenced variables: { secreto: "CARD-4111-1111-1111" }
```

Invariant 2 says render variables never reach an error message. `{:?}` on an error is what
`tracing`, `anyhow` and a panicking `unwrap()` all do, so this leaks through paths nobody
wrote deliberately. A runtime `set_debug(false)` is one deleted line away from being wrong
and nothing in CI would notice; removing the feature makes the leak unrepresentable.

Every other default feature is kept on purpose. Features are public template surface: adding
one later is additive and free, removing one breaks every prompt already written against it,
and a wheel on PyPI cannot be made to upgrade. So the rule is defaults minus what has a
written reason, never a list assembled from scratch.

**Would invalidate this:** minijinja separating error verbosity from variable capture, which
would let us keep the better diagnostics without the leak.

---

## D-019 · No `[workspace.dependencies]` table

**Date:** 2026-08-26 · **Status:** accepted · **Touches:** invariant 8

Each crate declares its own dependencies with its own versions. The root manifest carries
`[workspace.package]` for metadata inheritance and nothing else.

**Rejected:** the idiomatic central `[workspace.dependencies]` table with members writing
`dep = { workspace = true }`. It is what most Rust workspaces do and someone will propose it.

**Why:** it optimises for the wrong thing here. The benefit is keeping one version of a crate
across members — which the shared `Cargo.lock` already does, since the resolver unifies
compatible requirements whether or not the table exists. What the table changes is the *cost
of adding a dependency to `promptfs-core`*: with it, a crate the server pulled in is already
listed at the root and reaching for it in the core is one word, `workspace = true`. Invariant
8 exists because that list ships inside a customer's Python interpreter, and
`check-invariants.sh` only catches the seven names it knows. Making the core spell out every
version keeps each addition a deliberate act with a diff worth reading.

**Would invalidate this:** the two crates converging on a large shared dependency set, where
the drift risk from duplicated version requirements outweighs the friction. A third crate
(`promptfs-py`) that legitimately shares the core's exact list would be the trigger to
re-examine.
