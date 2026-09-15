//! `P4IGNORE` path arithmetic: which ignore file to write, and what to write
//! in it.

/// Where the ignore file lives, and the pattern that names `local` inside it.
///
/// `P4IGNORE` may be a bare file name, which Perforce looks for from each
/// file's directory upwards, or a path. lazyp4 writes to the one in the
/// workspace root, which is where a shared ignore file belongs. `None` when the
/// file is not inside the workspace at all.
pub fn entry(root: &str, name: &str, local: &str) -> Option<(String, String)> {
    let slashes = |s: &str| s.replace('\\', "/");
    let file = if std::path::Path::new(name).is_absolute() {
        slashes(name)
    } else {
        format!("{}/{name}", slashes(root).trim_end_matches('/'))
    };

    let root = slashes(root);
    let root = root.trim_end_matches('/');
    let local = slashes(local);
    // Windows spells the same path in several cases, so compare loosely.
    let head = local.get(..root.len())?;
    if !head.eq_ignore_ascii_case(root) || local.as_bytes().get(root.len()) != Some(&b'/') {
        return None;
    }
    Some((file, local[root.len() + 1..].to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ignore_pattern_is_the_path_below_the_workspace_root() {
        let (file, pattern) = entry("E:\\ws", ".p4ignore", "E:\\ws\\Content\\Big.uasset").unwrap();
        assert_eq!(file, "E:/ws/.p4ignore");
        assert_eq!(pattern, "Content/Big.uasset");

        // Windows spells the same path in several cases.
        assert_eq!(entry("E:\\WS", ".p4ignore", "e:\\ws\\A.txt").unwrap().1, "A.txt");
        // A trailing separator on the root must not double up.
        assert_eq!(
            entry("E:/ws/", ".p4ignore", "E:/ws/A.txt").unwrap().0,
            "E:/ws/.p4ignore"
        );
        // P4IGNORE may name a path rather than a file. What counts as absolute is
        // the platform's own business — a drive letter means nothing on Unix — so
        // this asks with a path that is absolute wherever the test is running.
        let elsewhere = if cfg!(windows) {
            "D:/shared/ignore.txt"
        } else {
            "/shared/ignore.txt"
        };
        assert_eq!(entry("E:/ws", elsewhere, "E:/ws/A.txt").unwrap().0, elsewhere);
        // Outside the workspace there is no pattern to write.
        assert!(entry("E:/ws", ".p4ignore", "C:/elsewhere/A.txt").is_none());
        assert!(entry("E:/ws", ".p4ignore", "E:/wsx/A.txt").is_none());
    }
}
