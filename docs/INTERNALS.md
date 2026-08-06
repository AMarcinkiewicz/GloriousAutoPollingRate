# How it works, and what it costs

Everything here is for the curious. None of it is needed to use the tool.

## Architecture

- The tray icon, menu, and notifications use the Win32 shell APIs directly. No Electron, no framework, no background service.
- A `WM_TIMER` every two seconds asks `EnumProcesses` for the current process ids and compares them against a cache of already resolved executable names, so a steady state tick is one syscall and some hash lookups. The rest of the time the process sits blocked in `GetMessageW` and does nothing at all.
- A report is only sent when the target rate actually changes, so a quiet desktop session sends nothing after the first one.
- Rates are applied by opening the mouse vendor HID collection and sending a feature report, built in for the Model D2 Pro 4K and overridable with a `protocol.toml`. See [PROTOCOL.md](PROTOCOL.md).
- Start with Windows is a single value under the per user `Run` key. There is no second copy of that state in the settings file, so the menu can never disagree with what Windows will actually do.
- On startup the process trims its working set, a one time reclaim of cold pages. Because a tick touches so little, the pages largely stay reclaimed and it sits near 1 MB rather than growing back.

## Footprint

Measured on the release build, sampled every minute, on a desktop with about 190 processes running and a 12 core CPU:

| Metric | Value |
| --- | --- |
| Executable size | 0.50 MB |
| Private memory | about 1.7 MB |
| Working set | 0.9 MB to 2.1 MB |
| CPU | 31 ms over 300 s, which is **0.01 percent of one core** |

## The process scan

The only recurring work is one process list scan every two seconds, and it is deliberately not the obvious implementation.

The obvious one is a `CreateToolhelp32Snapshot` walk. That costs about **3.6 ms** per call here, because it builds a snapshot of every process before anything is compared, and stopping early on a match barely helps since the expensive part has already happened. It also gets worse, not better, under exactly the conditions you care about: measured during a real Counter Strike 2 session it rose to 5.6 ms typical and 7.2 ms worst.

Instead, `EnumProcesses` returns just the list of process ids, which is one syscall and about 20 us. A process id's executable name never changes, so a name only ever has to be resolved once and is then remembered. On a normal tick nothing has started or exited, so the whole call is that one syscall plus a few hash lookups.

Measured back to back on the same machine, worst case, meaning nothing on the list is running so there is no early exit:

| Approach | Typical | Worst |
| --- | --- | --- |
| Toolhelp snapshot | 3603 us | 6695 us |
| `EnumProcesses`, ids only | 21 us | 62 us |
| `EnumProcesses`, resolving every id every time | 2743 us | 3607 us |
| **`EnumProcesses` plus a name cache**, what this uses | **27 us** | **67 us** |

About 130 times cheaper than the snapshot walk. So if you arrived wondering whether a background tool polling every two seconds could cost you a frame, the answer is that it does roughly 27 microseconds of work on one core while your game has the other eleven.

Two details are load bearing. The cache stores the resolved **name**, never a match result, because caching the result would bake in whichever watch list was current and keep answering with it after you edit the list. And it reconciles against the live id list on every tick, because Windows reuses process ids, and a stale entry would otherwise answer for a completely different process.

If you want it cheaper still, `poll_interval_ms` in `settings.toml` sets the interval. There is not much left to save, and the tradeoff is that a game takes longer to notice.

## A note on measuring this

Windows accounts process CPU time in steps of about 15.6 ms, which is far coarser than a single scan. Dividing a process CPU total by the number of scans is therefore not a reliable way to get a per scan figure. The numbers above come from timing the shipping function directly over hundreds of calls.
