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

---

## D-020 · The frontmatter split is positional, and the first close wins

**Date:** 2026-08-27 · **Status:** accepted · **Touches:** invariant 6, managed-repo contract

`split_frontmatter` recognises delimiters by position, never by content. The block exists only
if line 1 is `---` followed by nothing but spaces or tabs; it ends at the *first* subsequent line
of that same shape. Leading whitespace never opens or closes: `  ---` is content.
After that line the function stops looking, so `---` inside the body is ordinary text. `\r\n`
is a delimiter line ending too, a delimiter at EOF with no trailing newline closes the block,
and the resulting empty body is not a format error.

**Rejected:** taking the *last* delimiter (`rfind`), which protects a `---` written inside the
frontmatter's own YAML. Also rejected: feeding the bytes to an incremental YAML parser and
splitting where it reports the document ended, which is the only way to be actually correct.

**Why:** the two halves are not symmetric. The body is caller text bound for a model — prose,
Markdown, anything, `---` horizontal rules included — while the frontmatter is a small YAML
whose schema we own and whose authors we can tell to avoid one construct. Last-wins trades a
common case for a rare one. The cost of first-wins is smaller than it looks: a delimiter only
counts at column 0, and a YAML block scalar's content is indented, so the obvious hole — a
`---` line inside a `description: |` — cannot occur, and a test pins that. What remains is a
column-0 `---` in the frontmatter, which is a YAML *document separator*: the file was not
single-document frontmatter to begin with. Jekyll, Hugo and Astro share the same rule, and it
fails loudly, because `serde_yaml_ng` then receives truncated prose and rejects it. The
incremental parser is correct and unaffordable: it would parse YAML before the caller has
decided to, and hand back owned values instead of the two borrowed slices invariant 6 wants.

**Also rejected:** requiring the line to be exactly `---`, with no trailing blanks. Six of the
seven implementations surveyed — Zola, Cobalt, pulldown-cmark, gray_matter, Jekyll, Astro —
accept them, and a trailing space is invisible in an editor: the author sees a correct file and
an error naming line 1. What is *not* copied is gray-matter's and Astro's tolerance of arbitrary
trailing text, or a `trim()` on the whole line, which would let an indented `---` close the block
from inside a YAML block scalar.

**Would invalidate this:** a contract change that gives a column-0 `---` meaning inside the
frontmatter — multi-document frontmatter, or a dialect where the separator is content. The fix
then is an escape the contract defines, not last-wins, which would only move the failure into
the body.

---

## D-021 · Prompt frontmatter tolerates unknown fields

**Date:** 2026-08-27 · **Status:** accepted · **Touches:** consumer contract

`PromptMeta` accepts fields it does not know instead of rejecting them.

**Rejected:** `#[serde(deny_unknown_fields)]`, which would catch a typo — `temprature: 0.1` —
at parse time instead of letting it be silently ignored.

**Why:** the reader is whichever wheel the customer happens to have installed, and a wheel
already on PyPI cannot be made to upgrade. Any field added in a later version is "unknown" to
every older reader, so rejecting unknown fields turns a forward-compatible file into a hard
failure at render time, and makes the bundle format's additive rule unenforceable in practice.
Catching typos belongs to `contract-check`, which can warn without failing a production render.

**Would invalidate this:** a field whose *absence* changes behaviour silently — a `disabled:
true` that an old reader ignores while the author believes the prompt is off. That calls for a
version marker in the file, not for rejecting unknown fields.

---

## D-022 · `deployments.yaml` is read at one control ref, and environments come from it

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** managed-repo contract, architecture.md §4, §5.3

The server reads `.promptfs/deployments.yaml` at a single configured ref, the *control ref*:
`heads/main` unless set otherwise. Each target's prompt source is read at the target's own ref.
The environments that exist are the ones the file names, and a request for any other is a 404.
The bundle's `from_commit` is the control ref's commit. What describes the server's own
deployment — repository URL, credentials, listen address — comes from environment variables,
never from the repository.

**Rejected:** a control ref per environment (`heads/production` governing production),
environments declared in the server's configuration, and following the remote's default branch
automatically.

**Why:** one ref gives one answer to "what does production serve", and turns every routing
change — a canary weight, a promotion, a rollback — into an ordinary reviewed commit. A list of
environments in the server's configuration would be a second list to drift from the file. A 404
for an unknown environment makes `env="prodution"` fail when the SDK client is constructed, at
deploy time (D-010), instead of producing a client that serves nothing. Following the remote's
default branch was rejected because a rename would silently move what production obeys, where an
explicit ref that does not exist fails at startup.

**Would invalidate this:** teams that need a different review or protection policy for production
routing than for staging. Per-environment control refs would then earn their complexity.

---

## D-023 · A prompt with no entry for an environment is not served there

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** managed-repo contract, architecture.md §4

If `deployments.yaml` has no entry for a prompt in an environment, the prompt does not exist in
that environment. Resolving it is an error naming the prompt and the environment — a 404 on the
server path — never a fallback.

