---
name: contract-check
description: Validate a directory against the PromptFS managed-repo contract — layout, prompt frontmatter, namespaced addressing, fully-qualified refs and deployment weights. Use on fixtures under fixtures/, on a user's repo, or after touching any parsing code.
---

# contract-check

The managed-repo layout is a **public contract**: changing it breaks every existing user.
It is specified in prose in AGENTS.md and `docs/architecture.md` §4. This skill is the
executable copy — one place where the contract is checkable rather than only readable.

## Usage

```bash
.claude/skills/contract-check/validate.sh [path]        # default: cwd
.claude/skills/contract-check/validate.sh --self-test   # built-in checks
```

Or `/contract-check <path>`.

## What it checks

**Layout**

```
.promptfs/deployments.yaml    required
prompts/<namespace>/<name>.prompt.md
config.yaml                   optional (provider options)
```

**Prompt files**
- Frontmatter opens with `---` on line 1 and is terminated.
- Required keys present: `name`, `description`, `model`.
- `name:` matches the filename stem — otherwise `<namespace>/<name>` lookups miss.
- The file sits under a namespace directory; prompts are addressed `<namespace>/<name>`.

**deployments.yaml**
- Every `ref:` is **fully qualified** — `tags/…`, `heads/…`, or a raw SHA. Never a bare name.
- Weights per `<prompt, environment>` are handed to `validate_weights()` in the script.

## When to reach for it

- After generating a fixture with `/fixture-repo`, to confirm it matches the contract.
- Before changing the frontmatter parser, the deployments deserializer, or ref resolution —
  run it first to see what the contract currently guarantees.
- Against a real user repo when diagnosing a 4xx.

## Known ceilings

The script is line-oriented `grep`/`awk`, not a YAML parser — `pyyaml` is not in the stdlib
and a skill helper should not require a pip install. It assumes the canonical 2-space
indentation from `docs/architecture.md` §4, so unusual-but-valid YAML can slip past.

Once the Rust crate exists, `serde_yaml` should do this properly in a `#[test]` against
`fixtures/`, and this script stays as the pre-commit convenience copy. Both are marked with
`ponytail:` in the source.

## Related

- `/fixture-repo` — generates repos this validates.
- `git2-fixture-reviewer` — reviews whether fixtures exercise what they claim.
