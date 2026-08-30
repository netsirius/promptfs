//! The boundary where a file's bytes stop being bytes: frontmatter on one side, Jinja body on
//! the other. Nothing downstream — `PromptMeta`, the compiled AST — sees a prompt file whole.

use crate::error::FormatError;

const DELIMITER: &str = "---";

/// Not content: a Windows editor writes one, and it would hide the opening delimiter.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Splits a `.prompt.md` source into its YAML frontmatter and its Jinja body.
///
/// Opens with `---` on line 1, closes at the next `---` alone on a line. Everything after that
/// line is the body, verbatim: trimming would change what the model receives.
///
/// Both halves are borrowed from `src`: copying would double a resident bundle for nothing.
///
/// The body runs to the end of `src`, so a caller still holding the source recovers where the
/// body began with `src.len() - body.len()`. The frontmatter always starts on file line 2: a
/// line number from `serde_yaml_ng` is one less than the line in the file.
///
/// The error carries no path because this function never sees one; the caller wraps it into
/// [`PromptError::InvalidFormat`](crate::PromptError::InvalidFormat).
pub fn split_frontmatter(src: &str) -> Result<(&str, &str), FormatError> {
    let unmarked = src.strip_prefix(BYTE_ORDER_MARK).unwrap_or(src);

    // Anchored: a Markdown file with a horizontal rule in the middle is not a prompt file.
    let after_opening = strip_delimiter_line(unmarked).ok_or(FormatError::MissingStartDelimiter)?;

    // Derived, not counted: `DELIMITER.len() + 1` is off by one on a file with CRLF.
    let frontmatter_start = src.len() - after_opening.len();

    let (frontmatter_end, body) =
        split_at_closing_delimiter(src, frontmatter_start).ok_or(FormatError::UnterminatedBlock)?;

    Ok((&src[frontmatter_start..frontmatter_end], body))
}

/// Where the closing delimiter's line starts, and the body that follows it.
///
/// Walks whole lines because the first delimiter closes and the two delimiter lines can be
/// adjacent: `"\n---\n"` as a pattern misses the close in a file with empty frontmatter.
/// See docs/decisions.md D-020.
fn split_at_closing_delimiter(src: &str, from: usize) -> Option<(usize, &str)> {
    let mut line_start = from;
    loop {
        let rest = &src[line_start..];
        if let Some(body) = strip_delimiter_line(rest) {
            return Some((line_start, body));
        }
        // A line with no end is the last one, and this file never closed its block.
        line_start += rest.find('\n')? + 1;
    }
}

