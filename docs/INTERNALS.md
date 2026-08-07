# How it works, and what it costs

Everything here is for the curious. None of it is needed to use the tool.

## Architecture

- The tray icon, menu, and notifications use the Win32 shell APIs directly. No Electron, no framework, no background service.
- A `WM_TIMER` every two seconds asks `EnumProcesses` for the current process ids and compares them against a cache of already resolved executable names, so a steady state tick is one syscall and some hash lookups. The rest of the time the process sits blocked in `GetMessageW` and does nothing at all.
- A report is only sent when the target rate actually changes, so a quiet desktop session sends nothing after the first one.
- Rates are applied by opening the mouse vendor HID collection and sending a feature report, built in for the Model D2 Pro 4K and overridable with a `protocol.toml`. See [PROTOCOL.md](PROTOCOL.md).
- Start with Windows is a single value under the per user `Run` key. There is no second copy of that state in the settings file, so the menu can never disagree with what Windows will actually do.
- The process trims its working set twice: once at startup, and again whenever the tray menu closes. Because a timer tick touches so little, the pages then stay reclaimed and it sits near 1.3 MB rather than growing back.
- The window is hidden but is a real top level window, not a message only one parented to `HWND_MESSAGE`. That looks like the wrong choice for something with no UI, and it is deliberate: Explorer announces a shell restart by broadcasting `TaskbarCreated`, broadcasts do not reach message only windows, and without it the tray icon disappears for good the first time Explorer dies. `WS_EX_TOOLWINDOW` keeps the window out of the taskbar and Alt Tab.
- A notification means one thing: a program on your list opened and the active rate went on. It is an edge on whether anything is running rather than a comparison of rate values, so a game that stays open notifies once no matter how many ticks pass. Manual actions pass `Silent::Yes` and say nothing, on the grounds that you already know what you just clicked.

## Footprint

Measured on the release build, sampled every minute, on a desktop with about 190 processes running and a 12 core CPU:

| Metric | Value |
| --- | --- |
| Executable size | 0.50 MB |
| Private memory | about 1.8 to 2.1 MB |
| Working set, idle | about 1.3 to 2.0 MB |
| Working set, while the tray menu is open | about 4.4 to 5.4 MB, released when it closes |
| CPU | 31 ms over 300 s, which is **0.01 percent of one core** |

The working set deserves the second row rather than a single flattering number. Opening the tray menu pulls in the Windows shell libraries, `shell32`, `comctl32`, `uxtheme` and friends, and those are most of what the process is holding while a menu is on screen. Measured across a real menu open and close:

| Moment | Working set |
| --- | --- |
| Running, menu never opened | 1280 KB |
| Menu on screen | 4436 KB |
| Menu closed, after the trim | 1388 KB |
| Six seconds later | 1428 KB |

Left alone it would simply stay at the higher number for the rest of the session, so the menu handler trims on the way out. Note that most of that difference is shared library pages that other processes are already using, not memory your machine would otherwise have free, which is why private memory barely moves across the same sequence.

How much the menu costs depends on how much of the shell is already paged in. The 4436 KB above is a warm machine. Opening the menu for the first time straight after Explorer has restarted, when none of those libraries are resident, was measured at 9852 KB, trimming back to 2000 KB on close. The trim is what matters and it holds in both cases.

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
