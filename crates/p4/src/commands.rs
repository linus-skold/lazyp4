//! Typed wrappers over the commands lazyp4 runs.
//!
//! Each one builds argv, runs it tagged, and maps the records onto
//! [`crate::model`] types.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::*;
use crate::record::RecordExt;

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
