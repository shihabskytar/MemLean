<div align="center">

# 🧹 MemLean

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![CI](https://github.com/shihabskytar/MemLean/actions/workflows/ci.yml/badge.svg)](https://github.com/shihabskytar/MemLean/actions/workflows/ci.yml)
[![Windows](https://img.shields.io/badge/Windows-10%20%2B%2011-0078D6.svg)](ARCHITECTURE.md)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Offline first](https://img.shields.io/badge/Offline-first-00A67E.svg)](ARCHITECTURE.md)

### Frees memory when you're short, and tells you exactly what went wrong when your PC glitches.

**An offline-first, open-source RAM governor + flight recorder for Windows 10/11 PCs with 4–8 GB of RAM. No accounts. No telemetry. No network calls in v1. Nothing leaves your PC.**

</div>

> **Milestone 1 status — read this first.** This milestone is process snapshotting, grouping, private-working-set metrics, and a CLI dump only. There is **no daemon, no governor, no recorder, no UI, and no installer yet** — those are Milestones 2–15. Nothing in this README claims otherwise.

---

## Contents

- [What MemLean is](#what-memlean-is)
- [What MemLean is not](#what-memlean-is-not)
- [Tutorial](#tutorial)
- [How snapshotting works](#how-snapshotting-works)
- [Safety model](#safety-model)
- [Limited mode](#limited-mode)
- [Layout](#layout)
- [Roadmap](#roadmap)
- [Assumptions and honest limits](#assumptions-and-honest-limits)
- [License](#license)

---

## What MemLean is

MemLean will have two parts:

| Part | Job |
| --- | --- |
| **Governor** | Keeps the PC responsive by suspending or trimming idle background apps, throttling background processes, and catching memory leaks. |
| **Flight Recorder** | Continuously records system health in a small rolling in-memory buffer, and when the PC freezes or lags, explains in plain English what caused it and offers one-click fixes. |

Target users: students, office users, laptop owners, and IT freelancers (report export) on PCs where a RAM upgrade is expensive.

## What MemLean is not

| Never built | Why |
| --- | --- |
| **No "RAM cleaner" that flushes the standby list or calls `EmptyWorkingSet` system-wide** | Flushing standby memory throws away cached pages Windows would otherwise reuse for free, and forces re-reads from disk — it makes a slow PC slower while looking productive in a meter. MemLean trims only individual processes it has a reason to touch (from Milestone 3 on), never the whole system. |
| **No registry cleaner** | Registry "cleaning" has no measurable performance benefit on modern Windows and risks breaking applications. It will never be added. |
| **No fake health scores** | MemLean reports measured numbers (MB, CPU %, faults/s) and says "inconclusive" when the evidence does not support a verdict. A single 0–100 "health score" would hide exactly the information a user needs. |
| **No telemetry, no ads, no network calls in v1** | All data stays in `%APPDATA%\MemLean\`. Report export (Milestone 13) is a local file only; nothing is uploaded anywhere. |

---

## Tutorial

This tutorial takes you from a fresh clone to your first grouped process dump, in about five minutes. Every command below is copy-pasteable, and every expected output is shown.

### Step 1 — Prerequisites

| Requirement | Notes |
| --- | --- |
| Rust stable via [rustup](https://rustup.rs/) | `cargo`, `rustc`, `clippy`, and `rustfmt` are all used below |
| Windows 10/11 | Required for a real process snapshot (see [Limited mode](#limited-mode)) |
| MSVC build tools | Required only if you run the Windows-target check in Step 6 |

Check your toolchain first:

```bash
cargo --version
rustc --version
```

### Step 2 — Clone the repository

```bash
git clone https://github.com/shihabskytar/MemLean
cd MemLean
```

### Step 3 — Run the test suite

Milestone 1 ships with 7 core unit tests covering grouping, private-working-set sums, empty input, PID sorting, and sort order:

```bash
cargo test
```

Expected result:

```text
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Step 4 — Check code quality

The project enforces zero warnings and formatted code in CI, so run the same gates locally:

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Both commands should finish with no output — silence means clean.

### Step 5 — Run your first dump

```bash
cargo run -p memlean-cli -- dump
```

On Windows, you will see one line per executable group, sorted by private working set descending (numbers below are an illustrative format example, not a measured result):

```text
groups: 12  total private working set: 1843.2 MB
  612.4 MB private WS |   845.1 MB WS |   8 proc | chrome.exe [1234, 2345, 3456, ...]
  288.0 MB private WS |   310.5 MB WS |   1 proc | code.exe [4567]
```

Reading the columns, left to right:

| Column | Meaning |
| --- | --- |
| `MB private WS` | Memory private to those processes — the number that actually counts toward RAM pressure. Summing it across groups does **not** double-count, because private pages belong to one process by definition. |
| `MB WS` | Total working set, including shared pages — useful for context, not for pressure. |
| `proc` | How many processes share that executable (browsers and Electron apps have many). |
| `exe [PIDs]` | The executable name and every process ID in the group, so you can cross-check in Task Manager. |

For machine-readable output, add `--json`:

```bash
cargo run -p memlean-cli -- dump --json
```

```json
{
  "limited_mode": false,
  "groups": [],
  "total_private_working_set_bytes": 0
}
```

The JSON shape is always the same three fields: `limited_mode`, `groups`, and `total_private_working_set_bytes`. (The example above shows the empty-group shape; on Windows, `groups` is populated and `limited_mode` is `false`.)

### Step 6 — Verify the Windows target (optional, no Windows machine needed)

This type-checks the Win32 snapshot code for Windows without running it:

```bash
rustup target add x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-msvc
```

A full Windows *build* needs an MSVC linker and is therefore a CI job, not a local step on Linux.

### Step 7 — Troubleshooting

| Symptom | Meaning | What to do |
| --- | --- | --- |
| `limited mode: process snapshot is Windows-only; no processes listed on this platform` | You are not on Windows. The sampler is Windows-only by design. | Run the dump on Windows 10/11, or use the Windows-target check in Step 6. |
| A process you expected is missing from the dump | It could not be opened with query-limited access (elevated, protected, or exited mid-snapshot). | This is by design — skipped silently, never forced. See [Safety model](#safety-model). |
| `cargo fmt --check` prints a diff | Your local formatting differs from the project standard. | Run `cargo fmt`, then re-run the check. |

---

## How snapshotting works

```mermaid
flowchart LR
  CLI[memlean-cli dump] --> WIN[memlean-win::snapshot]
  WIN --> CORE[memlean-core::group_by_executable]
  CORE --> OUT[sorted groups / JSON]
```

Data flow is one-way and side-effect free after sampling: snapshot → group → sort → print. The CLI never writes, suspends, trims, or otherwise touches any process; it only reads counters.

| Win32 API | What it provides |
| --- | --- |
| Toolhelp32 snapshot | Process ID, parent ID, executable name |
| `OpenProcess` (`PROCESS_QUERY_LIMITED_INFORMATION` only) | A handle the standard user is allowed to hold |
| `QueryFullProcessImageNameW` | Full executable path (the grouping key) |
| `GetProcessMemoryInfo` (`PROCESS_MEMORY_COUNTERS_EX2`) | Working set, private working set, committed private bytes |
| `GetProcessTimes` | Creation time (for PID-reuse validation in Milestone 3), user + kernel CPU time |
| `GetProcessIoCounters` | Read/write transfer bytes |

Full detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## Safety model

MemLean runs as a standard user and only ever opens processes with `PROCESS_QUERY_LIMITED_INFORMATION` (Milestone 1) and, from Milestone 3, `PROCESS_SET_INFORMATION | PROCESS_SUSPEND_RESUME` for processes it can open that way — everything else is skipped silently, never forced.

When uncertain, it does nothing: no action is taken on a process it cannot positively identify, and every governor action is reversible (resume on focus, panic hotkey, resume-everything on quit). A suspended process is never left suspended by a crash: suspended PIDs plus creation times are persisted, and a watchdog resumes leftovers on restart (Milestone 3).

## Limited mode

Some features need privileges or counters this PC does not have. MemLean degrades gracefully instead of failing: a feature that needs admin (e.g. full ETW kernel sessions) falls back to counter-based data and is flagged **"limited mode"** in the UI.

On non-Windows platforms the process sampler returns no data at all — the CLI says so plainly (`limited mode: process snapshot is Windows-only`) and never presents fabricated numbers as if they were real measurements.

## Layout

| Crate | What it does |
| --- | --- |
| `crates/core` | Pure grouping/metrics logic, no `unsafe`, fully unit-tested |
| `crates/win` | All `unsafe` Win32 code: Toolhelp32 snapshot + PSAPI counters |
| `crates/cli` | `memlean-cli dump [--json]` |

Future crates from the spec (`daemon`, `ui`, `ipc`, `recorder`, `report`) are **not** created in Milestone 1 — no empty placeholders.

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| **1** | Process snapshotting, grouping, private-working-set metrics, CLI dump | ✅ Done |
| 2 | Governor policy engine + fixtures + tests | ⏳ Next |
| 3 | Suspend/resume/trim actuators + crash-safe state + recovery + watchdog | ⏳ |
| 4 | Foreground hook + idle detection (audio, mic/cam, CPU, I/O, network) | ⏳ |
| 5 | Daemon tray app, pause, panic hotkey, config, activity log | ⏳ |
| 6 | Recorder core: counters via PDH, ring buffer, footprint test | ⏳ |
| 7 | Recorder events, triggers, incident serialization | ⏳ |
| 8 | Diagnosis rule engine + fixtures + tests | ⏳ |
| 9 | IPC + dashboard UI (Now, Incidents, Bloat, Activity/Undo, whitelist) | ⏳ |
| 10 | Leak watchdog + restart flow | ⏳ |
| 11 | EcoQoS throttling + profiles | ⏳ |
| 12 | One-click fixes + governor/recorder synergy | ⏳ |
| 13 | HTML/PDF report export + redaction | ⏳ |
| 14 | Startup impact view | ⏳ |
| 15 | Installer, README, footprint benchmark | ⏳ |

## Assumptions and honest limits

- **Windows snapshot code is written and Windows-target-checked (`cargo check --target x86_64-pc-windows-msvc` passes, including clippy), but it cannot be runtime-tested on this Linux VM.** CI on `windows-latest` is the runtime check; a real Windows run is still needed before any claim about live snapshot behaviour.
- **Daemon footprint/CPU budgets (≤ 30 MB, ≤ 0.3% idle CPU, ≤ 4 MB ring buffer) are NOT yet measured** — Milestone 1 has no daemon, so there is nothing to measure. No footprint numbers are claimed.
- The `windows` crate is pinned to 0.61 (resolved to 0.61.3 in the lockfile); its 0.61 API surface differs from older versions (several PSAPI/Toolhelp functions return `Result` and `CreateToolhelp32Snapshot`/`OpenProcess` are `unsafe`), and the code is written against that surface.
- Private working set comes from `PROCESS_MEMORY_COUNTERS_EX2` `PrivateWorkingSetSize`, distinct from committed private bytes (`PrivateUsage`). Summing private working sets across processes does not double-count, because private pages belong to one process by definition (see `crates/core` docs).

## License

Apache-2.0 — see [LICENSE](LICENSE).
