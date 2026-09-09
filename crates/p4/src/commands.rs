//! Typed wrappers over the commands lazyp4 runs.
//!
//! Each one builds argv, runs it tagged, and maps the records onto
//! [`crate::model`] types.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::*;
use crate::record::RecordExt;

/// The number out of `Change 398 created.`
fn parse_created(message: &str) -> Option<u32> {
    let rest = message.strip_prefix("Change ")?;
    rest.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_created;

    #[test]
    fn reads_the_number_out_of_the_reply() {
        assert_eq!(parse_created("Change 398 created."), Some(398));
        assert_eq!(parse_created("Change 398 created fixing job000123."), Some(398));
    }

    #[test]
    fn ignores_anything_else_the_server_says() {
        assert_eq!(parse_created("Change 398 updated."), Some(398));
        assert_eq!(parse_created("//depot/f.txt#1 - opened for add"), None);
        assert_eq!(parse_created("Change default renamed"), None);
    }
}

/// Which changelists `changes` should list.
#[derive(Debug, Clone, Default)]
pub struct ChangeFilter {
    pub status: Option<ChangeStatus>,
    pub user: Option<String>,
    pub client: Option<String>,
    /// Stop after this many, newest first.
    pub max: Option<u32>,
    /// Restrict to a depot path, e.g. `//depot/main/...`.
    pub path: Option<String>,
}

impl ChangeFilter {
    pub fn pending() -> Self {
        ChangeFilter {
            status: Some(ChangeStatus::Pending),
            ..Default::default()
        }
    }

    pub fn submitted(max: u32) -> Self {
        ChangeFilter {
            status: Some(ChangeStatus::Submitted),
            max: Some(max),
            ..Default::default()
        }
    }
}

impl Client {
    pub fn info(&mut self) -> Result<ServerInfo> {
        let out = self.run("info", &[])?;
        let rec = out
            .records
            .first()
            .ok_or_else(|| Error::parse("`info` returned no record"))?;

        let client = rec.field("clientName").unwrap_or_default().to_owned();
        Ok(ServerInfo {
            user: rec.field("userName").unwrap_or_default().to_owned(),
            client_known: !client.is_empty() && client != "*unknown*",
            client,
            client_root: rec.field("clientRoot").map(str::to_owned),
            host: rec.field("clientHost").unwrap_or_default().to_owned(),
            server_address: rec.field("serverAddress").unwrap_or_default().to_owned(),
            server_version: rec.field("serverVersion").unwrap_or_default().to_owned(),
            stream: rec.field("clientStream").map(str::to_owned),
        })
    }

    pub fn changes(&mut self, filter: &ChangeFilter) -> Result<Vec<Changelist>> {
        let status;
        let max;
        let mut args: Vec<&str> = vec!["-l"]; // full descriptions, not the truncated form
        if let Some(s) = filter.status {
            status = match s {
                ChangeStatus::New => "pending",
                ChangeStatus::Pending => "pending",
                ChangeStatus::Submitted => "submitted",
                ChangeStatus::Shelved => "shelved",
            };
            args.extend(["-s", status]);
        }
        if let Some(u) = &filter.user {
            args.extend(["-u", u]);
        }
        if let Some(c) = &filter.client {
            args.extend(["-c", c]);
        }
        if let Some(m) = filter.max {
            max = m.to_string();
            args.extend(["-m", &max]);
        }
        if let Some(p) = &filter.path {
            args.push(p);
        }

        let out = self.run("changes", &args)?;
        out.records
            .iter()
            .map(|rec| {
                Ok(Changelist {
                    id: rec.required("change")?.parse().map_err(Error::parse_int)?,
                    status: rec
                        .field("status")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(ChangeStatus::Pending),
                    user: rec.field("user").unwrap_or_default().to_owned(),
                    client: rec.field("client").unwrap_or_default().to_owned(),
                    time: rec.parsed("time"),
                    description: rec.field("desc").unwrap_or_default().to_owned(),
                    // The server sends `shelved` as a valueless flag.
                    shelved: rec.field("shelved").is_some(),
                })
            })
            .collect()
    }

    /// Files open in the workspace. `change` narrows to one changelist.
    pub fn opened(&mut self, change: Option<ChangeId>) -> Result<Vec<OpenedFile>> {
        let id;
        let mut args: Vec<&str> = Vec::new();
        if let Some(c) = change {
            id = c.to_string();
            args.extend(["-c", &id]);
        }

        // Nothing open is a warning from the server, not a failure.
        let out = self.run_raw("opened", &args, "")?;
        out.records
            .iter()
            .map(|rec| {
                Ok(OpenedFile {
                    depot_path: rec.required("depotFile")?.to_owned(),
                    client_path: rec.field("clientFile").map(str::to_owned),
                    rev: rec.parsed("rev"),
                    action: rec.field("action").unwrap_or("edit").parse().unwrap(),
                    change: rec
                        .field("change")
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(ChangeId::Default),
                    file_type: rec.field("type").unwrap_or_default().to_owned(),
                    user: rec.field("user").map(str::to_owned),
                    client: rec.field("client").map(str::to_owned),
                    unresolved: rec.field("unresolved").is_some(),
                })
            })
            .collect()
    }

