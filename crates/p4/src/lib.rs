//! A typed Rust API over the Helix Core C++ API.
//!
//! ```no_run
//! use p4::{Client, Connection, ChangeFilter};
//!
//! let mut p4 = Client::connect(&Connection::default())?;
//! for cl in p4.changes(&ChangeFilter::pending())? {
//!     println!("{} {}", cl.id, cl.summary());
//! }
//! # Ok::<(), p4::Error>(())
//! ```

mod client;
mod commands;
mod error;
mod model;
mod record;

pub use client::{Client, Connection};
pub use commands::ChangeFilter;
pub use error::{Error, Result};
pub use model::*;

pub use p4_sys::ffi::{P4Message, RunOutput, TaggedField, TaggedRecord};
