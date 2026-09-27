//! Test fixtures shared across the crate's test modules. One copy of each prompt: two copies
//! is how the split test and the render test end up agreeing on different files.

/// Repo-relative path of the fixture prompt: the template's name and what errors name.
pub(crate) const CLASSIFIER_PATH: &str = "prompts/support/classifier.prompt.md";

/// The fixture prompt, verbatim from the `fixture-repo` skill and `docs/architecture.md` §4 —
/// the same bytes issue #6 will read out of a Git blob.
pub(crate) const CLASSIFIER: &str = r#"---
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

/// The blank line and the trailing newline are part of the prompt the model sees.
pub(crate) const CLASSIFIER_BODY: &str = r#"You are a classification assistant.
Customer tier: {{ customer_tier }}

Classify the following and answer in JSON:
{{ user_input }}
"#;
