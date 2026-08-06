# Glorious Auto Polling Rate

A tiny Windows tray tool that raises your Glorious mouse polling rate while a game is open and drops it back the rest of the time. High rate when it matters, low rate to save wireless battery, and you never think about it.

It was built for the Glorious Model D2 Pro 4K, and it is designed to be as light as a tool like this can be. No Electron, no background service, and a measured 0.01 percent of a single CPU core. The [Footprint](#footprint) section shows how that was measured rather than asking you to take it on faith.

## Why this exists

Glorious CORE lets you set a polling rate, but it is a single global value and the app itself is heavy. A high polling rate is great in game and wasteful everywhere else, because it drains the wireless battery faster and gives you nothing on the desktop. This tool flips the rate for you.

## How it decides

One rule, and that is the whole thing:

> If any program on your list is running, use the **active** rate. Otherwise use the **inactive** rate.

Running, not focused. Alt tab to Discord or your browser mid match and the rate stays high, because the game is still open. It drops back when you actually close the game.

## Features

- Two rates, picked from the tray menu, applied instantly and remembered.
- A plain text list of programs. One `.exe` per line, nothing else to learn.
- Starts with Windows by default, with a tray toggle to turn it off.
- Pause switching from the tray at any time.
- Optional notification each time the rate changes.
- Portable. One executable and two small files beside it.

## Footprint

Measured on the release build, sampled every minute, on a desktop with about 190 processes running and a 12 core CPU:

| Metric | Value |
| --- | --- |
| Executable size | 0.50 MB |
| Private memory | about 1.7 MB |
| Working set | 0.9 MB to 2.1 MB |
| CPU | 31 ms over 300 s, which is **0.01 percent of one core** |

### About that scan

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

About 130 times cheaper than the snapshot walk. So if you came here wondering whether a background tool polling every two seconds could cost you a frame, the answer is that it does roughly 27 microseconds of work on one core while your game has the other eleven.

Two details in there are load bearing. The cache stores the resolved **name**, never a match result, because caching the result would bake in whichever watch list was current and keep answering with it after you edit the list. And it reconciles against the live id list on every tick, because Windows reuses process ids, and a stale entry would otherwise answer for a completely different process.

If you want it cheaper still, `poll_interval_ms` in `settings.toml` sets the interval. There is not much left to save, and the tradeoff is that a game takes longer to notice.

A note on how these were measured: Windows accounts process CPU time in steps of about 15.6 ms, which is far coarser than a single scan. The per scan figures above therefore come from timing the shipping function directly over hundreds of calls, not from dividing a process CPU total.

## Requirements

- Windows 10 or 11, 64 bit.
- A Glorious Model D2 Pro 4K, USB id `258A:2036`, which works with nothing to configure. A Model O2 Pro 4K should work after a one line config change, see [Other Glorious mice](#other-glorious-mice). Anything else needs a one time capture.

## How the mouse is driven

There is no public API to set a Glorious polling rate, so this tool speaks to the mouse directly using HID feature reports. The reports for the Model D2 Pro 4K were captured from Glorious CORE, verified against the mouse, and built into the binary, so that mouse works with nothing to capture or paste. Supported rates are 125, 250, 500, 1000, 2000 and 4000 Hz.

The bytes are documented rather than hidden. They are listed in [docs/PROTOCOL.md](docs/PROTOCOL.md) and in `src/config.rs`, and you can override them without rebuilding by dropping a `protocol.toml` next to the executable.

## Other Glorious mice

### Model O2 Pro 4K

The Model O2 Pro 4K is the same mouse as the Model D2 Pro 4K in a different shell, so it very probably works here by changing one number.

Glorious CORE ships its own device table, and the two entries are the same shape:

| | Model D2 Pro 4k/8k | Model O2 Pro 4k/8k |
| --- | --- | --- |
| Receiver, the 4K dongle | `258A:2036` | `258A:2035` |
| Wired | `258A:201C` | `258A:201B` |
| Config HID collection | usage page `0xFFFF`, usage `0x00` | usage page `0xFFFF`, usage `0x00` |
| Separate polling rate support | yes | yes |

Same vendor, adjacent product ids, the same vendor defined collection, and the same feature flags. The `258A:2036` in that table is exactly the id this tool was built against, which is a good sign the rest of the row can be trusted too.

To try it, put a single line in a `protocol.toml` next to the executable:

```toml
pid = 0x2035
```

That is genuinely all of it. Every field you leave out keeps its built in value, so the captured rate commands, the vendor collection and the delivery method all carry over, and only the product id changes.

**This has not been tested on real hardware**, because the mouse was not on hand. If you have one, the honest test is to set the rate from the tray and confirm the mouse really changed, not just that the tooltip did. An issue saying whether it worked would be welcome either way.

### Anything else

For any other Glorious mouse the reports will differ and you need your own capture. It takes about fifteen minutes with Wireshark and [docs/CAPTURE.md](docs/CAPTURE.md) walks through it.

## Download and install

1. Go to the [Releases](https://github.com/amarcinkiewicz/GloriousAutoPollingRate/releases) page and download `GloriousAutoPollingRate.exe`.
2. Put it in a folder you like, for example `C:\Tools\GloriousAutoPollingRate\`.
3. Double click it. A mouse icon appears in the system tray, and it sets itself to start with Windows.
4. Right click the icon, choose Edit program list, and add the games you care about. Save, then choose Reload program list.
5. Pick your active and inactive rates from the same menu.

That is the whole install. There is no installer and nothing runs in the background except the tool itself.

## Everyday use

There are only a few things to know, and none of them take long.

**The rates come from the tray menu.** Right click the tray icon and pick an active rate and an inactive rate. The choice is applied at once and remembered.

<img src="assets/tray-menu.png" alt="The tray menu, showing the active and inactive rate pickers, the program list actions, and the notification and autostart toggles" width="236">

Each rate opens a submenu of everything your mouse supports, with the current choice check marked.

<img src="assets/tray-menu-rates.png" alt="The active rate submenu expanded, listing 125 through 4000 Hz with 4000 Hz check marked" width="369">

**The tray icon tells you which rate is live.** Grey means the inactive rate, so nothing on your list is running. Coloured means the active rate, so something is. It is the fastest way to confirm the tool is doing its job without opening anything.

<img src="assets/tray-icon-states.png" alt="The tray icon in both states, grey for the inactive 500 Hz rate and teal for the active 4000 Hz rate" width="362">

Hovering the icon shows the same thing in words, along with the current rate in Hz.

**A notification fires on each change**, if you leave that turned on in the tray menu. Nothing appears while the rate is holding steady, so on a normal day you see one when a game opens and one when it closes.

<img src="assets/notification.png" alt="A Windows notification reading Polling rate changed, 4000 Hz" width="380">

**The programs come from a plain text file.** Choose Edit program list to open `processlist.cfg` in Notepad. One process name per line, including the `.exe`, matched case insensitively. Lines starting with `#` are ignored. Save, then choose Reload program list.

```
cs2.exe
valorant.exe
overwatch.exe
```

If any of them is running, you get the active rate. Otherwise the inactive rate. There is nothing per program to configure.

A new game is picked up within a couple of seconds of launching, and the rate drops back a couple of seconds after you close it.

## Start automatically with Windows

On by default. The first time it runs it adds itself to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, which needs no administrator rights and no installer.

Turn it off any time with Start with Windows in the tray menu, which removes the entry and leaves nothing behind. The tray menu always shows the real state of the registry entry, so if you disable it elsewhere the checkbox follows.

## Files it creates

All next to the executable, all safe to delete:

| File | What it is |
| --- | --- |
| `processlist.cfg` | Your program list. The only file you normally edit. |
| `settings.toml` | The two rates, the notification switch, and `poll_interval_ms`. The tray menu writes the first three. |
| `protocol.toml` | Optional, absent on a normal install. Only for a mouse other than the one the built in commands came from. Any field you leave out keeps its built in value. |
| `devices.txt` | Only written when you run `--list`. A diagnostic dump, safe to delete. |

## Troubleshooting

- The tray tooltip says the mouse was not found. Make sure the 4K dongle is plugged in, then right click and choose Reload program list. If you use a different Glorious model, set the correct `vid` and `pid` in a `protocol.toml`, see [docs/CAPTURE.md](docs/CAPTURE.md).
- The tooltip says there is no command for a rate. The tray menu only offers rates it has commands for, so this normally means a `protocol.toml` is present and incomplete. Delete it to go back to the built in commands.
- You upgraded from an earlier version and still have a `config.toml`. It is no longer read and can be deleted. Rates now live in the tray menu and programs in `processlist.cfg`.
- You want to see which HID collections the tool can find. Run `GloriousAutoPollingRate.exe --list`. It writes a `devices.txt` next to the executable listing every matching collection with its usage page and report lengths, which is the right thing to paste into an issue. This is a windowed program with no console of its own, so running it from a terminal prints nothing: open `devices.txt`.

## Build from source

You need the Rust toolchain. The self contained GNU toolchain works with no Visual Studio required.

```powershell
rustup default stable-x86_64-pc-windows-gnu
cargo build --release
```

The binary lands in `target\release\GloriousAutoPollingRate.exe`.

## How it works

- The tray icon, menu, and notifications use the Win32 shell APIs directly.
- A `WM_TIMER` every two seconds asks `EnumProcesses` for the current process ids and compares them against a cache of already resolved executable names, so a steady state tick is one syscall and some hash lookups. See [About that scan](#about-that-scan). The rest of the time the process sits blocked in `GetMessageW` and does nothing at all.
- A report is only sent when the target rate actually changes, so a quiet desktop session sends nothing at all after the first one.
- Rates are applied by opening the mouse vendor HID collection and sending a feature report, built in for the Model D2 Pro 4K and overridable with a `protocol.toml`.
- Start with Windows is a single value under the per user `Run` key. There is no second copy of that state in the settings file, so the menu can never disagree with what Windows will actually do.
- On startup the process trims its working set, a one time reclaim of cold pages. Because a tick now touches so little, the pages largely stay reclaimed and it sits near 1 MB rather than growing back.

## Disclaimer

This project is not affiliated with, endorsed by, or supported by Glorious. Glorious, Model D2 Pro and Model O2 Pro are trademarks of their respective owner. The protocol is used through captured HID reports for personal interoperability. Use at your own risk.

## License

MIT. See [LICENSE](LICENSE).
