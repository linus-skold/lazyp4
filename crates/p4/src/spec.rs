//! Editing Perforce spec forms.
//!
//! `p4 <spec> -o` prints a form and `p4 <spec> -i` reads one back. A form is
//! comment lines, then fields at column zero whose values are tab-indented:
//!
//! ```text
//! Change:\t395
//!
//! Description:
//! \t# Do not submit - local only
//!
//! Files:
//! \t//depot/main/AGENTS.md\t# add
//! ```
//!
//! Only the field being changed is touched; every other byte of the form goes
//! back to the server as it arrived, so read-only and unknown fields survive.

/// Value of `field`, with the tab indent removed.
pub fn field(form: &str, field: &str) -> Option<String> {
    let lines: Vec<&str> = form.lines().collect();
    let start = find_field(&lines, field)?;
    let end = field_end(&lines, start);

    // `Name:\tvalue` on one line, or `Name:` with the value indented below.
    let head = lines[start]
        .split_once(':')
        .map(|(_, rest)| rest.trim())
        .unwrap_or_default();

    let mut out: Vec<String> = if head.is_empty() {
        Vec::new()
    } else {
        vec![head.to_owned()]
    };
    out.extend(
        lines[start + 1..end]
            .iter()
            .map(|l| l.strip_prefix('\t').unwrap_or(l).to_owned()),
    );

    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    Some(out.join("\n"))
}

/// Replace `field`'s value, leaving the rest of the form untouched.
///
/// Returns the form unchanged when it has no such field.
pub fn set_field(form: &str, field: &str, value: &str) -> String {
    let lines: Vec<&str> = form.lines().collect();
    let Some(start) = find_field(&lines, field) else {
        return form.to_owned();
    };
    let end = field_end(&lines, start);

    let mut out: Vec<String> = lines[..start].iter().map(|l| (*l).to_owned()).collect();
    out.push(format!("{field}:"));
    for line in value.lines() {
        out.push(format!("\t{line}"));
    }
    // Fields are separated by a blank line.
    out.push(String::new());
    out.extend(lines[end..].iter().map(|l| (*l).to_owned()));

    let mut text = out.join("\n");
    text.push('\n');
    text
}

fn find_field(lines: &[&str], field: &str) -> Option<usize> {
    let head = format!("{field}:");
    lines.iter().position(|l| l.starts_with(&head))
}

/// One past the last line of the field starting at `start`.
///
/// The value runs until the next line that begins at column zero with
/// something other than whitespace — the next field, or a comment.
fn field_end(lines: &[&str], start: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| !l.is_empty() && !l.starts_with([' ', '\t']))
        .map(|(i, _)| i)
        .unwrap_or(lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape captured from `p4 change -o 395`.
    const FORM: &str = "\
# A Perforce Change Specification.
#
#  Change:      The change number.

Change:\t395

Date:\t2026/09/08 19:26:09

Client:\tlinsko_linus-desktop_9415

Status:\tpending

Description:
\t# Do not submit - local only

Files:
\t//darksim/main/AGENTS.md\t# add
";

    #[test]
    fn reads_an_indented_field() {
        assert_eq!(
            field(FORM, "Description").as_deref(),
            Some("# Do not submit - local only")
        );
    }

    #[test]
    fn reads_a_field_that_shares_its_line() {
        assert_eq!(field(FORM, "Change").as_deref(), Some("395"));
        assert_eq!(field(FORM, "Status").as_deref(), Some("pending"));
    }

    #[test]
    fn reads_a_multi_line_field() {
        let form = "Description:\n\tfirst\n\tsecond\n\nFiles:\n\t//a\n";
        assert_eq!(field(form, "Description").as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn a_missing_field_reads_as_none() {
        assert_eq!(field(FORM, "Jobs"), None);
    }

    #[test]
    fn replaces_a_description_and_keeps_everything_else() {
        let out = set_field(FORM, "Description", "a better message");

        assert_eq!(field(&out, "Description").as_deref(), Some("a better message"));
        // Nothing else may shift.
        assert_eq!(field(&out, "Change").as_deref(), Some("395"));
        assert_eq!(field(&out, "Status").as_deref(), Some("pending"));
        assert!(out.contains("//darksim/main/AGENTS.md\t# add"));
        assert!(out.starts_with("# A Perforce Change Specification."));
    }

    #[test]
    fn writes_a_multi_line_description_with_tabs() {
        let out = set_field(FORM, "Description", "one\ntwo");
        assert!(out.contains("Description:\n\tone\n\ttwo\n\nFiles:"), "{out}");
    }

    #[test]
    fn round_trips_a_description_containing_blank_lines() {
        let out = set_field(FORM, "Description", "para one\n\npara two");
        assert_eq!(
            field(&out, "Description").as_deref(),
            Some("para one\n\npara two")
        );
        assert!(field(&out, "Files").is_some(), "the next field survives");
    }

    #[test]
    fn an_unknown_field_leaves_the_form_alone() {
        assert_eq!(set_field(FORM, "Nonexistent", "x"), FORM);
    }

    #[test]
    fn replacing_the_last_field_keeps_the_form_valid() {
        let out = set_field(FORM, "Files", "//depot/other.cpp\t# edit");
        assert_eq!(
            field(&out, "Files").as_deref(),
            Some("//depot/other.cpp\t# edit")
        );
        assert_eq!(field(&out, "Description").as_deref(), Some("# Do not submit - local only"));
    }
}
