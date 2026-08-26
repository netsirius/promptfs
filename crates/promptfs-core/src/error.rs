//! The error every fallible function in the core returns.
//!
//! This module is where invariant 2 stops being a rule someone reviews and starts being a
//! property of a type. An error names *which* input was missing, never what it held.
//!
//! Grown one variant at a time. Each failure the core learns to report adds a variant here
//! and one entry to `every_variant()` in the tests, which is what keeps the leak check
//! below honest.

use thiserror::Error;

/// Everything that can go wrong turning a prompt file into a string.
#[derive(Debug, Error)]
pub enum PromptError {
    #[error("input {input_name:?} was not provided")]
    InputNotFound {
        /// The name of the input that was missing.
        input_name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for a render variable: caller data, potentially PII. If this string can
    /// be made to appear in an error, invariant 2 is broken.
    const SECRET: &str = "CARD-4111-1111-1111";

    /// Stands in for the *name* of a declared input, which an error may carry.
    const INPUT_NAME: &str = "customer_tier";

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
                display.contains(INPUT_NAME),
                "does not name the input it is about: {display}"
            );
        }
    }

    /// Guards the `Cargo.toml`, not this module: minijinja's `debug` feature appends the
    /// caller's variables to a render error's `Debug` output. It is off (D-018), and
    /// nothing in `check-invariants.sh` would notice it coming back.
    #[test]
    fn minijinja_errors_carry_no_variables() {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template_owned("t.prompt.md", "{{ card.number }}".to_string())
            .expect("template compiles");
        let err = env
            .get_template("t.prompt.md")
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
