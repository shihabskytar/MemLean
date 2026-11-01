# MemLean architecture — Milestone 1 only

## Components (M1)

```
memlean-cli (crates/cli)
  └─► memlean-win::snapshot()        (Windows only)
  │     Toolhelp32 snapshot (pid/ppid/exe name)
  │     OpenProcess QUERY_LIMITED_INFORMATION
  │     QueryFullProcessImageNameW (exe path)
  │     GetProcessMemoryInfo (EX2: WorkingSet, PrivateWorkingSet, PrivateUsage)
  │     GetProcessTimes (creation time, user+kernel CPU)
  │     GetProcessIoCounters (read/write transfer bytes)
  └─► memlean-core::group_by_executable()
        group key: lowercased exe_path, or lowercased exe_name
        when the path is empty
        sums: private working set, working set, private bytes
        pids sorted ascending
  └─► display: groups sorted by private WS descending, or JSON
```

Data flow is one-way and side-effect free after sampling:
snapshot → group → sort → print. The CLI never writes, suspends,
trims, or otherwise touches any process; it only reads counters.

## What is deliberately not here (M1)

- No daemon, tray, hotkey, config, or activity log (Milestone 5).
- No policy engine deciding what may be suspended or trimmed
  (Milestone 2). `crates/core` groups and sums only.
- No recorder, ring buffer, triggers, or diagnosis engine
  (Milestones 6–8).
- No IPC, UI, report export, or installer (Milestones 9, 13, 15).

## Future process state machine — named, M3+

The full machine from the spec is:

```
Active → Idle candidate → Light-trimmed → Suspended → Resumed
```

It is **not implemented in Milestone 1** and is recorded here only so
later milestones do not have to rediscover it. Milestone 3 owns the
Suspended/Resumed transitions, crash-safe state, and the watchdog.

## Failure model (M1)

| Failure | Behaviour |
| --- | --- |
| Process cannot be opened (access denied, elevated, protected) | Skipped silently |
| Process exits mid-snapshot | Skipped silently (query failure) |
| A counter read fails | Process skipped, never partially reported |
| Toolhelp snapshot fails | Empty result, not an error |
| Non-Windows platform | Empty result + explicit limited-mode message |
| PID reuse | Not solved in M1; creation time is recorded for M3 validation |

## Invariants

- Every `unsafe` block in `crates/win` carries an invariants comment
  naming what must hold (valid handle, valid buffer + length,
  correctly-sized struct).
- `crates/core` contains no `unsafe` and no OS calls; its tests run
  identically on every platform.
- No network, no telemetry, no disk writes from the sampler or CLI.
