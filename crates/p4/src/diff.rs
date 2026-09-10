//! Turning Perforce diff output into ordinary unified diffs.
//!
//! Perforce never emits a complete unified diff. Two shapes come back:
//!
//! `p4 diff -du` sends a real `---`/`+++` pair per file, but the depot path on
//! one side and a local filesystem path on the other:
//!
//! ```text
//! --- //depot/main/Build.cs\t2026-03-24 19:30:23.000000000 0000
//! +++ E:\ws\Source\Build.cs\t2026-03-24 19:30:23.000000000 0000
//! @@ -1,5 +1,6 @@
//! ```
//!
//! `p4 describe -du` sends no file header at all, just a banner:
//!
//! ```text
//! ==== //depot/main/.p4ignore#7 (text) ====
//!
//! @@ -41,6 +41,10 @@
//! ```
//!
//! [`normalize`] reads either and produces one [`FileDiff`] per file;
//! [`to_unified`] renders them as a patch any diff viewer can read.

/// One file's diff, with the depot path as its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub depot_path: String,
    pub rev: Option<u32>,
    /// The `@@` hunks, verbatim. Empty when the file has no textual diff —
    /// an add, a delete, or a binary file.
    pub hunks: String,
}

impl FileDiff {
    /// Depot path without the leading `//`, for use as a diff path.
    fn short(&self) -> &str {
        self.depot_path.trim_start_matches('/')
    }
}

/// Split raw Perforce diff output into per-file diffs.
///
/// Unrecognised leading matter — the `describe` header, `Affected files ...`,
/// `Differences ...` — is skipped.
pub fn normalize(raw: &str) -> Vec<FileDiff> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut files: Vec<FileDiff> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if let Some((path, rev)) = parse_banner(line) {
            files.push(FileDiff {
                depot_path: path,
                rev,
                hunks: String::new(),
            });
            i += 1;
            continue;
        }

        // A `---` header only counts when `+++` follows it; inside a hunk the
        // same text is just a deleted line, which is why hunks are consumed
        // whole below rather than scanned line by line.
        if let Some(rest) = line.strip_prefix("--- ") {
            if lines.get(i + 1).is_some_and(|n| n.starts_with("+++ ")) {
                let (path, rev) = split_rev(rest.split('\t').next().unwrap_or(rest).trim());
                files.push(FileDiff {
                    depot_path: path,
                    rev,
                    hunks: String::new(),
                });
                i += 2;
                continue;
            }
        }

        if line.starts_with("@@ ") {
            let Some(current) = files.last_mut() else {
                i += 1;
                continue;
            };
            let end = hunk_end(&lines, i);
            for l in &lines[i..end] {
                current.hunks.push_str(l);
                current.hunks.push('\n');
            }
            i = end;
            continue;
        }

        i += 1;
    }

    files
}

/// Render file diffs as one unified patch.
pub fn to_unified(files: &[FileDiff]) -> String {
    let mut out = String::new();
    for file in files {
        if file.hunks.is_empty() {
            continue;
        }
        let path = file.short();
        out.push_str(&format!("diff --git a/{path} b/{path}\n"));
        out.push_str(&format!("--- a/{path}\n"));
        out.push_str(&format!("+++ b/{path}\n"));
        out.push_str(&file.hunks);
    }
    out
}

