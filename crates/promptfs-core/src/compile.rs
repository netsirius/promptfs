//! A prompt file, compiled once and rendered many times.
//!
//! The one place in the workspace that builds a minijinja `Environment` (invariant 3): the
//! server, the wheel and the Studio preview all get their escaping and their undefined policy
//! from here, or they do not get them at all.

use minijinja::{AutoEscape, Environment};
use serde::Serialize;

use crate::error::PromptError;
use crate::meta::PromptMeta;
use crate::parse::split_frontmatter;

/// Line 1 is the opening `---`, so the YAML block always starts on line 2.
const FRONTMATTER_FIRST_LINE: usize = 2;

/// A prompt whose frontmatter is parsed and whose body is compiled — the unit the server
/// caches and the SDK holds resident. Built once per source, rendered per call.
#[derive(Debug)]
pub struct CompiledPrompt {
    /// Repo-relative path: the template's name inside the environment, and what every error
    /// names.
    prompt_path: String,
    meta: PromptMeta,
    env: Environment<'static>,
}

impl CompiledPrompt {
    /// Splits `source`, parses the frontmatter and compiles the body, in that order. Every
    /// error names `prompt_path` and, where there is one, the line *in the file*: the two
    /// helpers at the bottom rebase the block-relative lines serde and minijinja report.
    ///
    /// Two settings go on the environment **before** the template is added, because minijinja
    /// fixes both when it compiles it — the other order compiles with the defaults and the
    /// settings guard nothing:
    ///
    /// - autoescape off (invariant 3): the default is chosen from the template *name's*
    ///   extension, and the name here is a repo path;
    /// - keep the trailing newline: the default strips it, and the body is verbatim — that
    ///   newline is part of what the model receives (`CLASSIFIER_BODY` pins it).
    ///
    /// `source` is `&str`, not bytes: a file that is not UTF-8 is the caller's error to
    /// report, with the path still in hand.
    pub fn compile(prompt_path: &str, source: &str) -> Result<Self, PromptError> {
        let (frontmatter, body) =
            split_frontmatter(source).map_err(|cause| PromptError::InvalidFormat {
                prompt_path: prompt_path.to_string(),
                cause,
            })?;

        let meta: PromptMeta = serde_yaml_ng::from_str(frontmatter).map_err(|cause| {
            PromptError::InvalidFrontmatter {
                prompt_path: prompt_path.to_string(),
                line: cause
                    .location()
                    .map(|at| frontmatter_line_in_file(at.line())),
                cause,
            }
        })?;

        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| AutoEscape::None);
        env.set_keep_trailing_newline(true);
        env.add_template_owned(prompt_path.to_string(), body.to_string())
            .map_err(|cause| PromptError::InvalidTemplate {
                prompt_path: prompt_path.to_string(),
                line: cause.line().map(|at| body_line_in_file(source, body, at)),
                cause,
            })?;

        Ok(Self {
            prompt_path: prompt_path.to_string(),
            meta,
            env,
        })
    }

    /// Renders the compiled body with `variables`.
    ///
    /// This is the call path of invariant 6: looking the template up is a map lookup and
    /// rendering allocates the output `String` and nothing else. Anything that parses or
    /// compiles belongs in [`compile`](Self::compile).
    pub fn render(&self, variables: impl Serialize) -> Result<String, PromptError> {
        let template = self.env.get_template(&self.prompt_path).map_err(|cause| {
            PromptError::RenderFailed {
                prompt_path: self.prompt_path.clone(),
                cause,
            }
        })?;

        template
            .render(variables)
            .map_err(|cause| PromptError::RenderFailed {
                prompt_path: self.prompt_path.clone(),
                cause,
            })
    }

    /// The frontmatter, parsed once at compile time. `model` and `temperature` travel with
    /// the render result so the caller configures its client from the prompt.
    pub fn meta(&self) -> &PromptMeta {
        &self.meta
    }
}

/// File line of a line serde reported inside the YAML block.
fn frontmatter_line_in_file(line_in_frontmatter: usize) -> usize {
    line_in_frontmatter + FRONTMATTER_FIRST_LINE - 1
}

