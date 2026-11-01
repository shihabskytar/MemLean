//! MemLean core: pure, deterministic process grouping and metrics.
//!
//! No `unsafe`, no OS calls, no I/O. This crate is safe to unit-test on
//! any platform. The daemon policy engine (Milestone 2) lives elsewhere;
//! nothing in this module decides whether a process may be touched.
//!
//! Summation note: `total_private_working_set_bytes` is a plain sum over
//! processes. That is *not* double-counting: a private working set is, by
//! definition, pages owned by exactly one process, so summing private
//! working sets across processes counts each page once. Shared pages
//! live in `working_set_bytes` and are deliberately not summed as a
//! "total private" figure.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One process, as observed by a sampler (see `memlean-win`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    /// File name only, e.g. `chrome.exe`.
    pub exe_name: String,
    /// Full image path, e.g. `C:\Program Files\Google\Chrome\Application\chrome.exe`.
    /// Empty when the sampler could not determine it.
    pub exe_path: String,
    /// Private working set: private pages resident in RAM.
    pub private_working_set_bytes: u64,
    /// Total working set, including shared pages.
    pub working_set_bytes: u64,
    /// Private committed bytes (PROCESS_MEMORY_COUNTERS_EX `PrivateUsage`).
    pub private_bytes: u64,
    /// User + kernel CPU time, in milliseconds.
    pub cpu_time_ms: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    /// Creation time as FILETIME ticks (100 ns since 1601-01-01 UTC).
    /// 0 when unknown. Recorded so Milestone 3 can validate PIDs
    /// against reuse; it is not a uniqueness guarantee on its own.
    pub creation_time: u64,
}

/// Processes sharing one executable, as grouped by [`group_by_executable`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessGroup {
    pub exe_name: String,
    pub exe_path: String,
    pub process_count: usize,
    pub total_private_working_set_bytes: u64,
    pub total_working_set_bytes: u64,
    pub total_private_bytes: u64,
    /// Sorted ascending.
    pub pids: Vec<u32>,
}

fn group_key(p: &ProcessInfo) -> String {
    if !p.exe_path.is_empty() {
        p.exe_path.to_lowercase()
    } else {
        p.exe_name.to_lowercase()
    }
}

/// Group processes by executable.
///
/// Key: lowercased `exe_path`, falling back to lowercased `exe_name`
/// when the path is empty (the sampler could not determine it).
/// Within a group, `exe_path` is taken from the first process seen for
/// that key in input order, and `exe_name` likewise; `pids` are sorted
/// ascending. Groups are returned in key order (deterministic) — call
/// [`sort_groups_by_private_ws_desc`] for the CLI's display order.
pub fn group_by_executable(processes: &[ProcessInfo]) -> Vec<ProcessGroup> {
    let mut map: BTreeMap<String, ProcessGroup> = BTreeMap::new();
    for p in processes {
        let key = group_key(p);
        let group = map.entry(key).or_insert_with(|| ProcessGroup {
            exe_name: p.exe_name.clone(),
            exe_path: p.exe_path.clone(),
            process_count: 0,
            total_private_working_set_bytes: 0,
            total_working_set_bytes: 0,
            total_private_bytes: 0,
            pids: Vec::new(),
        });
        group.process_count += 1;
        // Saturating sums: a corrupt sampler must never panic the caller.
        group.total_private_working_set_bytes = group
            .total_private_working_set_bytes
            .saturating_add(p.private_working_set_bytes);
        group.total_working_set_bytes = group
            .total_working_set_bytes
            .saturating_add(p.working_set_bytes);
        group.total_private_bytes = group.total_private_bytes.saturating_add(p.private_bytes);
        group.pids.push(p.pid);
    }
    let mut groups: Vec<ProcessGroup> = map.into_values().collect();
    for group in &mut groups {
        group.pids.sort_unstable();
    }
    groups
}

/// Sort groups by total private working set, descending. Ties break by
/// `exe_path` ascending so the order is deterministic.
pub fn sort_groups_by_private_ws_desc(groups: &mut [ProcessGroup]) {
    groups.sort_by(|a, b| {
        b.total_private_working_set_bytes
            .cmp(&a.total_private_working_set_bytes)
            .then_with(|| a.exe_path.cmp(&b.exe_path))
    });
}

