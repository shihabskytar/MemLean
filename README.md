# MemLean

**Frees memory when you're short, and tells you exactly what went wrong
when your PC glitches.**

MemLean is an offline-first, open-source tool for Windows 10/11 PCs
with 4–8 GB of RAM. It will have two parts:

1. **Governor** — keeps the PC responsive by suspending or trimming
   idle background apps, throttling background processes, and
   catching memory leaks.
2. **Flight Recorder** — continuously records system health in a small
   rolling in-memory buffer, and when the PC freezes or lags, explains
   in plain English what caused it and offers one-click fixes.

> **Milestone 1 status — read this first.** This milestone is process
> snapshotting, grouping, private-working-set metrics, and a CLI dump
> only. There is **no daemon, no governor, no recorder, no UI, and no
> installer yet** — those are Milestones 2–15. Nothing in this README
> claims otherwise.

## What MemLean does NOT do (and why)

- **No "RAM cleaner" that flushes the standby list or calls
  `EmptyWorkingSet` system-wide.** Flushing standby memory throws away
  cached pages Windows would otherwise reuse for free, and forces
  re-reads from disk — it makes a slow PC slower, while looking
  productive in a meter. MemLean trims only individual processes it
  has a reason to touch (from Milestone 3 on), never the whole system.
- **No registry cleaner.** Registry "cleaning" has no measurable
  performance benefit on modern Windows and risks breaking
  applications. It will never be added.
- **No fake health scores.** MemLean reports measured numbers (MB,
  CPU %, faults/s) and says "inconclusive" when the evidence does not
  support a verdict. A single 0–100 "health score" would hide exactly
  the information a user needs.
- **No telemetry, no ads, no network calls in v1.** All data stays in
  `%APPDATA%\MemLean\`. Report export (Milestone 13) is a local file
  only; nothing is uploaded anywhere.

## Safety model (one paragraph)

MemLean runs as a standard user and only ever opens processes with
`PROCESS_QUERY_LIMITED_INFORMATION` (Milestone 1) and, from Milestone 3,
`PROCESS_SET_INFORMATION | PROCESS_SUSPEND_RESUME` for processes it
can open that way — everything else is skipped silently, never forced.
When uncertain, it does nothing: no action is taken on a process it
cannot positively identify, and every governor action is reversible
(resume on focus, panic hotkey, resume-everything on quit). A
suspended process is never left suspended by a crash: suspended PIDs
plus creation times are persisted, and a watchdog resumes leftovers on
restart (Milestone 3).

## Limited mode

Some features need privileges or counters this PC does not have.
MemLean degrades gracefully instead of failing: a feature that needs
admin (e.g. full ETW kernel sessions) falls back to counter-based
data and is flagged **"limited mode"** in the UI. On non-Windows
platforms the process sampler returns no data at all — the CLI says
so plainly (`limited mode: process snapshot is Windows-only`) and
never presents fabricated numbers as if they were real measurements.

## Build and run (Milestone 1)

Prerequisites: Rust stable (via rustup). On Windows, the
`x86_64-pc-windows-msvc` target and MSVC build tools.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -p memlean-cli -- dump
cargo run -p memlean-cli -- dump --json
```

Windows-target type check (no runtime needed):

```bash
rustup target add x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-msvc
```

## Layout

| Crate | What it does |
| --- | --- |
| `crates/core` | Pure grouping/metrics logic, no `unsafe`, fully unit-tested |
| `crates/win` | All `unsafe` Win32 code: Toolhelp32 snapshot + PSAPI counters |
| `crates/cli` | `memlean-cli dump [--json]` |

Future crates from the spec (`daemon`, `ui`, `ipc`, `recorder`,
`report`) are **not** created in Milestone 1 — no empty placeholders.

## Assumptions

- **Windows snapshot code is written and Windows-target-checked
  (`cargo check --target x86_64-pc-windows-msvc` passes, including
  clippy), but it cannot be runtime-tested on this Linux VM.** CI on
  `windows-latest` is the runtime check; a real Windows run is still
  needed before any claim about live snapshot behaviour.
- **Daemon footprint/CPU budgets (≤ 30 MB, ≤ 0.3% idle CPU, ≤ 4 MB
  ring buffer) are NOT yet measured** — Milestone 1 has no daemon,
  so there is nothing to measure. No footprint numbers are claimed.
- The `windows` crate is pinned to 0.61 (resolved to 0.61.3 in the
  lockfile); its 0.61 API surface differs from older versions
  (several PSAPI/Toolhelp functions return `Result` and
  `CreateToolhelp32Snapshot`/`OpenProcess` are `unsafe`), and the
  code is written against that surface.
- Private working set comes from `PROCESS_MEMORY_COUNTERS_EX2`
  `PrivateWorkingSetSize`, distinct from committed private bytes
  (`PrivateUsage`). Summing private working sets across processes
  does not double-count, because private pages belong to one process
  by definition (see `crates/core` docs).

## License

Apache-2.0 — see `LICENSE`.