**Rejected:** falling back to the control ref's head. Also rejected, for now: an
environment-level default ref (`staging: default: heads/main`).

**Why:** deploying has to be an explicit act. With a fallback, a prompt merged to `main` and never
reviewed for production reaches production because nobody wrote a line saying it should not. An
environment-level default is a fair convenience, but it widens the public contract; it is
additive, so it can arrive later without breaking a single repository, while a fallback cannot be
withdrawn once people rely on it.

**Would invalidate this:** one line per prompt per environment proving to be real friction. The
answer then is the environment-level default, added additively — not an implicit fallback.

---

## D-024 · One instance serves one repository

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** architecture.md §2.1

A PromptFS server instance serves exactly one repository. §2.1's "multi-repository support" —
separating projects or business domains — means one instance per repository.

**Rejected:** one instance serving a configured list of repositories.

**Why:** prompts are addressed as `<namespace>/<name>`, and two repositories can both define
`support/classifier`. Serving both from one instance forces the repository into the prompt's
name, which changes the public contract for every user, single-repository users included. One
instance per repository keeps configuration, credentials, webhooks and failures apart: a revoked
token or a broken `deployments.yaml` in one domain cannot touch another. An instance idles under
50 MB, so the separation is cheap.

**Would invalidate this:** prompts that need to include templates from another repository, or
enough repositories that one process each becomes the operational burden.

---

## D-025 · A webhook is a hint: verified when a secret is set, absent when not

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** invariant 5, architecture.md §5.3

The server takes a webhook only as a signal that the repository changed. It does not read the
payload to learn what changed: it fetches, then compares its refs before and after. Concurrent
deliveries coalesce into a single fetch. The webhook route exists only when a secret is
configured, and every delivery is verified against it — GitHub's `X-Hub-Signature-256`
(HMAC-SHA256 with a `sha256=` prefix, compared in constant time); GitLab's `X-Gitlab-Token`, or
`webhook-signature` in its newer signing mode. Without a secret the server runs on polling alone.

**Rejected:** trusting the payload's account of what changed, and accepting unsigned deliveries.

**Why:** invariant 1 already makes a webhook harmless as a source of content — the server always
reads the truth from Git — so a forged delivery costs at most one fetch, and coalescing caps even
that. Trusting the payload would reintroduce a second, forgeable account of what changed. Tying
the route to the secret makes a misconfiguration degrade freshness instead of opening an
endpoint: forgetting the secret is slow, not unsafe.

**Would invalidate this:** a Git provider that cannot authenticate its deliveries at all. It gets
polling, which is already the fallback.

---

## D-026 · Machine clients use environment-scoped tokens; no authentication means loopback only

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** invariant 2, consumer contract, architecture.md §5.2, §5.3

`GET /v1/bundle` and `POST /v1/prompts/render` require a bearer token. Tokens come from
environment variables, are read-only, and each is scoped to the environments it may read. A
server with no authentication configured — no tokens and no Studio sign-in — listens on the
loopback interface only.

**Rejected:** tokens as an opt-in, with an open server by default. Also rejected: delegating
authentication entirely to the network — a private subnet, a service mesh — with no check in
PromptFS.

**Why:** a bundle is the source of every prompt deployed in an environment, and prompts are often
the customer's intellectual property; an open default leaks all of it on the first misconfigured
deploy. Tokens are configuration, not data, so they need no store (D-001), and they give least
privilege: an application holds a token that reads its own environment and nothing else — never
a Git credential. Binding to loopback when nothing is configured makes "listening publicly without
authentication" unrepresentable rather than discouraged, and lets phase 1 ship its render
endpoint before tokens exist.

**Would invalidate this:** deployments where handing a secret to every application is harder than
securing the network. A network-delegation mode would then be added — explicit, and logged loudly
at startup.

---

## D-027 · Studio identity belongs to the Git provider; saving is a pull request

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** D-001, architecture.md §2.1, §5.4

People sign in to Studio with their Git provider's OAuth, and may see or change what the provider
lets them see or change in the repository. Saving is proposing: Studio creates a branch, a commit
and a pull request through the provider's API, with the signed-in user's own token. The session
travels in a signed, encrypted cookie whose key comes from an environment variable; the server
stores nothing. One adapter per provider, GitHub first.

**Rejected:** a PromptFS user and role store. Also rejected: an SSO proxy in front of Studio
passing identity in a header, with commits pushed by `git2` under a server credential; and a
read-only Studio in v1.

**Why:** a user and role store is a database, which D-001 rules out — and the provider already
has users, teams and per-repository permissions. Delegating means the provider enforces who may
write, branch protection applies unchanged, and the Git history names the person behind each
change. A server credential would make every change the same bot's, and lose that audit trail.
The provider's API is needed regardless: a pull request is a provider concept, not a Git one. A
read-only Studio was rejected because editing and proposing is most of the reason Studio exists.

**Would invalidate this:** a provider with no API, such as a bare Git server. It would fall back
to a `git2` push to a branch, with the pull request opened by hand.

