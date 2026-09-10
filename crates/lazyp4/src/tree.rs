//! Arranging a flat list of depot paths as a directory tree.
//!
//! Depot paths are long and share a deep prefix, so a flat list pushes the part
//! that distinguishes two files off the right edge. The tree shows each
//! directory once, on its own row, and indents its contents one level beneath
//! it.

use std::collections::BTreeMap;

/// A file to place in the tree, and where the caller can find it again.
pub struct Entry<'a> {
    pub index: usize,
    /// Path relative to the tree root, `/` separated.
    pub path: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Dir {
        /// Path from the root, without a trailing slash. Identifies the row
        /// for collapsing, and prefixes the files beneath it.
        path: String,
        /// How many files sit under it, at any depth.
        files: usize,
        collapsed: bool,
    },
    File {
        /// Index into whatever list the caller passed in.
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    /// What to draw: a directory name, possibly several joined by `/`, or a
    /// file name.
    pub label: String,
    pub node: Node,
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    /// (index, file name), kept sorted by name.
    files: Vec<(usize, String)>,
}

impl Dir {
    fn insert(&mut self, index: usize, path: &str) {
        match path.split_once('/') {
            Some((head, rest)) if !rest.is_empty() => {
                self.dirs.entry(head.to_owned()).or_default().insert(index, rest);
            }
            // A trailing slash, or no slash at all: this is the file itself.
            _ => self
                .files
                .push((index, path.trim_end_matches('/').to_owned())),
        }
    }

    fn count(&self) -> usize {
        self.files.len() + self.dirs.values().map(Dir::count).sum::<usize>()
    }
}

/// Build the visible rows. `collapsed` answers whether a directory path's
/// contents are hidden.
///
/// A predicate rather than a set, because the caller may draw several trees
/// that share directory names without sharing their fold state.
pub fn build(entries: &[Entry<'_>], collapsed: &dyn Fn(&str) -> bool) -> Vec<Row> {
    let mut root = Dir::default();
    for entry in entries {
        root.insert(entry.index, entry.path);
    }

    let mut rows = Vec::new();
    flatten(&root, "", 0, collapsed, &mut rows);
    rows
}

fn flatten(
    dir: &Dir,
    prefix: &str,
    depth: usize,
    collapsed: &dyn Fn(&str) -> bool,
    out: &mut Vec<Row>,
) {
    for (name, child) in &dir.dirs {
        let path = format!("{prefix}{name}");
        let is_collapsed = collapsed(&path);
        out.push(Row {
            depth,
            label: format!("{name}/"),
            node: Node::Dir {
                path: path.clone(),
                files: child.count(),
                collapsed: is_collapsed,
            },
        });
        if !is_collapsed {
            flatten(child, &format!("{path}/"), depth + 1, collapsed, out);
        }
    }

    let mut files = dir.files.clone();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    for (index, name) in files {
        out.push(Row {
            depth,
            label: name,
            node: Node::File { index },
        });
    }
}

/// Strip `root` from a depot path. A path outside the root keeps its full form,
/// so nothing is silently hidden.
pub fn relative<'a>(depot_path: &'a str, root: &str) -> &'a str {
    if root.is_empty() {
        return depot_path.trim_start_matches('/');
    }
    depot_path
        .strip_prefix(root)
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(depot_path)
}

