//! MemLean Windows process snapshotting.
//!
//! All `unsafe` Win32 code in Milestone 1 lives here. The sampler is
//! deliberately conservative:
//! - It opens processes with `PROCESS_QUERY_LIMITED_INFORMATION` only
//!   (standard-user access). Anything it cannot open is skipped
//!   silently — access denied and exit races are normal, not errors.
//! - It never panics on a failed query and never returns partial
//!   garbage: a process whose counters cannot be read is skipped.
//! - On non-Windows targets [`snapshot`] returns an empty `Vec` with a
//!   documented reason. It does **not** fabricate data from another OS
//!   (e.g. Linux `/proc`) and present it as a Windows snapshot.
//!
//! PID reuse is not solved here: [`memlean_core::ProcessInfo::creation_time`]
//! is recorded so Milestone 3 can validate a PID before resuming it.

pub use memlean_core::ProcessInfo;

#[cfg(windows)]
mod windows_impl {
    use super::ProcessInfo;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows::Win32::System::Threading::{
        GetProcessIoCounters, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// RAII wrapper: the handle is closed exactly once on drop.
    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // Invariant: `self.0` is a valid handle obtained from a Win32
            // creator API (snapshot or OpenProcess), never null and never
            // already closed. CloseHandle on such a handle is safe.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn filetime_to_u64(ft: &FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
    }

    fn filetime_to_ms(ft: &FILETIME) -> u64 {
        // FILETIME ticks are 100 ns; 10_000 ticks = 1 ms.
        filetime_to_u64(ft) / 10_000
    }

    fn wide_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn query_process(pid: u32, ppid: u32, exe_name: String) -> Option<ProcessInfo> {
        // Invariant: OpenProcess with QUERY_LIMITED_INFORMATION only;
        // it returns Err for protected/elevated/foreign processes and
        // for processes that exited in the meantime. Either way we skip.
        // Invariant: pid comes from the Toolhelp snapshot just taken;
        // QUERY_LIMITED_INFORMATION is the least access that still
        // yields memory/times counters, and failure is handled by `?`.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let handle = OwnedHandle(handle);

        // Full image path (for grouping). Failure is non-fatal: the
        // core groups by exe name when the path is empty.
        let mut path_buf = [0u16; 1024];
        let mut path_len = path_buf.len() as u32;
        // Invariant: path_buf is a valid 1024-wide-char buffer and
        // path_len is its capacity on entry; on success it holds the
        // length written. The API writes at most path_len characters.
        let exe_path = unsafe {
            if QueryFullProcessImageNameW(
                handle.0,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(path_buf.as_mut_ptr()),
                &mut path_len,
            )
            .is_ok()
            {
                wide_to_string(&path_buf[..path_len as usize])
            } else {
                String::new()
            }
        };

        // Memory counters (EX2 gives PrivateWorkingSetSize, i.e. the
        // private working set, distinct from PrivateUsage/committed).
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        // Invariant: counters is a valid, correctly-sized EX2 struct and
        // handle.0 is a live process handle. GetProcessMemoryInfo only
        // writes cb bytes into it. A false result means the process
        // exited or is unreadable: skip it rather than invent numbers.
        let mem_ok = unsafe {
            GetProcessMemoryInfo(handle.0, &mut counters as *mut _ as *mut _, counters.cb).is_ok()
        };
        if !mem_ok {
            return None;
        }

        // CPU times and creation time.
        let mut creation = FILETIME::default();
        let mut _exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // Invariant: all four FILETIME out-params are valid stack
        // addresses, handle.0 is live. Failure => skip the process.
        let times_ok = unsafe {
            GetProcessTimes(handle.0, &mut creation, &mut _exit, &mut kernel, &mut user).is_ok()
        };
        if !times_ok {
            return None;
        }

        // I/O counters. Failure is tolerated (zeros) because I/O is
        // advisory in Milestone 1; memory and CPU are the hard signals.
        let mut io = Default::default();
        // Invariant: io is a valid IO_COUNTERS struct, handle.0 live.
        let _ = unsafe { GetProcessIoCounters(handle.0, &mut io) };

        Some(ProcessInfo {
            pid,
            ppid,
            exe_name,
            exe_path,
            private_working_set_bytes: counters.PrivateWorkingSetSize as u64,
            working_set_bytes: counters.WorkingSetSize as u64,
            private_bytes: counters.PrivateUsage as u64,
            cpu_time_ms: filetime_to_ms(&kernel).saturating_add(filetime_to_ms(&user)),
            read_bytes: io.ReadTransferCount,
            write_bytes: io.WriteTransferCount,
            creation_time: filetime_to_u64(&creation),
        })
    }

    pub fn snapshot() -> Vec<ProcessInfo> {
        // Invariant: TH32CS_SNAPPROCESS with PID 0 snapshots all
        // processes; the returned handle (on Ok) is valid until closed
        // by OwnedHandle. On Err there is nothing to enumerate, so an
        // empty result is the honest answer, not an error.
        let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(h) => OwnedHandle(h),
            Err(_) => return Vec::new(),
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // Invariant: entry.dwSize is set before the first call, as the
        // API requires, and entry is a valid PROCESSENTRY32W address.
        if unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_err() {
            return Vec::new();
        }

        let mut out = Vec::new();
        loop {
            let pid = entry.th32ProcessID;
            let ppid = entry.th32ParentProcessID;
            let exe_name = wide_to_string(&entry.szExeFile);
            // PID 0 (System Idle) has no meaningful counters; skip it.
            if pid != 0 {
                if let Some(info) = query_process(pid, ppid, exe_name) {
                    out.push(info);
                }
            }
            // Invariant: entry remains a valid PROCESSENTRY32W address;
            // Err means enumeration ended (or the snapshot became
            // invalid), so we stop instead of looping forever.
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        out
    }
}

/// Snapshot all processes visible to this (standard) user.
///
/// Windows: real Toolhelp32 + PSAPI snapshot (see module docs).
/// Other platforms: returns an empty `Vec` — the sampler is
/// Windows-only and this build cannot observe Windows processes.
/// Callers must surface that as "limited mode", never as zero usage.
#[cfg(windows)]
pub fn snapshot() -> Vec<ProcessInfo> {
    windows_impl::snapshot()
}

#[cfg(not(windows))]
pub fn snapshot() -> Vec<ProcessInfo> {
    // Documented non-Windows behaviour: no data is fabricated here.
    // The CLI reports limited mode for this platform.
    Vec::new()
}