---

## D-028 · One active server instance per repository

**Date:** 2026-09-26 · **Status:** accepted · **Touches:** invariants 4 and 5, D-009, architecture.md §2.2

v1 runs a single active instance per repository, restarted by its orchestrator when it fails.
Replicas behind a load balancer are not supported. This narrows D-009: its SLA is exact because
the only instance is the one that receives the webhook.

**Rejected:** several replicas with eventual consistency.

**Why:** without a shared database each replica keeps its own clone, and the replicas that did
not receive the webhook lag until their next poll. In that window a universal-path retry can land
on a lagging replica and resolve a different version — the exact failure invariant 4 exists to
prevent — and an SDK that reconnects to one swaps back to an older bundle. The single instance
costs little: D-010 makes an outage invisible to every application with an SDK, and the load
that grows with usage — rendering — runs inside those applications, so the server's load grows
with the number of application instances rather than with their traffic. For comparison
(checked 2026-09-26), Langfuse replicates freely because every replica reads the same Postgres;
it pays with a database to operate, a 60-second SDK cache by default, and A/B splits drawn at
random on every call.

**Would invalidate this:** universal-path traffic in production that needs high availability
before the TypeScript SDK exists, a fleet large enough to exceed one instance's SSE capacity, or a
hard availability requirement on Studio. Replicas then need routing-key affinity at the load
balancer and an SDK that refuses to swap to an older bundle — and since a published wheel cannot
be upgraded (D-008), that refusal has to ship in the first wheel if replicas are ever to be an
option.

---

## D-029 · A missing input always fails the render, and names the input

**Date:** 2026-10-03 · **Status:** accepted · **Touches:** invariants 2 and 6, consumer contract

Two layers, both in `CompiledPrompt::render`. First, every name declared in `inputs:` must be
present in the caller's variables; the first one absent, in declaration order, fails the render
with `InputNotFound { prompt_path, input_name }` before the engine runs. Second, the engine runs
with `UndefinedBehavior::Strict`, so a variable the template uses but the author never declared
— the typo case — fails with `RenderFailed`. A supplied null is a value, not an absence: it
passes the check and renders as `None`. Authors write an optional variable as
`{% if x is defined %}` or `{{ x | default("…") }}` — both pass under strict mode.

**Rejected:** `Lenient`, minijinja's default — a typo'd or forgotten variable renders as empty,
the request succeeds, and a mutilated prompt reaches the model with nobody told. Rejected too:
`Strict` alone — its error says `undefined value` and names nothing, so the caller cannot tell
which input to supply. And reporting every missing input at once — it makes the variant a
`Vec`, a heavier public contract for an error the caller fixes one deploy at a time.

**Why:** an error that names the missing input and the file turns a silent quality regression
into a failure the caller can act on, and invariant 2 holds by construction: the variant has a
name field and no value field. The check costs no extra conversion on the hot path — the
variables become a `minijinja::Value` once and the engine renders that same value — and it
allocates only on failure.

**Would invalidate this:** prompts in the wild that rely on silently empty optional variables
at a scale where `is defined` is an unreasonable migration, or a need for optional declared
inputs — which would become a frontmatter field (`optional: true`), additive under D-021, not a
return to `Lenient`.

---

## D-030 · Routing hashes the prompt address and the key with FNV-1a, 64-bit

**Date:** 2026-10-03 · **Status:** accepted · **Touches:** invariant 4, consumer contract

A deployment picks its target from `fnv1a_64(address ‖ 0x00 ‖ routing_key) % total_weight`,
where `address` is `<namespace>/<name>`. Targets own consecutive slices of `0..total_weight` in
declaration order, each as wide as its weight; a weight of zero owns no slice. The hash, its
constants and that byte layout are pinned by tests against values computed outside Rust.

**Rejected:** `std`'s `DefaultHasher` — its documentation says the algorithm is unspecified and
must not be relied on across releases, so a toolchain upgrade could re-route every key while
the server and a wheel built months apart disagree. xxHash — a better distribution, at the cost
of a new dependency in the core, which ships inside customers' processes. SHA-256 — a
cryptographic hash defends against an adversary choosing keys, which no one here is. Hashing
the key alone — every prompt then maps a trace to the same point, so the traces in one 10%
canary are exactly the traces in every other 10% canary, and when quality drops nobody can
tell which canary did it.

**Why:** the promise is "the same key resolves to the same ref" across processes, languages
and years, so the algorithm must be published and frozen. FNV-1a is a few lines, has no
dependency, and its distribution measured 9.98% on a 90/10 split over 10,000 sequential keys.
Hashing the address in makes two prompts' canaries share ~1% of traces instead of all of them,
and costs nothing: the bytes are streamed into the hash, never concatenated.

Consequence worth knowing: reordering a deployment's targets moves the slices, and so re-routes
keys, even when the weights are unchanged.

**Would invalidate this:** a measured distribution problem on real routing keys — which would
mean a new hash behind a new bundle format version, never a silent swap.
