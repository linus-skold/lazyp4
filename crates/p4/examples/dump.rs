//! Dev tool: run any command and print the raw tagged output.
//!
//! Use it to see the shape of a command's records before writing a parser.
//!
//! Pass `--untagged` first to see the plain text form instead.
//!
//! ```text
//! cargo run -p p4 --example dump -- describe -du -S 166
//! cargo run -p p4 --example dump -- --untagged describe -du 396
//! ```

use p4::{Client, Connection};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let mut untagged = false;
    let mut cwd = None;
    while let Some(flag) = args.first().cloned() {
        match flag.as_str() {
            "--untagged" => {
                untagged = true;
                args.remove(0);
            }
            // Where to resolve P4CONFIG from, when it is not the process cwd.
            "--cwd" => {
                args.remove(0);
                cwd = Some(args.remove(0));
            }
            _ => break,
        }
    }

    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("usage: dump [--untagged] [--cwd <dir>] <command> [args...]");
        std::process::exit(2);
    };
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();

    let conn = Connection {
        cwd,
        ..if untagged {
            Connection::untagged()
        } else {
            Connection::default()
        }
    };
    let mut p4 = Client::connect(&conn).expect("connect");
    let out = p4.run_raw(cmd, &rest, "").expect("run");

    for (i, record) in out.records.iter().enumerate() {
        println!("--- record {i} ---");
        for field in &record.fields {
            println!("  {:<16} {:?}", field.key, field.value);
        }
    }
    for line in &out.info {
        println!("[info @{}] {:?}", line.at, line.text);
    }
    for msg in &out.messages {
        println!("[msg {}] {:?}", msg.severity, msg.text);
    }
    if !out.text.is_empty() {
        println!("--- text ({} bytes) ---", out.text.len());
        println!("{}", String::from_utf8_lossy(&out.text));
    }
}
