//! Template for a git2-built fixture prompt repo. Copy into a test module and adapt.
//!
//! Builds a BARE repo (production reads bare clones) populated to the managed-repo contract,
//! with all three ref shapes the contract promises: a tag, a branch, and the raw commit SHA.
//!
//! Requires `tempfile` under [dev-dependencies].

use std::path::Path;

use git2::{Repository, Signature};
use tempfile::TempDir;

/// A bare fixture repo plus the refs a test needs to address it.
pub struct FixtureRepo {
    /// Kept alive so the repo outlives the test; dropping it removes the directory.
    _dir: TempDir,
    pub repo: Repository,
    /// Fully-qualified, exactly as the contract requires callers to pass them.
    pub tag_ref: String,
    pub branch_ref: String,
    pub commit_sha: String,
}

impl FixtureRepo {
    /// One namespace, one prompt, one deployment. The minimum that exercises the pipeline.
    pub fn simple() -> Self {
        Self::build(&[(
            "support/classifier",
            PROMPT_CLASSIFIER,
        )], DEPLOYMENTS_SIMPLE)
    }

    /// Two weighted targets across a tag and a branch — the canary routing case.
    /// Weights are deliberately not 50/50 so an off-by-one in bucketing is visible.
    pub fn canary() -> Self {
        Self::build(&[(
            "support/classifier",
            PROMPT_CLASSIFIER,
        )], DEPLOYMENTS_CANARY)
    }

    /// `prompts` is `(<namespace>/<name>, file body)`. Writes each to
    /// `prompts/<namespace>/<name>.prompt.md`, commits, then creates both a branch and a tag
    /// pointing at that commit.
    pub fn build(prompts: &[(&str, &str)], deployments: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let repo = Repository::init_bare(dir.path()).expect("init bare");

        let mut builder = TreeBuilderCtx::new(&repo);
        builder.insert(".promptfs/deployments.yaml", deployments);
        builder.insert("config.yaml", CONFIG_YAML);
        for (addr, body) in prompts {
            builder.insert(&format!("prompts/{addr}.prompt.md"), body);
        }
        let tree_oid = builder.finish();

        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig = Signature::now("PromptFS Fixture", "fixture@promptfs.test").expect("signature");
        let commit_oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "fixture", &tree, &[])
            .expect("commit");

        // Ref shape 1: a branch. refs/heads/main already exists from the commit above.
        // Ref shape 2: a tag. Lightweight — swap to `repo.tag(..)` for an annotated tag if the
        // code under test peels tag objects; the two behave differently under revparse_single.
        let commit = repo.find_commit(commit_oid).expect("find commit");
        repo.tag_lightweight("v1.0.0", commit.as_object(), false)
            .expect("tag");

        Self {
            _dir: dir,
            repo,
            // Fully qualified, always. Never bare "main" / "v1.0.0".
            tag_ref: "tags/v1.0.0".to_string(),
            branch_ref: "heads/main".to_string(),
            commit_sha: commit_oid.to_string(),
            }
    }

    pub fn path(&self) -> &Path {
        self.repo.path()
    }
}

/// Writes nested paths into a git tree. libgit2's TreeBuilder is single-level, so nested
/// directories have to be built bottom-up; this collapses that into `insert("a/b/c.md", ..)`.
struct TreeBuilderCtx<'r> {
    repo: &'r Repository,
    entries: Vec<(String, git2::Oid)>,
}

impl<'r> TreeBuilderCtx<'r> {
    fn new(repo: &'r Repository) -> Self {
        Self { repo, entries: Vec::new() }
    }

    fn insert(&mut self, path: &str, content: &str) {
        let oid = self.repo.blob(content.as_bytes()).expect("blob");
        self.entries.push((path.to_string(), oid));
    }

