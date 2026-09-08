//! Print a normalized unified patch for a changelist or for open files.
//!
//! ```text
//! cargo run -p p4 --example patch -- 396            # submitted
//! cargo run -p p4 --example patch -- -S 166         # a shelf
//! cargo run -p p4 --example patch -- --opened       # workspace changes
//! ```

use p4::{diff, ChangeId, Client, Connection};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let mut cwd = None;
    if args.first().is_some_and(|a| a == "--cwd") {
        args.remove(0);
        cwd = Some(args.remove(0));
    }
    let shelved = args.first().is_some_and(|a| a == "-S");
    if shelved {
        args.remove(0);
    }

    // Diff content only comes back on an untagged connection.
    let conn = Connection {
        cwd,
        ..Connection::untagged()
    };
    let mut p4 = Client::connect(&conn).expect("connect");

    let raw = match args.first().map(String::as_str) {
        Some("--opened") | None => p4.diff_text(&[]).expect("diff"),
        Some(spec) => {
            let change: ChangeId = spec.parse().expect("changelist number");
            p4.describe_diff_text(change, shelved).expect("describe")
        }
    };

    let files = diff::normalize(&raw);
    eprintln!("-- {} file(s) with hunks --", files.iter().filter(|f| !f.hunks.is_empty()).count());
    for f in &files {
        eprintln!("   {} {}", if f.hunks.is_empty() { "(no diff)" } else { "         " }, f.depot_path);
    }
    print!("{}", diff::to_unified(&files));
}
