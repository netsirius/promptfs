//! The error every fallible function in the core returns.
//!
//! This module is where invariant 2 stops being a rule someone reviews and starts being a
//! property of a type. An error names *which* input was missing, never what it held.
//!
//! Grown one variant at a time. Each failure the core learns to report adds a variant here
//! and one entry to `every_variant()` in the tests, which is what keeps the leak check
//! below honest.

use thiserror::Error;

/// A prompt file that does not have the shape of a prompt file. Carries no path: the parser
/// never sees one, and the caller that read the bytes wraps this into `InvalidFormat`.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("line 1 must be exactly `---`")]
    MissingStartDelimiter,
    /// No line number: the opening is always line 1, and the only informative line would be
    /// one that looks like a delimiter and is not — which costs a second scan to find.
    #[error("the block is never closed by a `---` line")]
    UnterminatedBlock,
}

/// Everything that can go wrong turning a prompt file into a string.
///
/// Variants follow the pipeline: the file has no frontmatter block, the block is not a
/// `PromptMeta`, the body does not compile, the compiled body does not render.
#[derive(Debug, Error)]
pub enum PromptError {
    #[error("prompt file {prompt_path} is not valid: {cause}")]
    InvalidFormat {
        prompt_path: String,
        cause: FormatError,
    },
    /// Valid YAML that is not a `PromptMeta` — a wrong type, a missing `name` — or not YAML
    /// at all. One variant: the author fixes all of them in the same place.
    #[error("prompt file {prompt_path}: invalid frontmatter{}", at_line(*.line))]
    InvalidFrontmatter {
        prompt_path: String,
        /// Counted in the file, not in the YAML block, which opens on line 2.
        line: Option<usize>,
        // `#[source]`, never `{cause}` in the message: serde prints its own line, counted
        // from the top of the block, and two disagreeing numbers in one string send the
        // author to the wrong one.
        #[source]
        cause: serde_yaml_ng::Error,
    },
    #[error("prompt file {prompt_path}: template does not compile{}", at_line(*.line))]
    InvalidTemplate {
        prompt_path: String,
        /// Counted in the file, not in the body.
        line: Option<usize>,
        // `#[source]` for the same reason as above: minijinja counts from the body's first line.
        #[source]
        cause: minijinja::Error,
    },
    /// A compiled body that fails while rendering, for any reason other than a missing input.
    #[error("prompt file {prompt_path}: render failed")]
    RenderFailed {
        prompt_path: String,
        /// Names kinds, types and template lines, never a variable's value — pinned by
        /// `render_errors_name_types_not_values` below.
        #[source]
        cause: minijinja::Error,
    },
    #[error("input {input_name} was not provided")]
    InputNotFound {
        /// The name, never the value: render variables are caller data (invariant 2).
        input_name: String,
    },
}

fn at_line(line: Option<usize>) -> String {
    line.map_or_else(String::new, |line| format!(" at line {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A render variable: caller data. If it can be made to appear in an error, invariant 2
    /// is broken.
    const SECRET: &str = "CARD-4111-1111-1111";

    /// Stands in for the *name* of a declared input, which an error may carry.
    const INPUT_NAME: &str = "customer_tier";

    /// Stands in for the path of a prompt file, which an error may carry.
    const PROMPT_PATH: &str = "t.prompt.md";

    /// One entry per variant of `PromptError`. The tests below are only as strong as this
    /// list, so it grows with the enum.
    fn every_variant() -> Vec<PromptError> {
        vec![
            // `input_name` is a name, not caller data — it comes from the prompt's own
            // `inputs:` declaration. There is deliberately nowhere here to put SECRET,
            // which is why this variant cannot leak.
            PromptError::InputNotFound {
                input_name: INPUT_NAME.to_string(),
            },
            PromptError::InvalidFormat {
                prompt_path: PROMPT_PATH.to_string(),
                cause: FormatError::MissingStartDelimiter,
            },
            PromptError::InvalidFormat {
                prompt_path: PROMPT_PATH.to_string(),
                cause: FormatError::UnterminatedBlock,
            },
            PromptError::InvalidFrontmatter {
                prompt_path: PROMPT_PATH.to_string(),
                line: Some(3),
                cause: serde_yaml_ng::from_str::<crate::PromptMeta>("[").expect_err("not a map"),
            },
            PromptError::InvalidTemplate {
                prompt_path: PROMPT_PATH.to_string(),
                line: None,
                cause: minijinja::Error::new(minijinja::ErrorKind::SyntaxError, "unexpected `}}`"),
            },
            PromptError::RenderFailed {
                prompt_path: PROMPT_PATH.to_string(),
                cause: minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "string + number",
                ),
            },
        ]
    }

    #[test]
    fn every_variant_is_listed() {
        assert!(
            !every_variant().is_empty(),
            "no variants listed — the checks below pass vacuously until this list is real"
        );
    }

    /// Invariant 2, enforced rather than reviewed. `{:?}` is what `tracing`, `anyhow` and
    /// a panicking `unwrap` all reach for, so `Debug` is checked as well as `Display`.
    #[test]
    fn no_variant_leaks_caller_data() {
        for e in every_variant() {
            let display = e.to_string();
            let debug = format!("{e:?}");
            assert!(!display.contains(SECRET), "Display leaked: {display}");
            assert!(!debug.contains(SECRET), "Debug leaked: {debug}");
        }
    }

    /// An error the user cannot act on is a bug report about us.
    #[test]
    fn the_error_names_the_input() {
        for e in every_variant() {
            let display = e.to_string();
            assert!(
                display.contains(INPUT_NAME) || display.contains(PROMPT_PATH),
                "does not name the input it is about: {display}"
            );
        }
    }

    /// Guards the `Cargo.toml`, not this module: minijinja's `debug` feature appends the
    /// caller's variables to a render error's `Debug`. It is off (D-018) and unguarded.
    #[test]
    fn minijinja_errors_carry_no_variables() {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template_owned(PROMPT_PATH, "{{ card.number }}".to_string())
            .expect("template compiles");
        let err = env
            .get_template(PROMPT_PATH)
            .expect("template is registered")
            .render(minijinja::context! { card => SECRET })
            .expect_err("strict undefined must fail");

        let debug = format!("{err:?}");
        assert!(
            !debug.contains(SECRET),
            "minijinja leaked a render variable — is the `debug` feature back on? {debug}"
        );
    }

    /// What lets `RenderFailed` carry the `minijinja::Error` whole: a failure whose operand
    /// *is* the caller's value still reports only its type. If this ever fails, the variant
    /// has to shrink to `kind` and `line`.
    #[test]
    fn render_errors_name_types_not_values() {
        let mut env = minijinja::Environment::new();
        env.add_template_owned(PROMPT_PATH, "{{ card + 1 }}".to_string())
            .expect("template compiles");
        let err = env
            .get_template(PROMPT_PATH)
            .expect("template is registered")
            .render(minijinja::context! { card => SECRET })
            .expect_err("string + number is an invalid operation");

        let display = err.to_string();
        let debug = format!("{err:?}");
        assert!(
            display.contains("string"),
            "should name the type: {display}"
        );
        assert!(!display.contains(SECRET), "Display leaked: {display}");
        assert!(!debug.contains(SECRET), "Debug leaked: {debug}");
    }
}