    fn finish(self) -> git2::Oid {
        // Build directories depth-first: group by parent, write subtrees, then the root.
        let mut root = self.repo.treebuilder(None).expect("treebuilder");
        let mut dirs: std::collections::BTreeMap<String, Vec<(String, git2::Oid)>> =
            Default::default();

        for (path, oid) in self.entries {
            match path.rsplit_once('/') {
                None => root.insert(&path, oid, 0o100644).map(|_| ()).expect("insert"),
                Some((dir, name)) => dirs
                    .entry(dir.to_string())
                    .or_default()
                    .push((name.to_string(), oid)),
            }
        }

        // Deepest paths first, so a subtree's children exist before the subtree is written.
        let mut nested: Vec<_> = dirs.into_iter().collect();
        nested.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.matches('/').count()));

        let mut written: std::collections::BTreeMap<String, git2::Oid> = Default::default();
        for (dir, files) in nested {
            let mut tb = self.repo.treebuilder(None).expect("treebuilder");
            for (name, oid) in files {
                tb.insert(&name, oid, 0o100644).expect("insert blob");
            }
            // Attach any already-written subtrees that live directly under this dir.
            let child_prefix = format!("{dir}/");
            let children: Vec<_> = written
                .keys()
                .filter(|k| k.starts_with(&child_prefix) && !k[child_prefix.len()..].contains('/'))
                .cloned()
                .collect();
            for child in children {
                let oid = written.remove(&child).unwrap();
                tb.insert(&child[child_prefix.len()..], oid, 0o040000).expect("insert tree");
            }
            written.insert(dir, tb.write().expect("write tree"));
        }

        for (dir, oid) in written {
            if !dir.contains('/') {
                root.insert(&dir, oid, 0o040000).expect("insert root tree");
            }
        }

        root.write().expect("write root")
    }
}

const CONFIG_YAML: &str = "providers:\n  openai:\n    base_url: https://api.openai.com/v1\n";

const PROMPT_CLASSIFIER: &str = r#"---
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
"#;

const DEPLOYMENTS_SIMPLE: &str = r#"deployments:
  support/classifier:
    environments:
      production:
        targets:
          - ref: "heads/main"
            weight: 100
"#;

// Two targets, non-equal weights, across both ref shapes.
const DEPLOYMENTS_CANARY: &str = r#"deployments:
  support/classifier:
    environments:
      production:
        strategy: canary
        targets:
          - ref: "tags/v1.0.0"
            weight: 85
          - ref: "heads/main"
            weight: 15
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_exposes_all_three_ref_shapes() {
        let fx = FixtureRepo::simple();

        // Every shape the managed-repo contract promises must resolve.
        for spec in [&fx.tag_ref, &fx.branch_ref, &fx.commit_sha] {
            fx.repo
                .revparse_single(spec)
                .unwrap_or_else(|e| panic!("ref {spec} did not resolve: {e}"));
        }
    }

    #[test]
    fn prompt_is_readable_at_the_contract_path() {
        let fx = FixtureRepo::simple();
        let tree = fx.repo.revparse_single("heads/main").unwrap()
            .peel_to_commit().unwrap()
            .tree().unwrap();

        let entry = tree
            .get_path(Path::new("prompts/support/classifier.prompt.md"))
            .expect("prompt at contract path");
        let blob = fx.repo.find_blob(entry.id()).unwrap();
        assert!(std::str::from_utf8(blob.content()).unwrap().starts_with("---"));
    }

    // The property invariant 4 actually protects: a retried request resolves to the same
    // version. Aggregate weight distribution comes out correct even with unseeded rand(),
    // so this assertion — not a distribution test — is what catches the violation.
    //
    // Enable once the router exists (phase 2).
    #[test]
    #[ignore = "phase 2: router not implemented yet"]
    fn same_seed_key_resolves_to_the_same_ref() {
        let _fx = FixtureRepo::canary();
        // let first = router.resolve("support/classifier", "production", "req-abc-123");
        // for _ in 0..100 {
        //     assert_eq!(first, router.resolve("support/classifier", "production", "req-abc-123"));
        // }
    }
}
