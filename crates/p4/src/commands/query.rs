//! Commands that only ask: server and workspace state, file history, and
//! where a depot path lands on disk. None of them changes anything.

use crate::client::Client;
use crate::commands::ChangeFilter;
use crate::error::{Error, Result};
use crate::model::*;
use crate::record::RecordExt;

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

    /// The streams under `in_depot`, e.g. `//depot/...`, or every stream on
    /// the server when it is `None`.
    pub fn streams(&mut self, in_depot: Option<&str>) -> Result<Vec<Stream>> {
        let args: Vec<&str> = in_depot.into_iter().collect();
        let out = self.run("streams", &args)?;
        Ok(out
            .records
            .iter()
            .filter_map(|rec| {
                Some(Stream {
                    path: rec.field("Stream")?.to_owned(),
                    name: rec.field("Name").unwrap_or_default().to_owned(),
                    parent: rec.field("Parent").unwrap_or_default().to_owned(),
                    kind: rec.field("Type").unwrap_or_default().to_owned(),
                    owner: rec.field("Owner").unwrap_or_default().to_owned(),
                })
            })
            .collect())
    }

    /// The workspaces on the server. `owner` narrows to one user's.
    ///
    /// Sorted by last use, newest first, so the ones worth switching to are at
    /// the top of a list that can run to thousands on a shared server.
    pub fn clients(&mut self, owner: Option<&str>) -> Result<Vec<Workspace>> {
        let mut args: Vec<&str> = Vec::new();
        if let Some(u) = owner {
            args.extend(["-u", u]);
        }
        let out = self.run("clients", &args)?;
        let mut workspaces: Vec<Workspace> = out
            .records
            .iter()
            .filter_map(|rec| {
                Some(Workspace {
                    name: rec.field("client")?.to_owned(),
                    owner: rec.field("Owner").unwrap_or_default().to_owned(),
                    root: rec.field("Root").unwrap_or_default().to_owned(),
                    host: rec.field("Host").unwrap_or_default().to_owned(),
                    stream: rec.field("Stream").map(str::to_owned),
                    description: rec.field("Description").unwrap_or_default().trim().to_owned(),
                    accessed: rec.parsed("Access"),
                })
            })
            .collect();
        workspaces.sort_by_key(|w| std::cmp::Reverse(w.accessed));
        Ok(workspaces)
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

    /// Who last wrote each line of a file — the blame equivalent.
    ///
    /// Only submitted content can be annotated: a file opened for add exists
    /// nowhere the server can look, and it says so.
    pub fn annotate(&mut self, depot_path: &str) -> Result<Vec<AnnotatedLine>> {
        // -c reports the change that wrote each line rather than the file
        // revision, which is the number worth showing; -u adds who and when;
        // -q drops the banner line.
        let out = self.run("annotate", &["-c", "-u", "-q", depot_path])?;
        Ok(out
            .records
            .iter()
            // The leading record names the file and has no line on it.
            .filter_map(|rec| {
                Some(AnnotatedLine {
                    change: rec.parsed("lower")?,
                    user: rec.field("user").unwrap_or_default().to_owned(),
                    time: rec.parsed("time"),
                    text: rec.field("data").unwrap_or_default().to_owned(),
                })
            })
            .collect())
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
