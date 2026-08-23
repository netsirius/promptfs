---
name: fixture-repo
description: Scaffold a git2-built fixture prompt repo for a Rust test — namespaced prompts, .promptfs/deployments.yaml, and multiple ref shapes (tags/heads/SHA) so canary routing and ref resolution are actually exercised.
disable-model-invocation: true
---

# fixture-repo

Builds a temporary bare Git repo in a test, populated to the managed-repo contract, using
`git2` — never a mock, never the `git` binary.

AGENTS.md prescribes this pattern: *"prefer fixture repos under `fixtures/` created via `git2`
in-test over mocking `git2` itself. The Git interaction is the part most likely to be wrong."*
Mocking `git2` deletes the code most likely to contain the bug.

## Usage

`/fixture-repo <what the test needs>`

Examples:

- `/fixture-repo a canary deployment with 85/15 across a tag and a branch`
- `/fixture-repo a prompt with malformed frontmatter, for the 4xx path`
- `/fixture-repo two namespaces so tree traversal is exercised`

## What to produce

Copy [`template.rs`](template.rs) into the test module and adapt it. Keep the builder shape —
it exists so every fixture in the suite creates the same ref shapes and the coverage gap
`git2-fixture-reviewer` looks for cannot open silently.

## Rules the generated fixture must follow

1. **Bare repo.** Production reads bare clones. A fixture with a working tree can pass on
   path-based reads that break against the real thing. Use `Repository::init_bare`.
2. **All three ref shapes, unless the test is explicitly about one.** A tag (`tags/v1.0.0`),
   a branch (`heads/main`), and the raw commit SHA. Fixtures that only ever create branches
   are how the tag-resolution path ships untested.
3. **Fully-qualified refs everywhere** — `heads/main`, never `main`; `tags/v1.0.0`, never
   `v1.0.0`. In the fixture *and* in the assertions. A test that accepts a bare name teaches
   the parser to accept one.
4. **Canary fixtures need ≥ 2 targets with different, non-equal weights.** A single target
   does not exercise the router, and 50/50 hides off-by-one bucketing errors.
5. **Assert same-key-same-ref for canary tests.** Invariant 4's real property is that a
   retried request resolves to the same version. Aggregate weight distribution comes out
   correct even with unseeded `rand()` — only the stability assertion catches it.
6. **`TempDir` for cleanup**, so a failing test does not leave repos behind. If `tempfile` is
   not yet a dev-dependency, add it under `[dev-dependencies]` and say so — it is a test-only
   dependency, which AGENTS.md's "a new crate needs a reason" rule is satisfied by.
7. **No `Command::new("git")`.** Same rule as `crates/*/src`; the hook does not scan tests,
   so this one is on you.

## Managed-repo contract the fixture must match

```
.promptfs/deployments.yaml
prompts/<namespace>/<name>.prompt.md
prompts/<namespace>/<name>.eval.yaml    # phase 4, optional in fixtures
config.yaml
```

Prompt file: YAML frontmatter (`name`, `description`, `model`, `temperature`, `inputs`)
delimited by `---`, then a Jinja2 body. Prompts are addressed `<namespace>/<name>`.

Run `/contract-check` against a generated fixture directory to confirm it matches.
