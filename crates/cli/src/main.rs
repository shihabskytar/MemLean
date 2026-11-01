//! `memlean-cli dump [--json]` — Milestone 1 grouped process dump.
//!
//! Grouping and sorting live in `memlean-core`; sampling lives in
//! `memlean-win`. This binary only presents the result. On non-Windows
//! platforms it reports limited mode and an empty group list — it never
//! fabricates process numbers.

use memlean_core::{group_by_executable, sort_groups_by_private_ws_desc, total_private_ws};
use serde::Serialize;

#[derive(Serialize)]
struct Dump {
    limited_mode: bool,
    groups: Vec<memlean_core::ProcessGroup>,
    total_private_working_set_bytes: u64,
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut want_json = false;
    let mut command: Option<&str> = None;
    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => want_json = true,
            other => {
                if command.is_none() {
                    command = Some(other);
                } else {
                    eprintln!("unexpected argument: {other}");
                    std::process::exit(2);
                }
            }
        }
    }
    if command != Some("dump") {
        eprintln!("usage: memlean-cli dump [--json]");
        std::process::exit(2);
    }

    // Limited mode is a platform fact, not a failure: the sampler is
    // Windows-only (see memlean-win). An empty snapshot on a
    // non-Windows host is therefore expected, not zero usage.
    let limited_mode = !cfg!(windows);
    let processes = memlean_win::snapshot();
    let mut groups = group_by_executable(&processes);
    sort_groups_by_private_ws_desc(&mut groups);
    let total = total_private_ws(&groups);

    if want_json {
        let dump = Dump {
            limited_mode,
            groups,
            total_private_working_set_bytes: total,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&dump).expect("dump is serializable")
        );
    } else {
        if limited_mode {
            println!(
                "limited mode: process snapshot is Windows-only; no processes listed on this platform"
            );
        }
        println!(
            "groups: {}  total private working set: {:.1} MB",
            groups.len(),
            mb(total)
        );
        for group in &groups {
            let pids: Vec<String> = group.pids.iter().map(u32::to_string).collect();
            println!(
                "{:>8.1} MB private WS | {:>8.1} MB WS | {:>3} proc | {} [{}]",
                mb(group.total_private_working_set_bytes),
                mb(group.total_working_set_bytes),
                group.process_count,
                group.exe_name,
                pids.join(", ")
            );
        }
    }
}