/// File line of a line minijinja reported inside the body.
///
/// The body is the tail of `source` (`split_frontmatter` guarantees it), so the lines before
/// it are counted once here, at compile time, and never on the render path.
fn body_line_in_file(source: &str, body: &str, line_in_body: usize) -> usize {
    let body_start = source.len() - body.len();
    source[..body_start].matches('\n').count() + line_in_body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FormatError;
    use crate::fixtures::{CLASSIFIER, CLASSIFIER_PATH};
    use minijinja::context;

    /// A render variable: caller data.
    const SECRET: &str = "CARD-4111-1111-1111";

    fn classifier() -> CompiledPrompt {
        CompiledPrompt::compile(CLASSIFIER_PATH, CLASSIFIER).expect("the fixture compiles")
    }

    /// The test this task exists for. A `CompiledPrompt` that still borrows `source` passes
    /// every other test here — the fixtures are `&'static str` — and only fails when the
    /// source is dropped before the render, which is what a cache or a thread hand-off does.
    #[test]
    fn the_source_may_die_before_the_first_render() {
        let prompt = {
            let owned = String::from(CLASSIFIER);
            CompiledPrompt::compile(CLASSIFIER_PATH, &owned)
        };
        let body = prompt
            .expect("compiles")
            .render(context! { user_input => "Where is my order?", customer_tier => "gold" })
            .expect("renders after its source is gone");
        assert!(body.contains("Customer tier: gold"), "{body}");
    }

    #[test]
    fn compiles_once_and_renders_twice_with_different_variables() {
        let prompt = classifier();
        let gold = prompt
            .render(context! { user_input => "Where is my order?", customer_tier => "gold" })
            .expect("first render");
        let free = prompt
            .render(context! { user_input => "Cancel my plan.", customer_tier => "free" })
            .expect("second render");
        assert_eq!(
            gold,
            "You are a classification assistant.\nCustomer tier: gold\n\n\
             Classify the following and answer in JSON:\nWhere is my order?\n"
        );
        assert_eq!(
            free,
            "You are a classification assistant.\nCustomer tier: free\n\n\
             Classify the following and answer in JSON:\nCancel my plan.\n"
        );
    }

    #[test]
    fn exposes_the_parsed_frontmatter() {
        let prompt = classifier();
        assert_eq!(prompt.meta().name, "classifier");
        assert_eq!(prompt.meta().model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(prompt.meta().inputs, ["user_input", "customer_tier"]);
    }

    /// Invariant 3. minijinja picks escaping from the template name's extension, and the
    /// name here is a path from a repository we do not control. The literal `&` in the body
    /// would survive either way; the one inside the variable is what escaping mangles.
    #[test]
    fn a_prompt_named_html_does_not_escape_ampersands() {
        const HTML_NAMED: &str = "---\nname: html\n---\nTom & Jerry, {{ who }}\n";
        let prompt =
            CompiledPrompt::compile("prompts/x/html.prompt.html", HTML_NAMED).expect("compiles");
        let body = prompt
            .render(context! { who => "Ben & Holly" })
            .expect("renders");
        assert_eq!(body, "Tom & Jerry, Ben & Holly\n");
    }

    #[test]
    fn a_file_with_no_frontmatter_is_wrapped_with_its_path() {
        let err = CompiledPrompt::compile(CLASSIFIER_PATH, "no frontmatter here\n")
            .expect_err("not a prompt file");
        assert!(
            matches!(
                &err,
                PromptError::InvalidFormat { prompt_path, cause: FormatError::MissingStartDelimiter }
                    if prompt_path == CLASSIFIER_PATH
            ),
            "{err:?}"
        );
    }

    /// serde reports line 2 of the block; the author's editor shows line 3 of the file.
    #[test]
    fn invalid_frontmatter_names_the_file_line() {
        const BAD_TEMPERATURE: &str = "---\nname: x\ntemperature: warm\n---\nbody\n";
        let err = CompiledPrompt::compile(CLASSIFIER_PATH, BAD_TEMPERATURE)
            .expect_err("warm is not an f32");
        match err {
            PromptError::InvalidFrontmatter {
                prompt_path, line, ..
            } => {
                assert_eq!(prompt_path, CLASSIFIER_PATH);
                assert_eq!(line, Some(3), "file line, not YAML line");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// A missing `name` is the same failure as a wrong type: the frontmatter is not a
    /// `PromptMeta`. Not a variant of its own.
    #[test]
    fn a_missing_name_is_invalid_frontmatter() {
        const NAMELESS: &str = "---\ndescription: no name\n---\nbody\n";
        let err = CompiledPrompt::compile(CLASSIFIER_PATH, NAMELESS).expect_err("name is required");
        assert!(
            matches!(err, PromptError::InvalidFrontmatter { .. }),
            "{err:?}"
        );
    }

    /// minijinja reports line 2 of the body; the file has three lines before it.
    #[test]
    fn a_body_that_does_not_compile_names_the_file_line() {
        const BAD_BLOCK: &str = "---\nname: x\n---\nfine\n{% if %}\n";
        let err = CompiledPrompt::compile(CLASSIFIER_PATH, BAD_BLOCK).expect_err("if needs a test");
        match err {
            PromptError::InvalidTemplate {
                prompt_path, line, ..
            } => {
                assert_eq!(prompt_path, CLASSIFIER_PATH);
                assert_eq!(line, Some(5), "file line, not body line");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// Invariant 2 on the render path, end to end: the failing operand *is* the caller's
    /// value and the error still does not contain it.
    #[test]
    fn a_render_failure_carries_no_variable_values() {
        const ADDS_A_STRING: &str = "---\nname: x\n---\n{{ card + 1 }}\n";
        let prompt = CompiledPrompt::compile(CLASSIFIER_PATH, ADDS_A_STRING).expect("compiles");
        let err = prompt
            .render(context! { card => SECRET })
            .expect_err("string + number is an invalid operation");
        assert!(matches!(err, PromptError::RenderFailed { .. }), "{err:?}");
        assert!(!err.to_string().contains(SECRET), "Display leaked: {err}");
        assert!(
            !format!("{err:?}").contains(SECRET),
            "Debug leaked: {err:?}"
        );
    }

    #[test]
    fn line_helpers_count_from_the_top_of_the_file() {
        assert_eq!(frontmatter_line_in_file(1), 2);
        let source = "---\nname: x\n---\nfirst\nsecond\n";
        let body = &source[source.len() - "first\nsecond\n".len()..];
        assert_eq!(body_line_in_file(source, body, 1), 4);
        assert_eq!(body_line_in_file(source, body, 2), 5);
    }
}
