//! Handing a patch to `hunk`, the external diff viewer.
//!
//! `hunk` takes over the terminal, so lazyp4 must leave the alternate screen
//! first and restore it afterwards. That has to happen on the main loop, not
//! on the worker thread.

use std::io;
use std::process::Command;

/// Whether `hunk` is on PATH. Checked once, at startup.
pub fn available() -> bool {
    Command::new("hunk")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Show `patch` in `hunk`, blocking until the viewer exits.
///
/// The patch goes through a temp file rather than stdin: `hunk` is an
/// interactive viewer and needs the terminal's stdin for itself.
pub fn show(patch: &str) -> io::Result<()> {
    let path = std::env::temp_dir().join(format!("lazyp4-{}.patch", std::process::id()));
    std::fs::write(&path, patch)?;

    let status = Command::new("hunk")
        .arg("patch")
        .arg(&path)
        .arg("--mode")
        .arg("auto")
        .status();

    let _ = std::fs::remove_file(&path);
    status.map(|_| ())
}