    /// A changelist and its files. `shelved` describes the shelf rather than
    /// the open files, which is the only way to see a shelved changelist's
    /// contents.
    pub fn describe(&mut self, change: ChangeId, shelved: bool) -> Result<Description> {
        let id = change.to_string();
        // -s suppresses the diffs; lazyp4 fetches those separately.
        let mut args: Vec<&str> = vec!["-s"];
        if shelved {
            args.push("-S");
        }
        args.push(&id);

        let out = self.run("describe", &args)?;
        let rec = out
            .records
            .first()
            .ok_or_else(|| Error::parse("`describe` returned no record"))?;

        let change = Changelist {
            id: rec.required("change")?.parse().map_err(Error::parse_int)?,
            status: rec
                .field("status")
                .and_then(|s| s.parse().ok())
                .unwrap_or(ChangeStatus::Pending),
            user: rec.field("user").unwrap_or_default().to_owned(),
            client: rec.field("client").unwrap_or_default().to_owned(),
            time: rec.parsed("time"),
            description: rec.field("desc").unwrap_or_default().to_owned(),
            shelved,
        };

        // `describe` reports files as depotFile0, depotFile1, … on one record.
        let files = (0..rec.indexed_count("depotFile"))
            .map(|i| DescribedFile {
                depot_path: rec.indexed("depotFile", i).unwrap_or_default().to_owned(),
                rev: rec.indexed("rev", i).and_then(|v| v.parse().ok()),
                action: rec.indexed("action", i).unwrap_or("edit").parse().unwrap(),
                file_type: rec.indexed("type", i).map(str::to_owned),
            })
            .collect();

        Ok(Description { change, files })
    }

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

    /// Workspace files that differ from the depot without being open.
    ///
    /// This walks the whole workspace and is slow — tens of seconds on a large
    /// tree — so call it only when the user asks for it.
    pub fn status(&mut self) -> Result<Vec<StatusEntry>> {
        let out = self.run_raw("status", &[], "")?;
        Ok(out
            .records
            .iter()
            .filter_map(|rec| {
                Some(StatusEntry {
                    depot_path: rec.field("depotFile")?.to_owned(),
                    local_path: rec.field("clientFile")?.to_owned(),
                    action: rec.field("action").unwrap_or("add").parse().unwrap(),
                })
            })
            .collect())
    }

    /// The changelist spec form, as `p4 change -o` prints it.
    ///
    /// Needs an untagged connection: a tagged reply is a record, not the form
    /// that `save_change_spec` must send back.
    pub fn change_spec(&mut self, change: ChangeId) -> Result<String> {
        let id = change.to_string();
        Ok(self.run("change", &["-o", &id])?.merged_text())
    }

    /// Create an empty pending changelist and return its number.
    ///
    /// The form `p4 change -o` hands back for a new changelist already lists
    /// every file open in the default changelist, and saving it as-is would
    /// sweep all of them in. `Files` is cleared so the caller moves exactly
    /// what it means to.
    pub fn create_change(&mut self, description: &str) -> Result<ChangeId> {
        let form = self.run("change", &["-o"])?.merged_text();
        let form = crate::spec::set_field(&form, "Description", description);
        let form = crate::spec::set_field(&form, "Files", "");

        let out = self.run_with_input("change", &["-i"], &form)?;
        // The server answers "Change 398 created."
        out.info
            .iter()
            .find_map(|line| parse_created(&line.text))
            .map(ChangeId::Number)
            .ok_or_else(|| Error::parse("no changelist number in the reply"))
    }

    /// Delete an empty pending changelist.
    pub fn delete_change(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("change", &["-d", &id])?;
        Ok(())
    }

    /// Write a changelist spec form back.
    pub fn save_change_spec(&mut self, form: &str) -> Result<()> {
        self.run_with_input("change", &["-i"], form)?;
        Ok(())
    }

    /// Replace a pending changelist's description.
    ///
    /// Reads the current form and edits one field of it, so nothing else about
    /// the changelist is disturbed.
    pub fn set_description(&mut self, change: ChangeId, description: &str) -> Result<()> {
        let form = self.change_spec(change)?;
        self.save_change_spec(&crate::spec::set_field(&form, "Description", description))
    }