/// `text` without its first line, if that line is a delimiter — `None` otherwise.
///
/// ```text
/// "---\nbody"      -> Some("body")
/// "---  \r\nbody"  -> Some("body")   trailing blanks and CRLF belong to the line
/// "---"            -> Some("")       a delimiter at EOF still ends its line
/// "----\n"         -> None
/// "--- x\n"        -> None
/// "  ---\n"        -> None           leading blanks are content, never a delimiter
/// ```
///
/// It returns the rest instead of a `bool` so that the line ending leaves with the line it
/// belongs to: counted separately, a CRLF close is missed or leaves its `\n` on the body.
///
/// `Some` does not mean the file is valid. The caller knows whether it is asking about the
/// opening or the closing delimiter; this does not.
fn strip_delimiter_line(text: &str) -> Option<&str> {
    // Trailing blanks only. Leading ones are what would let an indented `---` inside a YAML
    // block scalar close the frontmatter. See D-020.
    let after_dashes = text
        .strip_prefix(DELIMITER)?
        .trim_start_matches([' ', '\t']);
    match after_dashes.strip_prefix('\r').unwrap_or(after_dashes) {
        // An editor that strips the final newline must not break a valid prompt.
        line if line.is_empty() => Some(line),
        line => line.strip_prefix('\n'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PromptMeta;

    /// The fixture prompt, verbatim from the `fixture-repo` skill — the same bytes task 6
    /// will read out of a Git blob.
    const CLASSIFIER: &str = r#"---
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
    const CLASSIFIER_BODY: &str = r#"You are a classification assistant.
Customer tier: {{ customer_tier }}

Classify the following and answer in JSON:
{{ user_input }}
"#;

    /// Byte offset of `part` inside `whole`, panicking unless it really is a slice of it.
    fn offset_in(part: &str, whole: &str) -> usize {
        let (part_addr, whole_addr) = (part.as_ptr() as usize, whole.as_ptr() as usize);
        assert!(
            part_addr >= whole_addr && part_addr + part.len() <= whole_addr + whole.len(),
            "{part:?} is not a slice of the input — it was copied"
        );
        part_addr - whole_addr
    }

    /// Mirrors `strip_delimiter_line`. Relax one and this has to relax with it, or the
    /// property below starts passing for the wrong reason.
    fn is_delimiter_line(s: &str) -> bool {
        s.strip_prefix("---")
            .map(|rest| rest.trim_start_matches([' ', '\t']))
            .is_some_and(|rest| matches!(rest, "" | "\n" | "\r\n"))
    }

    /// The property test runs over all of these, so a case added here is covered everywhere.
    const VALID: [&str; 7] = [
        CLASSIFIER,
        "---\n---\nHola\n",                     // empty frontmatter
        "---\nname: bare\n---",                 // closes at EOF, empty body
        "---\r\nname: crlf\r\n---\r\nHola\r\n", // Windows
        "---\nname: x\n---\n\nHola\n",          // body opens with a blank line
        "---\nname: x\n---\n---\nb\n",          // a delimiter line as the body's first line
        "---\nname: x\n---  \t\nbody\n",        // trailing blanks on the closing line
    ];

    #[test]
    fn the_frontmatter_half_parses_into_prompt_meta() {
        let (frontmatter, _) = split_frontmatter(CLASSIFIER).expect("the fixture is valid");
        let meta: PromptMeta = serde_yaml_ng::from_str(frontmatter).expect("valid frontmatter");
        assert_eq!(meta.name, "classifier");
        assert_eq!(meta.inputs, ["user_input", "customer_tier"]);
    }

    #[test]
    fn the_body_half_is_everything_after_the_closing_delimiter() {
        let (_, body) = split_frontmatter(CLASSIFIER).expect("the fixture is valid");
        assert_eq!(body, CLASSIFIER_BODY);
    }

    /// The round-trip `docs/phase-1.md` asks for: the halves are slices of the input, and the
    /// only bytes dropped between them are the two delimiter lines.
    ///
    /// `&str` in the signature is compiler-enforced; that the halves are not copies is not.
    #[test]
    fn the_halves_are_slices_and_only_delimiter_lines_are_dropped() {
        for src in VALID {
            let (frontmatter, body) = split_frontmatter(src).expect("valid fixture");
            let frontmatter_start = offset_in(frontmatter, src);
            let body_start = offset_in(body, src);
            let opening = &src[..frontmatter_start];
            let closing = &src[frontmatter_start + frontmatter.len()..body_start];

            assert!(
                is_delimiter_line(opening),
                "dropped more than a line: {opening:?}"
            );
            assert!(
                is_delimiter_line(closing),
                "dropped more than a line: {closing:?}"
            );
            assert_eq!(
                body_start + body.len(),
                src.len(),
                "the body must reach EOF"
            );
        }
    }

    /// The two delimiter lines are adjacent here, so there is no `\n` in front of the close.
    /// Anything searching for `"\n---\n"` reports this file as unterminated.
    #[test]
    fn an_empty_frontmatter_is_valid() {
        let (frontmatter, body) = split_frontmatter("---\n---\nHola\n").expect("valid, if useless");
        assert_eq!(frontmatter, "");
        assert_eq!(body, "Hola\n");
    }

    /// `---` is a horizontal rule in Markdown and prompt bodies are prose. Taking the *last*
    /// delimiter would split here and send prose to `serde_yaml_ng`. See D-020.
    #[test]
    fn a_horizontal_rule_in_the_body_is_not_a_delimiter() {
        const REPORT: &str = "---\nname: report\n---\nResume esto:\n\n---\n\nY concluye.\n";
        let (frontmatter, body) = split_frontmatter(REPORT).expect("the rule is body text");
        assert_eq!(frontmatter, "name: report\n");
        assert_eq!(body, "Resume esto:\n\n---\n\nY concluye.\n");
    }

    /// A repo cloned on Windows carries `\r\n`, and the body's endings must survive untouched:
    /// it is the text sent to a model.
    #[test]
    fn crlf_delimiters_are_accepted_and_the_body_keeps_its_line_endings() {
        let (frontmatter, body) =
            split_frontmatter("---\r\nname: crlf\r\n---\r\nHola\r\n").expect("valid on Windows");
        assert_eq!(frontmatter, "name: crlf\r\n");
        let meta: PromptMeta = serde_yaml_ng::from_str(frontmatter).expect("YAML tolerates \\r");
        assert_eq!(meta.name, "crlf");
        assert_eq!(body, "Hola\r\n");
    }

    /// An empty body is a content problem, not a format one: `contract-check` can warn about
    /// it without failing a render in production.
    #[test]
    fn a_closing_delimiter_at_eof_leaves_an_empty_body() {
        let (frontmatter, body) = split_frontmatter("---\nname: bare\n---").expect("block closes");
        assert_eq!(frontmatter, "name: bare\n");
        assert_eq!(body, "");
    }

    /// `&src[a..b]` panics if a bound falls inside a multi-byte character, and a panic here is
    /// a customer's outage. Every offset lands just past a `\n`, which is always a boundary.
    #[test]
    fn multi_byte_characters_do_not_split_mid_character() {
        const ACCENTS: &str = "---\nname: acentos\ndescription: Resumé ñ 日本語\n---\n¡Hola! 🎯\n";
        let (frontmatter, body) = split_frontmatter(ACCENTS).expect("valid, and not ASCII");
        let meta: PromptMeta = serde_yaml_ng::from_str(frontmatter).expect("valid frontmatter");
        assert_eq!(meta.name, "acentos");
        assert_eq!(meta.description.as_deref(), Some("Resumé ñ 日本語"));
        assert_eq!(body, "¡Hola! 🎯\n");
    }

    /// Without this, nothing in the suite would notice a `trim_start` on the body slice.
    #[test]
    fn a_body_that_opens_with_a_blank_line_keeps_it() {
        let (_, body) = split_frontmatter("---\nname: x\n---\n\nHola\n").expect("valid");
        assert_eq!(body, "\nHola\n");
    }

    /// Trailing blanks are invisible in an editor; leading ones stay fatal. That asymmetry is
    /// the rule, and D-020 has the argument.
    #[test]
    fn trailing_blanks_close_but_leading_ones_do_not() {
        let (frontmatter, body) =
            split_frontmatter("---\nname: x\n---  \t\nb\n").expect("trailing blanks close");
        assert_eq!(frontmatter, "name: x\n");
        assert_eq!(body, "b\n");

        let err =
            split_frontmatter("---\nname: x\n  ---\nb\n").expect_err("indented closes nothing");
        assert!(matches!(err, FormatError::UnterminatedBlock), "{err:?}");
    }

    /// D-020's accepted cost, pinned: a block scalar's content is indented, so it cannot
    /// produce a delimiter at column 0. Only a document separator there closes early.
    #[test]
    fn an_indented_delimiter_inside_a_block_scalar_does_not_close() {
        const SCALAR: &str = "---\ndescription: |\n  uno\n  ---\n  dos\n---\nBody\n";
        let (frontmatter, body) = split_frontmatter(SCALAR).expect("the indented rule is YAML");
        let meta: PromptMeta = serde_yaml_ng::from_str(&format!("name: x\n{frontmatter}"))
            .expect("the block scalar survived intact");
        assert_eq!(meta.description.as_deref(), Some("uno\n---\ndos\n"));
        assert_eq!(body, "Body\n");
    }

    /// Rejecting it would tell an author their perfectly valid file is not a prompt file.
    #[test]
    fn a_byte_order_mark_does_not_hide_the_opening_delimiter() {
        let (frontmatter, body) =
            split_frontmatter("\u{feff}---\nname: bom\n---\nHola\n").expect("BOM is not content");
        assert_eq!(frontmatter, "name: bom\n");
        assert_eq!(body, "Hola\n");
    }

    /// "This is not a prompt file" and "this prompt file is broken" send the user to
    /// different places, so they are different variants.
    #[test]
    fn a_file_with_no_opening_delimiter_is_not_a_prompt_file() {
        for src in [
            "name: classifier\n",
            "",
            "----\n",
            "  ---\nname: x\n---\n",
            "--- x\n",
        ] {
            let err = split_frontmatter(src).expect_err("no block to split");
            assert!(
                matches!(err, FormatError::MissingStartDelimiter),
                "wrong variant for {src:?}: {err:?}"
            );
        }
    }

    #[test]
    fn a_block_that_never_closes_is_a_broken_prompt_file() {
        // The lone `---` is the interesting one: the open consumed the whole file.
        for src in ["---\nname: classifier\n", "---"] {
            let err = split_frontmatter(src).expect_err("block never closes");
            // A broken file must not come back as an empty body.
            assert!(
                matches!(err, FormatError::UnterminatedBlock),
                "wrong variant for {src:?}: {err:?}"
            );
        }
    }
}
