//! Turning a unified diff into rows the Diff panel can draw.
//!
//! Two things a raw `@@` dump does not give you, and which make a diff
//! readable: the line number each side is at, and — where a line was rewritten
//! rather than replaced — which words inside it actually changed.

use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// The `@@ -a,b +c,d @@` line.
    Header,
    Context,
    Add,
    Delete,
    /// `\ No newline at end of file`, and anything else uncounted.
    Note,
}

/// A run of text, and whether it differs from the line it is paired with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub changed: bool,
}

impl Segment {
    fn plain(text: impl Into<String>) -> Self {
        Segment {
            text: text.into(),
            changed: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub segments: Vec<Segment>,
}

impl Row {
    /// The row's text without the leading diff marker.
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn marker(&self) -> char {
        match self.kind {
            RowKind::Add => '+',
            RowKind::Delete => '-',
            RowKind::Header | RowKind::Note => ' ',
            RowKind::Context => ' ',
        }
    }
}

/// Parse unified-diff hunks into numbered rows with intra-line highlighting.
pub fn rows(hunks: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;

    // A run of deletions immediately followed by additions is a rewrite; the
    // two are compared word by word once the run ends.
    let mut pending = 0usize;

    for line in hunks.lines() {
        if let Some((old, new)) = parse_header(line) {
            highlight_run(&mut rows, &mut pending);
            old_no = old;
            new_no = new;
            rows.push(Row {
                kind: RowKind::Header,
                old_no: None,
                new_no: None,
                segments: vec![Segment::plain(line)],
            });
            continue;
        }

        let (kind, body) = match line.chars().next() {
            Some('+') => (RowKind::Add, &line[1..]),
            Some('-') => (RowKind::Delete, &line[1..]),
            Some('\\') => (RowKind::Note, line),
            Some(' ') => (RowKind::Context, &line[1..]),
            // Perforce writes an empty context line bare, with no leading space.
            None => (RowKind::Context, line),
            _ => (RowKind::Context, line),
        };

        match kind {
            RowKind::Add => {
                rows.push(Row {
                    kind,
                    old_no: None,
                    new_no: Some(new_no),
                    segments: vec![Segment::plain(body)],
                });
                new_no += 1;
                pending += 1;
            }
            RowKind::Delete => {
                rows.push(Row {
                    kind,
                    old_no: Some(old_no),
                    new_no: None,
                    segments: vec![Segment::plain(body)],
                });
                old_no += 1;
                pending += 1;
            }
            RowKind::Note => {
                rows.push(Row {
                    kind,
                    old_no: None,
                    new_no: None,
                    segments: vec![Segment::plain(body)],
                });
            }
            _ => {
                highlight_run(&mut rows, &mut pending);
                rows.push(Row {
                    kind: RowKind::Context,
                    old_no: Some(old_no),
                    new_no: Some(new_no),
                    segments: vec![Segment::plain(body)],
                });
                old_no += 1;
                new_no += 1;
            }
        }
    }

    highlight_run(&mut rows, &mut pending);
    rows
}

/// Word-diff the run of changed rows that just ended.
///
/// Only an equal number of deletions and additions is paired up. Anything else
/// is a genuine insertion or removal, where highlighting every word as changed
/// would say nothing.
fn highlight_run(rows: &mut [Row], pending: &mut usize) {
    let run = std::mem::take(pending);
    if run == 0 {
        return;
    }
    let start = rows.len() - run;
    let deletes = rows[start..]
        .iter()
        .filter(|r| r.kind == RowKind::Delete)
        .count();
    let adds = run - deletes;
    if deletes == 0 || deletes != adds {
        return;
    }
    // Deletions always precede the additions they pair with.
    if rows[start..start + deletes]
        .iter()
        .any(|r| r.kind != RowKind::Delete)
    {
        return;
    }

    for i in 0..deletes {
        let old = rows[start + i].text();
        let new = rows[start + deletes + i].text();
        let (old_segs, new_segs) = word_diff(&old, &new);
        rows[start + i].segments = old_segs;
        rows[start + deletes + i].segments = new_segs;
    }
}

/// Split two lines into matching and differing runs.
fn word_diff(old: &str, new: &str) -> (Vec<Segment>, Vec<Segment>) {
    // Unicode segmentation, so `b);` splits into `b`, `)`, `;` rather than
    // marking the punctuation as changed along with the identifier.
    let diff = TextDiff::from_unicode_words(old, new);
    let mut old_segs: Vec<Segment> = Vec::new();
    let mut new_segs: Vec<Segment> = Vec::new();

    for change in diff.iter_all_changes() {
        let text = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                push(&mut old_segs, text, false);
                push(&mut new_segs, text, false);
            }
            ChangeTag::Delete => push(&mut old_segs, text, true),
            ChangeTag::Insert => push(&mut new_segs, text, true),
        }
    }

    // A line rewritten end to end reads better with no highlight at all than
    // with every word lit up.
    if changed_fraction(&old_segs) > 0.8 && changed_fraction(&new_segs) > 0.8 {
        return (
            vec![Segment::plain(old)],
            vec![Segment::plain(new)],
        );
    }
    (old_segs, new_segs)
}

