//! List pending and recently submitted changelists through the typed commands.
//!
//! ```text
//! cargo run -p p4 --example changes
//! ```

use p4::{ChangeFilter, Client, Connection};

fn main() {
    let mut p4 = match Client::connect(&Connection::default()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect: {e}");
            std::process::exit(1);
        }
    };

    let info = p4.info().unwrap_or_else(|e| fail("info", e));
    println!(
        "{}@{} on {} ({})",
        info.user,
        if info.client_known { &info.client } else { "no client" },
        info.server_address,
        info.server_version
    );

    println!("\n-- pending --");
    for cl in p4
        .changes(&ChangeFilter::pending())
        .unwrap_or_else(|e| fail("changes", e))
    {
        println!(
            "{:>8} {:<12} {}{}",
            cl.id.to_string(),
            cl.user,
            cl.summary(),
            if cl.shelved { "  [shelved]" } else { "" }
        );

        for f in p4
            .describe(cl.id, false)
            .map(|d| d.files)
            .unwrap_or_default()
        {
            println!("           {} {}", f.action.code(), f.depot_path);
        }
    }

    println!("\n-- last 5 submitted --");
    for cl in p4
        .changes(&ChangeFilter::submitted(5))
        .unwrap_or_else(|e| fail("changes", e))
    {
        println!("{:>8} {:<12} {}", cl.id.to_string(), cl.user, cl.summary());
    }

    println!("\n-- opened --");
    for f in p4.opened(None).unwrap_or_else(|e| fail("opened", e)) {
        println!("{} {} ({})", f.action.code(), f.depot_path, f.change);
    }
}

fn fail(what: &str, e: p4::Error) -> ! {
    eprintln!("{what}: {e}");
    std::process::exit(1);
}
