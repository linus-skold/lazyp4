//! Commands that act on the files in the workspace: opening them, moving them
//! between changelists, throwing the changes away, and bringing the workspace
//! up to date.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::*;
use crate::record::RecordExt;

impl Client {
    /// Open files for `add`, `edit` or `delete` directly into a changelist.
    ///
    /// This is what brings a file `p4 status` found under Perforce's control.
    pub fn open_files(
        &mut self,
        action: &FileAction,
        change: ChangeId,
        paths: &[&str],
    ) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let cmd = match action {
            FileAction::Add => "add",
            FileAction::Edit => "edit",
            FileAction::Delete => "delete",
            other => {
                return Err(Error::parse(format!("cannot open a file for {other}")));
            }
        };
        let id = change.to_string();
        let mut args = vec!["-c", &id];
        args.extend_from_slice(paths);
        self.run(cmd, &args)?;
        Ok(())
    }

    /// Move already-open files into another changelist.
    ///
    /// `ChangeId::Default` moves them back out of a numbered changelist.
    pub fn reopen(&mut self, change: ChangeId, paths: &[&str]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let id = change.to_string();
        let mut args = vec!["-c", &id];
        args.extend_from_slice(paths);
        self.run("reopen", &args)?;
        Ok(())
    }

    /// What `p4 revert` would actually do, without doing it.
    ///
    /// The server's own account, which is stronger than the file list the
    /// caller already holds: it drops anything that is not open here and names
    /// the action each file would be closed from.
    pub fn revert_preview(&mut self, paths: &[&str]) -> Result<Vec<RevertPreview>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["-n"];
        args.extend_from_slice(paths);
        // A file with nothing to revert is a warning, not a failure.
        let out = self.run_raw("revert", &args, "")?;
        Ok(out
            .records
            .iter()
            .filter_map(|rec| {
                Some(RevertPreview {
                    depot_path: rec.field("depotFile")?.to_owned(),
                    action: rec.field("action").unwrap_or("edit").parse().unwrap(),
                })
            })
            .collect())
    }

    /// Throw away the local changes to open files and close them.
    ///
    /// Irreversible: the workspace copy of an edited file is overwritten with
    /// the depot's, and a file opened for add is left on disk but untracked.
    /// No changelist is named because a file can only be open once per
    /// workspace, so its path is unambiguous.
    pub fn revert(&mut self, paths: &[&str]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.run("revert", paths)?;
        Ok(())
    }

    /// Bring the workspace up to date, returning how many files changed.
    ///
    /// Files open for edit are left alone: Perforce refuses to overwrite them
    /// rather than discarding work.
    pub fn sync(&mut self) -> Result<usize> {
        let out = self.run_raw("sync", &[], "")?;
        match out.errors().cloned().collect::<Vec<_>>() {
            // "File(s) up-to-date." arrives as a warning, and is not one.
            msgs if msgs.iter().all(|m| m.text.contains("up-to-date")) => Ok(out.records.len()),
            msgs => Err(Error::Server(msgs)),
        }
    }

    /// Point the workspace at another stream and resync it.
    ///
    /// Refused by the server while files are open, which is what stops this
    /// from stranding work.
    pub fn switch_stream(&mut self, stream: &str) -> Result<()> {
        self.run("switch", &[stream])?;
        Ok(())
    }
}
