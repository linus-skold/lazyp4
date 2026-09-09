//! Types for the Perforce concepts lazyp4 shows.

use std::fmt;
use std::str::FromStr;

/// Which changelist. The default changelist has no number but is otherwise an
/// ordinary changelist, so it has to be part of the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeId {
    Default,
    Number(u32),
}

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeId::Default => f.write_str("default"),
            ChangeId::Number(n) => write!(f, "{n}"),
        }
    }
}

impl FromStr for ChangeId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "default" || s == "new" {
            return Ok(ChangeId::Default);
        }
        s.parse().map(ChangeId::Number)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    New,
    Pending,
    Submitted,
    Shelved,
}

impl FromStr for ChangeStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "new" => ChangeStatus::New,
            "pending" => ChangeStatus::Pending,
            "submitted" => ChangeStatus::Submitted,
            "shelved" => ChangeStatus::Shelved,
            _ => return Err(()),
        })
    }
}

/// What a file is opened for, or what a submitted revision did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    Add,
    Edit,
    Delete,
    Branch,
    MoveAdd,
    MoveDelete,
    Integrate,
    Import,
    Purge,
    Archive,
    Other(String),
}

impl FileAction {
    /// The single letter lazyp4 shows in file lists, following `p4` and git
    /// convention.
    pub fn code(&self) -> char {
        match self {
            FileAction::Add | FileAction::MoveAdd | FileAction::Import => 'A',
            FileAction::Edit => 'M',
            FileAction::Delete | FileAction::MoveDelete | FileAction::Purge => 'D',
            FileAction::Branch => 'B',
            FileAction::Integrate => 'I',
            FileAction::Archive => 'R',
            FileAction::Other(_) => '?',
        }
    }
}

impl FromStr for FileAction {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "add" => FileAction::Add,
            "edit" => FileAction::Edit,
            "delete" => FileAction::Delete,
            "branch" => FileAction::Branch,
            "move/add" => FileAction::MoveAdd,
            "move/delete" => FileAction::MoveDelete,
            "integrate" => FileAction::Integrate,
            "import" => FileAction::Import,
            "purge" => FileAction::Purge,
            "archive" => FileAction::Archive,
            other => FileAction::Other(other.to_owned()),
        })
    }
}

impl fmt::Display for FileAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FileAction::Add => "add",
            FileAction::Edit => "edit",
            FileAction::Delete => "delete",
            FileAction::Branch => "branch",
            FileAction::MoveAdd => "move/add",
            FileAction::MoveDelete => "move/delete",
            FileAction::Integrate => "integrate",
            FileAction::Import => "import",
            FileAction::Purge => "purge",
            FileAction::Archive => "archive",
            FileAction::Other(s) => s,
        })
    }
}

/// A changelist as listed by `p4 changes`.
#[derive(Debug, Clone)]
pub struct Changelist {
    pub id: ChangeId,
    pub status: ChangeStatus,
    pub user: String,
    pub client: String,
    /// Seconds since the epoch, as the server reports it.
    pub time: Option<i64>,
    pub description: String,
    /// True when the changelist has shelved files.
    pub shelved: bool,
}

impl Changelist {
    /// First line of the description, for a one-line list entry.
    pub fn summary(&self) -> &str {
        self.description
            .lines()
            .next()
            .unwrap_or_default()
            .trim_end()
    }
}

/// A file open in the workspace, as listed by `p4 opened`.
#[derive(Debug, Clone)]
pub struct OpenedFile {
    pub depot_path: String,
    /// Absent unless the command was asked for client paths.
    pub client_path: Option<String>,
    pub rev: Option<u32>,
    pub action: FileAction,
    pub change: ChangeId,
    pub file_type: String,
    pub user: Option<String>,
    pub client: Option<String>,
    /// True when the server flagged the file as needing a resolve.
    pub unresolved: bool,
}

/// A workspace file that differs from the depot but is not open, as reported
/// by `p4 status`.
///
/// The action says what it would take to reconcile it: `Add` for a file
/// Perforce does not know about, `Edit` for one changed without being checked
/// out, `Delete` for one removed from disk.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub depot_path: String,
    pub local_path: String,
    pub action: FileAction,
}

/// One file inside a changelist, as reported by `p4 describe`.
#[derive(Debug, Clone)]
pub struct DescribedFile {
    pub depot_path: String,
    pub rev: Option<u32>,
    pub action: FileAction,
    pub file_type: Option<String>,
}

/// A changelist together with its files.
#[derive(Debug, Clone)]
pub struct Description {
    pub change: Changelist,
    pub files: Vec<DescribedFile>,
}

#[cfg(test)]
mod tests {
    use super::civil_date;

    #[test]
    fn formats_a_server_timestamp() {
        // 1788895733 is the time on change 396.
        assert_eq!(civil_date(1_788_895_733), "2026-09-08");
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(951_782_400), "2000-02-29", "a leap day");
    }
}

/// One revision of one file, from `p4 filelog`.
#[derive(Debug, Clone)]
pub struct Revision {
    pub rev: u32,
    pub change: u32,
    pub action: FileAction,
    pub user: String,
    pub time: Option<i64>,
    pub file_type: String,
    pub description: String,
}

/// Format a server timestamp as `YYYY-MM-DD`.
///
/// Hinnant's civil-from-days, so no date crate is needed for the one thing
/// lazyp4 shows: which day a revision landed.
pub fn civil_date(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The server and workspace lazyp4 is talking to, from `p4 info`.
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    pub user: String,
    pub client: String,
    pub client_root: Option<String>,
    pub host: String,
    pub server_address: String,
    pub server_version: String,
    pub stream: Option<String>,
    /// False when `clientName` came back as `*unknown*`.
    pub client_known: bool,
}
