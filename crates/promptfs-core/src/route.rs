//! Picks one target of a deployment for a routing key.
//!
//! The server and every SDK link this same code, so a retry resolves to the same ref wherever it
//! lands (invariant 4). That holds only while the hash below never changes: it is part of the
//! consumer contract, not an implementation detail. See D-030.

use thiserror::Error;

/// A deployment that cannot route: caught when it is built, so picking never fails.
#[derive(Debug, Error, PartialEq)]
pub enum RouteError {
    #[error("ref {git_ref} is not fully qualified: expected `tags/…` or `heads/…`")]
    BareRef { git_ref: String },
    #[error("deployment has no target with a weight above zero")]
    NoWeight,
}

/// A fully-qualified Git reference — `tags/v1.2.0`, `heads/main`. A bare `v1.2.0` is
/// ambiguous between a tag and a branch, so it does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRef(String);

impl GitRef {
    pub fn parse(git_ref: &str) -> Result<Self, RouteError> {
        let short_name = git_ref
            .strip_prefix("tags/")
            .or_else(|| git_ref.strip_prefix("heads/"));
        match short_name {
            Some(name) if !name.is_empty() => Ok(Self(git_ref.to_string())),
            _ => Err(RouteError::BareRef {
                git_ref: git_ref.to_string(),
            }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The string that seeds the weighting. Borrowed: it arrives with every render, and the call
/// path allocates nothing beyond the output (invariant 6).
#[derive(Debug, Clone, Copy)]
pub struct RoutingKey<'a>(&'a str);

impl<'a> RoutingKey<'a> {
    pub fn new(key: &'a str) -> Self {
        Self(key)
    }
}

/// One weighted entry in a deployment. A weight of zero keeps the target declared and never
/// selects it.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub git_ref: GitRef,
    pub weight: u32,
}

/// The targets of one prompt in one environment. Weights need not sum to 100.
#[derive(Debug)]
pub struct Deployment {
    /// The `<namespace>/<name>` address. Hashed with the key so that two prompts' canaries
    /// select independent sets of traces. See D-030.
    prompt: String,
    targets: Vec<Target>,
    /// `u64` so that no list of `u32` weights can overflow it.
    total_weight: u64,
}

impl Deployment {
    pub fn new(prompt: &str, targets: Vec<Target>) -> Result<Self, RouteError> {
        let total_weight = targets.iter().map(|t| u64::from(t.weight)).sum();
        if total_weight == 0 {
            return Err(RouteError::NoWeight);
        }
        Ok(Self {
            prompt: prompt.to_string(),
            targets,
            total_weight,
        })
    }

    /// The same key always picks the same target; across many keys, picks land on the weights.
    pub fn pick(&self, key: RoutingKey<'_>) -> &Target {
        self.target_at(self.point(key))
    }

    /// Where `key` lands in `0..total_weight`. The hashed bytes are the prompt address, a NUL,
    /// then the key — streamed, never concatenated. That byte layout is contract, like the hash.
    fn point(&self, key: RoutingKey<'_>) -> u64 {
        // NUL cannot occur in a path, so no prompt/key split can collide with another.
        let bytes = self.prompt.bytes().chain([0]).chain(key.0.bytes());
        fnv1a_64(bytes) % self.total_weight
    }

    /// The target whose slice of `0..total_weight` contains `point`. Targets own consecutive
    /// slices in declaration order, each as wide as its weight: weights `[1, 3]` give the first
    /// target `0` and the second `1..=3`.
    fn target_at(&self, point: u64) -> &Target {
        let mut slice_end = 0;
        for target in &self.targets {
            slice_end += u64::from(target.weight);
            // `<`, not `<=`: `slice_end` is where the next target's slice starts.
            if point < slice_end {
                return target;
            }
        }
        unreachable!("pick reduces point modulo total_weight, the sum of every slice")
    }
}

/// FNV-1a, 64-bit, as published. Its constants are the contract: changing either re-routes
/// every key in production. Pinned by `fnv1a_matches_the_published_vectors`.
fn fnv1a_64(bytes: impl IntoIterator<Item = u8>) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.into_iter().fold(OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIFIER: &str = "support/classifier";
    const RESPONDER: &str = "support/responder";
    const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    const STABLE: &str = "tags/v1.2.0";
    const CANARY: &str = "heads/canary-gpt4o";

    fn target(git_ref: &str, weight: u32) -> Target {
        Target {
            git_ref: GitRef::parse(git_ref).expect("qualified ref"),
            weight,
        }
    }

    fn deployment_of(prompt: &str, targets: &[(&str, u32)]) -> Deployment {
        Deployment::new(prompt, targets.iter().map(|&(r, w)| target(r, w)).collect())
            .expect("routable deployment")
    }

    fn deployment(targets: &[(&str, u32)]) -> Deployment {
        deployment_of(CLASSIFIER, targets)
    }

    /// From the FNV specification. If this fails, every key in production has moved.
    #[test]
    fn fnv1a_matches_the_published_vectors() {
        assert_eq!(fnv1a_64(*b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(*b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(*b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_bare_ref_does_not_parse() {
        for bare in ["v1.2.0", "main", "tags/", "refs/tags/v1.2.0"] {
            assert!(
                matches!(GitRef::parse(bare), Err(RouteError::BareRef { .. })),
                "{bare}"
            );
        }
    }

    #[test]
    fn a_deployment_with_no_weight_cannot_be_built() {
        let err =
            Deployment::new(CLASSIFIER, vec![target(STABLE, 0)]).expect_err("nothing to pick");
        assert_eq!(err, RouteError::NoWeight);
    }

    /// The assertion this task exists for: an aggregate split also passes with `rand()`.
    #[test]
    fn the_same_key_always_picks_the_same_target() {
        let deployment = deployment(&[(STABLE, 90), (CANARY, 10)]);
        let first = deployment.pick(RoutingKey::new("trace-4bf92f35")).clone();
        for _ in 0..1_000 {
            assert_eq!(deployment.pick(RoutingKey::new("trace-4bf92f35")), &first);
        }
    }

    #[test]
    fn a_single_target_always_resolves_to_it_whatever_its_weight() {
        for weight in [1, 7, 100, u32::MAX] {
            let deployment = deployment(&[(STABLE, weight)]);
            for i in 0..100 {
                let key = format!("trace-{i}");
                assert_eq!(
                    deployment.pick(RoutingKey::new(&key)).git_ref.as_str(),
                    STABLE
                );
            }
        }
    }

    /// Deliberately not 50/50, so an off-by-one at a slice boundary picks the wrong target.
    #[test]
    fn slice_boundaries_follow_the_weights() {
        let deployment = deployment(&[(STABLE, 1), (CANARY, 3)]);
        assert_eq!(deployment.target_at(0).git_ref.as_str(), STABLE);
        assert_eq!(deployment.target_at(1).git_ref.as_str(), CANARY);
        assert_eq!(deployment.target_at(3).git_ref.as_str(), CANARY);
    }

    #[test]
    fn a_zero_weight_target_is_never_picked() {
        let deployment = deployment(&[(STABLE, 2), (CANARY, 0), ("heads/main", 2)]);
        let picked: Vec<&str> = (0..4)
            .map(|point| deployment.target_at(point).git_ref.as_str())
            .collect();
        assert_eq!(picked, [STABLE, STABLE, "heads/main", "heads/main"]);
    }

    /// Pins the hashed byte layout: these points were computed independently, outside Rust.
    /// If one moves, every trace in production is re-routed.
    #[test]
    fn the_point_depends_on_the_prompt_and_the_key() {
        let key = RoutingKey::new(TRACE_ID);
        assert_eq!(
            deployment_of(CLASSIFIER, &[(STABLE, 90), (CANARY, 10)]).point(key),
            47
        );
        assert_eq!(
            deployment_of(RESPONDER, &[(STABLE, 90), (CANARY, 10)]).point(key),
            72
        );
    }

    /// Two 10% canaries share ~1% of traces when independent, all 10% when the hash ignores
    /// the prompt. Measured: 94 of 10k.
    #[test]
    fn canaries_of_different_prompts_select_independent_traces() {
        let classifier = deployment_of(CLASSIFIER, &[(STABLE, 90), (CANARY, 10)]);
        let responder = deployment_of(RESPONDER, &[(STABLE, 90), (CANARY, 10)]);
        let in_both = (0..10_000)
            .filter(|i| {
                let key = format!("trace-{i}");
                let key = RoutingKey::new(&key);
                classifier.pick(key).git_ref.as_str() == CANARY
                    && responder.pick(key).git_ref.as_str() == CANARY
            })
            .count();
        assert!(
            (40..=200).contains(&in_both),
            "{in_both} traces in both canaries"
        );
    }

    /// 1.5 points of tolerance: measured 9.98% on these keys before this was written.
    #[test]
    fn a_90_10_split_lands_on_the_weights() {
        const KEYS: u32 = 10_000;
        let deployment = deployment(&[(STABLE, 90), (CANARY, 10)]);
        let canary = (0..KEYS)
            .filter(|i| {
                let key = format!("trace-{i}");
                deployment.pick(RoutingKey::new(&key)).git_ref.as_str() == CANARY
            })
            .count();
        assert!(
            (850..=1_150).contains(&canary),
            "canary got {canary} of {KEYS}"
        );
    }
}
