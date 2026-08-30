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
#[derive(Debug, Error)]
pub enum PromptError {
    #[error("prompt file {prompt_path} is not valid: {cause}")]
    InvalidFormat {
        prompt_path: String,
        cause: FormatError,
    },
    #[error("input {input_name} was not provided")]
    InputNotFound {
        /// The name, never the value: render variables are caller data (invariant 2).
        input_name: String,
    },
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
}
