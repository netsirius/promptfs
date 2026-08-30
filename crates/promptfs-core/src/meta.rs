//! The frontmatter half of a `.prompt.md`.
//!
//! Splitting a file into this and its Jinja body is task 3; this module only describes the
//! shape the YAML deserializes into.

use serde::Deserialize;

/// A prompt's frontmatter, deserialized from the YAML block the file opens with.
///
/// The managed-repo contract fixes these field names: a rename breaks repositories we do not
/// control. Unknown fields are accepted rather than rejected — see docs/decisions.md D-021.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PromptMeta {
    /// Not an address. A prompt is addressed as `<namespace>/<name>`, and the namespace comes
    /// from the file's directory, not from here.
    pub name: String,

    #[serde(default)]
    pub description: Option<String>,

    /// Advisory: PromptFS never calls a model. It travels in the render result so the caller
    /// configures their client from the prompt instead of from their own code.
    #[serde(default)]
    pub model: Option<String>,

    /// Advisory, like `model`.
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Declared by the author, not inferred from the template: that is what lets a missing
    /// input be caught before rendering starts, with a better error than the engine gives.
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontmatter of the fixture prompt, verbatim from the `fixture-repo` skill.
    const FRONTMATTER: &str = "\
name: classifier
description: Classifies user intent for technical support.
model: gpt-4o-mini
temperature: 0.1
inputs:
  - user_input
  - customer_tier
";

    #[test]
    fn deserializes_the_fixture_frontmatter() {
        let meta: PromptMeta = serde_yaml_ng::from_str(FRONTMATTER).expect("valid frontmatter");
        assert_eq!(meta.name, "classifier");
        assert_eq!(meta.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(meta.inputs, ["user_input", "customer_tier"]);
    }

    #[test]
    fn only_name_is_required() {
        let meta: PromptMeta = serde_yaml_ng::from_str("name: bare").expect("name alone is valid");
        assert_eq!(meta.description, None);
        assert!(meta.inputs.is_empty());
    }

    /// The wheel doing the reading cannot be made to upgrade, so a field added later must not
    /// fail a file written for an older version. See D-021.
    #[test]
    fn an_unknown_field_does_not_fail_the_parse() {
        let meta: PromptMeta = serde_yaml_ng::from_str("name: bare\nfuture_field: 7\n")
            .expect("unknown fields are tolerated");
        assert_eq!(meta.name, "bare");
    }
}
