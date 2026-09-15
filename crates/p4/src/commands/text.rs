//! Commands that return file content as plain text rather than records.
//!
//! The server drops diff content from a tagged reply, so these need an
//! untagged connection — see [`crate::Connection::tagged`].

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::ChangeId;

impl Client {
    /// Unified diff of open workspace files against the depot.
    ///
    /// Needs an untagged connection — see [`Connection::tagged`].
    ///
    /// [`Connection::tagged`]: crate::Connection::tagged
    pub fn diff_text(&mut self, paths: &[&str]) -> Result<String> {
        let mut args = vec!["-du"];
        args.extend_from_slice(paths);
        // An unchanged file is reported as a warning, not a failure.
        Ok(self.run_raw("diff", &args, "")?.merged_text())
    }

    /// Unified diff of a submitted changelist, or of a shelf when `shelved`.
    ///
    /// Needs an untagged connection.
    pub fn describe_diff_text(&mut self, change: ChangeId, shelved: bool) -> Result<String> {
        let id = change.to_string();
        let mut args = vec!["-du"];
        if shelved {
            args.push("-S");
        }
        args.push(&id);
        Ok(self.run_raw("describe", &args, "")?.merged_text())
    }

    /// Contents of one depot revision, e.g. `//depot/f.txt#3` or
    /// `//depot/f.txt@=412` for a shelved revision.
    ///
    /// This is how an add or a delete gets content, since Perforce reports no
    /// diff for either.
    pub fn print_text(&mut self, spec: &str) -> Result<String> {
        // -q drops the `//depot/file#1 - add change 1 (text)` banner.
        let out = self.run_raw("print", &["-q", spec], "")?;
        match out.errors().cloned().collect::<Vec<_>>() {
            msgs if msgs.is_empty() => Ok(String::from_utf8_lossy(&out.text).into_owned()),
            msgs => Err(Error::Server(msgs)),
        }
    }
}
