//! Arranging a flat list of depot paths as a directory tree.
//!
//! Depot paths are long and share a deep prefix, so a flat list pushes the part
//! that distinguishes two files off the right edge. The tree shows each
//! directory once and indents beneath it.
//!
//! Chains of directories with a single child are folded into one row —
//! `Source/Darksim/Actors/` rather than three rows of one entry each — which is
//! what makes a deep tree readable.

use std::collections::{BTreeMap, HashSet};

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

/// Build the visible rows. `collapsed` holds directory paths whose contents are
/// hidden.
pub fn build(entries: &[Entry<'_>], collapsed: &HashSet<String>) -> Vec<Row> {
    let mut root = Dir::default();
    for entry in entries {
        root.insert(entry.index, entry.path);
    }

    let mut rows = Vec::new();
    flatten(&root, "", 0, collapsed, &mut rows);
    rows
}

fn flatten(dir: &Dir, prefix: &str, depth: usize, collapsed: &HashSet<String>, out: &mut Vec<Row>) {
    for (name, child) in &dir.dirs {
        // Fold a run of single-child directories into one row.
        let mut label = name.clone();
        let mut node = child;
        while node.files.is_empty() && node.dirs.len() == 1 {
            let (only_name, only) = node.dirs.iter().next().expect("one child");
            label.push('/');
            label.push_str(only_name);
            node = only;
        }

        let path = format!("{prefix}{label}");
        let is_collapsed = collapsed.contains(&path);
        out.push(Row {
            depth,
            label: format!("{label}/"),
            node: Node::Dir {
                path: path.clone(),
                files: node.count(),
                collapsed: is_collapsed,
            },
        });
        if !is_collapsed {
            flatten(node, &format!("{path}/"), depth + 1, collapsed, out);
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
        let set: HashSet<String> = collapsed.iter().map(|s| (*s).to_owned()).collect();
        build(&entries, &set)
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
    fn folds_a_chain_of_single_child_directories() {
        let out = rows(&["Source/Darksim/Actors/Door.cpp"], &[]);
        assert_eq!(labels(&out), ["Source/Darksim/Actors/", "  Door.cpp"]);
    }

    #[test]
    fn stops_folding_where_the_tree_branches() {
        let out = rows(&["Source/Darksim/A.cpp", "Source/Editor/B.cpp"], &[]);
        assert_eq!(
            labels(&out),
            ["Source/", "  Darksim/", "    A.cpp", "  Editor/", "    B.cpp"]
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
        let out = rows(&["Source/Darksim/A.cpp", "Source/Editor/B.cpp"], &["Source"]);
        assert_eq!(labels(&out), ["Source/"]);
        let Node::Dir { collapsed, files, .. } = &out[0].node else {
            panic!("expected a directory");
        };
        assert!(collapsed);
        assert_eq!(*files, 2, "the count still tells you what is inside");
    }

    #[test]
    fn collapsing_uses_the_folded_path() {
        // The row reads `Source/Darksim/Actors/`, so that is its identity.
        let out = rows(
            &["Source/Darksim/Actors/Door.cpp"],
            &["Source/Darksim/Actors"],
        );
        assert_eq!(labels(&out), ["Source/Darksim/Actors/"]);
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
            relative("//darksim/main/Source/App.cpp", "//darksim/main"),
            "Source/App.cpp"
        );
    }

    #[test]
    fn a_path_outside_the_root_is_left_whole() {
        // Better a long row than a file that silently vanishes.
        assert_eq!(
            relative("//other/depot/x.txt", "//darksim/main"),
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