/// The deepest directory every path shares, used when the workspace is not a
/// stream and there is no root to take from the server.
pub fn common_root<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let mut shared: Option<Vec<&str>> = None;
    for path in paths {
        // The last component is a file name, never part of the shared root.
        let parts: Vec<&str> = path.split('/').collect();
        let dirs = &parts[..parts.len().saturating_sub(1)];
        shared = Some(match shared {
            None => dirs.to_vec(),
            Some(current) => current
                .iter()
                .zip(dirs)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    shared.map(|p| p.join("/")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(paths: &[&str], collapsed: &[&str]) -> Vec<Row> {
        let entries: Vec<Entry> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| Entry { index, path })
            .collect();
        build(&entries, &|path: &str| collapsed.contains(&path))
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.label))
            .collect()
    }

    #[test]
    fn nests_files_under_their_directories() {
        let out = rows(&["Source/App.cpp", "Source/App.h", "README.md"], &[]);
        assert_eq!(
            labels(&out),
            ["Source/", "  App.cpp", "  App.h", "README.md"],
            "directories come before files, each sorted"
        );
    }

    #[test]
    fn every_directory_gets_its_own_row() {
        let out = rows(&["Source/Core/Actors/Door.cpp"], &[]);
        assert_eq!(
            labels(&out),
            [
                "Source/",
                "  Core/",
                "    Actors/",
                "      Door.cpp"
            ],
            "a chain of single-child directories is not collapsed into one row"
        );
    }

    #[test]
    fn siblings_sit_at_the_same_depth() {
        let out = rows(&["Source/Core/A.cpp", "Source/Editor/B.cpp"], &[]);
        assert_eq!(
            labels(&out),
            ["Source/", "  Core/", "    A.cpp", "  Editor/", "    B.cpp"]
        );
    }

    #[test]
    fn a_directory_counts_every_file_beneath_it() {
        let out = rows(&["a/b/one.txt", "a/b/two.txt", "a/three.txt"], &[]);
        let Node::Dir { files, .. } = out[0].node else {
            panic!("expected a directory first");
        };
        assert_eq!(files, 3);
    }

    #[test]
    fn a_collapsed_directory_hides_its_contents() {
        let out = rows(&["Source/Core/A.cpp", "Source/Editor/B.cpp"], &["Source"]);
        assert_eq!(labels(&out), ["Source/"]);
        let Node::Dir { collapsed, files, .. } = &out[0].node else {
            panic!("expected a directory");
        };
        assert!(collapsed);
        assert_eq!(*files, 2, "the count still tells you what is inside");
    }

    #[test]
    fn a_directory_deep_in_the_tree_can_be_collapsed_on_its_own() {
        let out = rows(
            &["Source/Core/Actors/Door.cpp"],
            &["Source/Core/Actors"],
        );
        assert_eq!(
            labels(&out),
            ["Source/", "  Core/", "    Actors/"],
            "its ancestors stay open"
        );
    }

    #[test]
    fn file_indexes_survive_the_rearrangement() {
        let out = rows(&["z/last.txt", "a/first.txt"], &[]);
        let found: Vec<usize> = out
            .iter()
            .filter_map(|r| match r.node {
                Node::File { index } => Some(index),
                _ => None,
            })
            .collect();
        // Sorted into a/ then z/, but each keeps the caller's index.
        assert_eq!(found, [1, 0]);
    }

    #[test]
    fn strips_the_root_from_a_depot_path() {
        assert_eq!(
            relative("//depot/main/Source/App.cpp", "//depot/main"),
            "Source/App.cpp"
        );
    }

    #[test]
    fn a_path_outside_the_root_is_left_whole() {
        // Better a long row than a file that silently vanishes.
        assert_eq!(
            relative("//other/depot/x.txt", "//depot/main"),
            "//other/depot/x.txt"
        );
    }

    #[test]
    fn finds_the_deepest_shared_directory() {
        let paths = [
            "//depot/main/Source/A.cpp",
            "//depot/main/Source/B.cpp",
            "//depot/main/Config/C.ini",
        ];
        assert_eq!(common_root(paths.into_iter()), "//depot/main");
    }

    #[test]
    fn a_single_file_roots_at_its_own_directory() {
        assert_eq!(
            common_root(["//depot/main/A.cpp"].into_iter()),
            "//depot/main"
        );
    }

    #[test]
    fn no_files_means_no_root() {
        assert_eq!(common_root([].into_iter()), "");
    }
}