/// Sum of total private working sets across all groups.
pub fn total_private_ws(groups: &[ProcessGroup]) -> u64 {
    groups.iter().fold(0u64, |acc, g| {
        acc.saturating_add(g.total_private_working_set_bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pid: u32, ppid: u32, name: &str, path: &str, private_ws: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            exe_name: name.to_string(),
            exe_path: path.to_string(),
            private_working_set_bytes: private_ws,
            working_set_bytes: private_ws + 100,
            private_bytes: private_ws + 200,
            cpu_time_ms: 0,
            read_bytes: 0,
            write_bytes: 0,
            creation_time: 0,
        }
    }

    #[test]
    fn groups_by_path_case_insensitively() {
        let procs = vec![
            sample(10, 1, "chrome.exe", r"C:\App\Chrome.exe", 100),
            sample(11, 1, "chrome.exe", r"c:\app\chrome.exe", 50),
            sample(12, 1, "notepad.exe", r"C:\Windows\notepad.exe", 70),
        ];
        let groups = group_by_executable(&procs);
        assert_eq!(groups.len(), 2);
        let chrome = groups.iter().find(|g| g.exe_name == "chrome.exe").unwrap();
        assert_eq!(chrome.process_count, 2);
        assert_eq!(chrome.total_private_working_set_bytes, 150);
        assert_eq!(chrome.pids, vec![10, 11]);
    }

    #[test]
    fn falls_back_to_name_when_path_empty() {
        let procs = vec![
            sample(20, 1, "helper.exe", "", 30),
            sample(21, 1, "helper.exe", "", 40),
            sample(22, 1, "other.exe", "", 10),
        ];
        let groups = group_by_executable(&procs);
        assert_eq!(groups.len(), 2);
        let helper = groups.iter().find(|g| g.exe_name == "helper.exe").unwrap();
        assert_eq!(helper.process_count, 2);
        assert_eq!(helper.total_private_working_set_bytes, 70);
    }

    #[test]
    fn private_ws_sums_do_not_double_count_shared() {
        // Private WS is per-process private by definition; the sum is
        // exactly the sum of the parts, even when working sets overlap
        // in shared pages (working_set_bytes is larger here on purpose).
        let procs = vec![
            sample(30, 1, "a.exe", r"C:\a.exe", 1000),
            sample(31, 1, "a.exe", r"C:\a.exe", 2000),
        ];
        let groups = group_by_executable(&procs);
        assert_eq!(groups[0].total_private_working_set_bytes, 3000);
        assert_eq!(groups[0].total_working_set_bytes, 3200);
        assert_eq!(total_private_ws(&groups), 3000);
    }

    #[test]
    fn empty_input_yields_no_groups() {
        assert!(group_by_executable(&[]).is_empty());
        assert_eq!(total_private_ws(&[]), 0);
    }

    #[test]
    fn pids_are_sorted() {
        let procs = vec![
            sample(55, 1, "a.exe", r"C:\a.exe", 10),
            sample(12, 1, "a.exe", r"C:\a.exe", 10),
            sample(40, 1, "a.exe", r"C:\a.exe", 10),
        ];
        let groups = group_by_executable(&procs);
        assert_eq!(groups[0].pids, vec![12, 40, 55]);
    }

    #[test]
    fn single_process_group_and_sort() {
        let procs = vec![sample(7, 1, "solo.exe", r"C:\solo.exe", 42)];
        let mut groups = group_by_executable(&procs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].process_count, 1);
        sort_groups_by_private_ws_desc(&mut groups);
        assert_eq!(groups[0].total_private_working_set_bytes, 42);
    }

    #[test]
    fn sort_is_descending_with_deterministic_ties() {
        let mut groups = group_by_executable(&[
            sample(1, 0, "b.exe", r"C:\b.exe", 50),
            sample(2, 0, "a.exe", r"C:\a.exe", 50),
            sample(3, 0, "c.exe", r"C:\c.exe", 90),
        ]);
        sort_groups_by_private_ws_desc(&mut groups);
        assert_eq!(groups[0].exe_name, "c.exe");
        // Tie at 50: exe_path ascending decides.
        assert_eq!(groups[1].exe_name, "a.exe");
        assert_eq!(groups[2].exe_name, "b.exe");
    }
}