fn push(segs: &mut Vec<Segment>, text: &str, changed: bool) {
    match segs.last_mut() {
        Some(last) if last.changed == changed => last.text.push_str(text),
        _ => segs.push(Segment {
            text: text.to_owned(),
            changed,
        }),
    }
}

fn changed_fraction(segs: &[Segment]) -> f32 {
    let total: usize = segs.iter().map(|s| s.text.trim().len()).sum();
    if total == 0 {
        return 0.0;
    }
    let changed: usize = segs
        .iter()
        .filter(|s| s.changed)
        .map(|s| s.text.trim().len())
        .sum();
    changed as f32 / total as f32
}

/// Starting line numbers from `@@ -old,count +new,count @@`.
fn parse_header(line: &str) -> Option<(u32, u32)> {
    let body = line.strip_prefix("@@ ")?.split(" @@").next()?;
    let mut parts = body.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;

    let start = |span: &str| -> Option<u32> {
        span.split(',').next()?.parse().ok()
    };
    Some((start(old)?, start(new)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUNK: &str = "\
@@ -41,4 +41,5 @@
 Intermediate/
-Saved/
+Saved/output

+Binaries/
";

    #[test]
    fn numbers_both_sides_from_the_hunk_header() {
        let rows = rows(HUNK);
        let context = rows.iter().find(|r| r.kind == RowKind::Context).unwrap();
        assert_eq!(context.old_no, Some(41));
        assert_eq!(context.new_no, Some(41));

        let del = rows.iter().find(|r| r.kind == RowKind::Delete).unwrap();
        assert_eq!(del.old_no, Some(42));
        assert_eq!(del.new_no, None, "a deleted line has no new number");

        let add = rows.iter().find(|r| r.kind == RowKind::Add).unwrap();
        assert_eq!(add.new_no, Some(42));
        assert_eq!(add.old_no, None);
    }

    #[test]
    fn a_pure_insertion_is_numbered_on_the_new_side_only() {
        let rows = rows(HUNK);
        let last = rows.iter().rfind(|r| r.kind == RowKind::Add).unwrap();
        assert_eq!(last.text(), "Binaries/");
        assert_eq!(last.new_no, Some(44));
    }

    #[test]
    fn a_rewritten_line_highlights_only_what_changed() {
        let rows = rows("@@ -1,1 +1,1 @@\n-let x = compute(a, b);\n+let x = compute(a, c);\n");
        let del = rows.iter().find(|r| r.kind == RowKind::Delete).unwrap();
        let add = rows.iter().find(|r| r.kind == RowKind::Add).unwrap();

        let changed = |r: &Row| -> String {
            r.segments
                .iter()
                .filter(|s| s.changed)
                .map(|s| s.text.as_str())
                .collect()
        };
        assert_eq!(changed(del), "b");
        assert_eq!(changed(add), "c");
        // The text itself must survive the split intact.
        assert_eq!(del.text(), "let x = compute(a, b);");
    }

    #[test]
    fn an_unrelated_replacement_is_not_highlighted_word_by_word() {
        let rows = rows("@@ -1,1 +1,1 @@\n-alpha beta gamma\n+zeta eta theta\n");
        for r in &rows {
            assert!(
                r.segments.iter().all(|s| !s.changed),
                "a wholly different line should not light up every word"
            );
        }
    }

    #[test]
    fn unequal_runs_are_left_alone() {
        // One line became two; there is no pairing to highlight.
        let rows = rows("@@ -1,1 +1,2 @@\n-one\n+one\n+two\n");
        assert!(rows.iter().all(|r| r.segments.iter().all(|s| !s.changed)));
    }

    #[test]
    fn a_bare_empty_line_counts_as_context() {
        // Perforce writes empty context lines with no leading space.
        let rows = rows("@@ -1,3 +1,3 @@\n a\n\n b\n");
        let contexts: Vec<&Row> = rows.iter().filter(|r| r.kind == RowKind::Context).collect();
        assert_eq!(contexts.len(), 3);
        assert_eq!(contexts[2].old_no, Some(3));
    }

    #[test]
    fn a_no_newline_note_does_not_consume_a_line_number() {
        let rows = rows("@@ -1,1 +1,1 @@\n-a\n\\ No newline at end of file\n+b\n");
        let note = rows.iter().find(|r| r.kind == RowKind::Note).unwrap();
        assert_eq!(note.old_no, None);
        assert_eq!(note.new_no, None);
        let add = rows.iter().find(|r| r.kind == RowKind::Add).unwrap();
        assert_eq!(add.new_no, Some(1));
    }

    #[test]
    fn several_hunks_restart_the_numbering() {
        let rows = rows("@@ -1,1 +1,1 @@\n a\n@@ -80,1 +90,1 @@\n b\n");
        let contexts: Vec<&Row> = rows.iter().filter(|r| r.kind == RowKind::Context).collect();
        assert_eq!(contexts[0].old_no, Some(1));
        assert_eq!(contexts[1].old_no, Some(80));
        assert_eq!(contexts[1].new_no, Some(90));
    }

    #[test]
    fn empty_input_yields_no_rows() {
        assert!(rows("").is_empty());
    }
}