/// Build a patch for a file whose whole content is added or removed.
///
/// Perforce reports no diff for an add or a delete, so the content has to come
/// from elsewhere — `p4 print`, or the workspace file — and be turned into a
/// one-sided hunk here.
pub fn whole_file(depot_path: &str, content: &str, added: bool) -> String {
    let path = depot_path.trim_start_matches('/');
    let lines: Vec<&str> = if content.is_empty() {
        Vec::new()
    } else {
        content.lines().collect()
    };
    let n = lines.len();

    let mut out = format!("diff --git a/{path} b/{path}\n");
    if added {
        out.push_str("new file mode 100644\n");
        out.push_str(&format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{n} @@\n"));
    } else {
        out.push_str("deleted file mode 100644\n");
        out.push_str(&format!("--- a/{path}\n+++ /dev/null\n@@ -1,{n} +0,0 @@\n"));
    }

    let marker = if added { '+' } else { '-' };
    for line in lines {
        out.push(marker);
        out.push_str(line);
        out.push('\n');
    }
    if !content.is_empty() && !content.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
    out
}

/// `==== //depot/path#7 (text) ====` and its variants.
fn parse_banner(line: &str) -> Option<(String, Option<u32>)> {
    let inner = line.strip_prefix("==== ")?.strip_suffix(" ====")?;
    let first = inner.split_whitespace().next()?;
    if !first.starts_with("//") {
        return None;
    }
    Some(split_rev(first))
}

fn split_rev(spec: &str) -> (String, Option<u32>) {
    match spec.rsplit_once('#') {
        Some((path, rev)) => (path.to_owned(), rev.parse().ok()),
        None => (spec.to_owned(), None),
    }
}

/// Index one past the last line of the hunk starting at `start`.
///
/// Driven by the counts in the `@@` header rather than by looking for the next
/// marker, because a deleted line can begin with `---` or `@@` and would
/// otherwise end the hunk early.
fn hunk_end(lines: &[&str], start: usize) -> usize {
    let (mut old, mut new) = match parse_hunk_header(lines[start]) {
        Some(counts) => counts,
        None => return start + 1,
    };

    let mut i = start + 1;
    while i < lines.len() && (old > 0 || new > 0) {
        match lines[i].chars().next() {
            // "\ No newline at end of file" annotates the line before it.
            Some('\\') => {}
            Some('-') => old = old.saturating_sub(1),
            Some('+') => new = new.saturating_sub(1),
            // A context line, including the empty line Perforce writes bare.
            _ => {
                old = old.saturating_sub(1);
                new = new.saturating_sub(1);
            }
        }
        i += 1;
    }
    i
}

/// Line counts from `@@ -old,count +new,count @@`; a missing count means 1.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let body = line.strip_prefix("@@ ")?;
    let body = body.split(" @@").next()?;
    let mut parts = body.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;

    let count = |span: &str| -> Option<usize> {
        match span.split_once(',') {
            Some((_, c)) => c.parse().ok(),
            None => Some(1),
        }
    };
    Some((count(old)?, count(new)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured from `describe -du 396` against a live server.
    const DESCRIBE: &str = "\
Change 396 by linsko@linsko_linus-desktop_9415 on 2026/09/08 19:28:53

\t# Updated .p4ignore

Affected files ...


Differences ...

==== //depot/main/.p4ignore#7 (text) ====

@@ -41,6 +41,10 @@
 Intermediate/
 Saved/

+# Ignore build output
+Binaries/
+*.DotSettings.user
+

 # Ignore UBT's configuration.xml
 Engine/Programs/UnrealBuildTool/*
";

    /// Captured from `diff -du <file>`, info and text already interleaved.
    const WORKSPACE: &str = "\
--- //depot/main/Source/Core/Core.Build.cs\t2026-03-24 19:30:23.000000000 0000
+++ E:\\ws\\Source\\Core\\Core.Build.cs\t2026-03-24 19:30:23.000000000 0000
@@ -1,5 +1,6 @@
 // Fill out your copyright notice.

+using System.IO;
 using UnrealBuildTool;

";

    #[test]
    fn reads_a_describe_banner() {
        let files = normalize(DESCRIBE);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].depot_path, "//depot/main/.p4ignore");
        assert_eq!(files[0].rev, Some(7));
        assert!(files[0].hunks.starts_with("@@ -41,6 +41,10 @@"));
        // The describe preamble must not leak into the hunks.
        assert!(!files[0].hunks.contains("Differences"));
    }

    #[test]
    fn reads_a_workspace_header_and_keeps_the_depot_path() {
        let files = normalize(WORKSPACE);
        assert_eq!(files.len(), 1);
        // The `+++` side is a local path; the depot path is the identity.
        assert_eq!(
            files[0].depot_path,
            "//depot/main/Source/Core/Core.Build.cs"
        );
        assert!(files[0].hunks.contains("+using System.IO;"));
    }

    #[test]
    fn separates_several_files() {
        let raw = format!("{DESCRIBE}\n==== //depot/other.cpp#2 (text) ====\n\n@@ -1 +1 @@\n-a\n+b\n");
        let files = normalize(&raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].depot_path, "//depot/other.cpp");
        assert_eq!(files[1].hunks, "@@ -1 +1 @@\n-a\n+b\n");
    }

    #[test]
    fn a_deleted_line_that_looks_like_a_header_stays_in_the_hunk() {
        // Markdown rules and diff fragments are ordinary content.
        let raw = "\
==== //depot/README.md#3 (text) ====

@@ -1,4 +1,4 @@
 title
----
-@@ fake
+--- new
+@@ also fake
";
        let files = normalize(raw);
        assert_eq!(files.len(), 1, "must not split on content that looks like a header");
        assert!(files[0].hunks.contains("-@@ fake"));
        assert!(files[0].hunks.contains("+@@ also fake"));
    }

    #[test]
    fn a_hunk_header_without_counts_means_one_line() {
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
        assert_eq!(parse_hunk_header("@@ -41,6 +41,10 @@"), Some((6, 10)));
        assert_eq!(parse_hunk_header("not a hunk"), None);
    }

    #[test]
    fn renders_a_patch_a_viewer_can_read() {
        let patch = to_unified(&normalize(DESCRIBE));
        assert!(patch.starts_with(
            "diff --git a/depot/main/.p4ignore b/depot/main/.p4ignore\n\
             --- a/depot/main/.p4ignore\n\
             +++ b/depot/main/.p4ignore\n\
             @@ -41,6 +41,10 @@"
        ), "{patch}");
    }

    #[test]
    fn files_without_hunks_are_left_out() {
        let files = vec![FileDiff {
            depot_path: "//depot/added.txt".into(),
            rev: Some(1),
            hunks: String::new(),
        }];
        assert_eq!(to_unified(&files), "");
    }

    #[test]
    fn an_added_file_becomes_a_one_sided_hunk() {
        let patch = whole_file("//depot/new.txt", "one\ntwo\n", true);
        assert!(patch.contains("--- /dev/null"));
        assert!(patch.contains("+++ b/depot/new.txt"));
        assert!(patch.contains("@@ -0,0 +1,2 @@"));
        assert!(patch.contains("+one\n+two\n"));
    }

    #[test]
    fn a_deleted_file_becomes_a_one_sided_hunk() {
        let patch = whole_file("//depot/gone.txt", "only\n", false);
        assert!(patch.contains("+++ /dev/null"));
        assert!(patch.contains("@@ -1,1 +0,0 @@"));
        assert!(patch.contains("-only\n"));
    }

    #[test]
    fn an_empty_added_file_has_an_empty_hunk() {
        let patch = whole_file("//depot/empty.txt", "", true);
        assert!(patch.contains("@@ -0,0 +1,0 @@"));
        assert!(!patch.contains("\\ No newline"));
    }

    #[test]
    fn a_missing_trailing_newline_is_marked() {
        let patch = whole_file("//depot/x.txt", "no newline", true);
        assert!(patch.ends_with("\\ No newline at end of file\n"));
    }

    #[test]
    fn output_with_no_diffs_yields_nothing() {
        assert!(normalize("Change 395 by a@b on 2026/01/01 *pending*\n\nAffected files ...\n").is_empty());
    }
}