    /// Files that must be resolved before they can be submitted.
    pub fn unresolved(&mut self) -> Result<Vec<Unresolved>> {
        // -n previews; without it `p4 resolve` is interactive and would sit
        // waiting for an answer this client has no way to give.
        let out = self.run_raw("resolve", &["-n"], "")?;
        Ok(out
            .records
            .iter()
            .filter_map(|rec| {
                Some(Unresolved {
                    local_path: rec.field("clientFile")?.to_owned(),
                    from_path: rec.field("fromFile").unwrap_or_default().to_owned(),
                    start_rev: rec.parsed("startFromRev"),
                    end_rev: rec.parsed("endFromRev"),
                    resolve_type: rec.field("resolveType").unwrap_or_default().to_owned(),
                    content_type: rec
                        .field("contentResolveType")
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect())
    }

    /// Settle files one way or the other.
    ///
    /// Always with an `-a` flag, since a bare `p4 resolve` prompts per file.
    pub fn resolve(&mut self, how: Resolution, paths: &[&str]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec![how.flag()];
        args.extend_from_slice(paths);
        self.run("resolve", &args)?;
        Ok(())
    }

    /// Copy open files to the server as a shelf on `change`.
    ///
    /// An empty `paths` shelves everything open in the changelist. `-f` is
    /// always passed so shelving again updates what is already there rather
    /// than failing.
    pub fn shelve(&mut self, change: ChangeId, paths: &[&str]) -> Result<()> {
        let id = change.to_string();
        let mut args = vec!["-c", &id, "-f"];
        args.extend_from_slice(paths);
        self.run("shelve", &args)?;
        Ok(())
    }

    /// Replace a shelf with whatever is open now.
    ///
    /// Unlike [`Client::shelve`], files that were shelved but are no longer
    /// open are dropped from the shelf, so it ends up matching the workspace.
    pub fn replace_shelf(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("shelve", &["-r", "-c", &id])?;
        Ok(())
    }

    /// Discard a shelf. The changelist and its open files are untouched.
    pub fn delete_shelf(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("shelve", &["-d", "-c", &id])?;
        Ok(())
    }

    /// Open a shelf's files in `into`, taking a copy of the shelved content.
    ///
    /// The shelf itself is left alone, which is what makes this the way to
    /// carry work between machines.
    pub fn unshelve(&mut self, from: ChangeId, into: ChangeId) -> Result<()> {
        let source = from.to_string();
        let target = into.to_string();
        self.run("unshelve", &["-s", &source, "-c", &target])?;
        Ok(())
    }

    /// Open files that reverse a submitted change, into `into`.
    ///
    /// `spec` is a path with a revision range, e.g. `//depot/main/...@=412`
    /// for everything one changelist submitted. Nothing reaches the depot
    /// until the resulting changelist is submitted.
    pub fn undo(&mut self, into: ChangeId, spec: &str) -> Result<()> {
        let id = into.to_string();
        self.run("undo", &["-c", &id, spec])?;
        Ok(())
    }

    /// Submit a pending changelist to the depot.
    ///
    /// Irreversible once it succeeds. It fails if any file needs resolving,
    /// and the server's message says which.
    pub fn submit(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("submit", &["-c", &id])?;
        Ok(())
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

    /// Local filesystem path of a depot file, or `None` when it is not mapped
    /// into the workspace. Needs a tagged connection.
    pub fn local_path(&mut self, depot_path: &str) -> Result<Option<String>> {
        let out = self.run_raw("where", &[depot_path], "")?;
        // `where` names the local path `path`; `clientFile` is client syntax.
        Ok(out
            .records
            .first()
            .and_then(|r| r.field("path"))
            .map(str::to_owned))
    }

    /// Revision history of one file, newest first.
    pub fn filelog(&mut self, depot_path: &str, max: Option<u32>) -> Result<Vec<Revision>> {
        let m;
        let mut args: Vec<&str> = vec!["-l"];
        if let Some(v) = max {
            m = v.to_string();
            args.extend(["-m", &m]);
        }
        args.push(depot_path);

        let out = self.run("filelog", &args)?;
        let Some(rec) = out.records.first() else {
            return Ok(Vec::new());
        };

        Ok((0..rec.indexed_count("rev"))
            .filter_map(|i| {
                Some(Revision {
                    rev: rec.indexed("rev", i)?.parse().ok()?,
                    change: rec.indexed("change", i)?.parse().ok()?,
                    action: rec.indexed("action", i).unwrap_or("edit").parse().unwrap(),
                    user: rec.indexed("user", i).unwrap_or_default().to_owned(),
                    time: rec.indexed("time", i).and_then(|v| v.parse().ok()),
                    file_type: rec.indexed("type", i).unwrap_or_default().to_owned(),
                    description: rec.indexed("desc", i).unwrap_or_default().to_owned(),
                })
            })
            .collect())
    }
}
