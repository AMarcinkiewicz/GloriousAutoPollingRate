# Glorious Auto Polling Rate

A tiny Windows tray tool that raises your Glorious mouse polling rate while a game is open and drops it back the rest of the time. High rate when it matters, low rate to save wireless battery, and you never think about it.

It was built for the Glorious Model D2 Pro 4K, and it is designed to be as light as a tool like this can be. No Electron, no background service, no measurable CPU while idle.

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

Measured on the release build:

| Metric | Value |
| --- | --- |
| Executable size | about 0.46 MB |
| Private memory | under 2 MB |
| Working set while idle | under 1 MB after startup trim |
| CPU while idle | 0 percent (a process list scan every 2 seconds, too small to register) |

## Requirements

- Windows 10 or 11, 64 bit.
- A Glorious Model D2 Pro 4K, USB id `258A:2036`. Other Glorious mice need a one time capture, see the note below.

## How the mouse is driven

There is no public API to set a Glorious polling rate, so this tool speaks to the mouse directly using HID feature reports. The reports for the Model D2 Pro 4K were captured from Glorious CORE, verified against the mouse, and built into the binary, so that mouse works with nothing to capture or paste. Supported rates are 125, 250, 500, 1000, 2000 and 4000 Hz.

The bytes are documented rather than hidden. They are listed in [docs/PROTOCOL.md](docs/PROTOCOL.md) and in `src/config.rs`, and you can override them without rebuilding by dropping a `protocol.toml` next to the executable.

If you have a different Glorious mouse, the reports will differ and you need your own capture. It takes about fifteen minutes with Wireshark and [docs/CAPTURE.md](docs/CAPTURE.md) walks through it.

## Download and install

1. Go to the [Releases](https://github.com/amarcinkiewicz/GloriousAutoPollingRate/releases) page and download `GloriousAutoPollingRate.exe`.
2. Put it in a folder you like, for example `C:\Tools\GloriousAutoPollingRate\`.
3. Double click it. A mouse icon appears in the system tray, and it sets itself to start with Windows.
4. Right click the icon, choose Edit program list, and add the games you care about. Save, then choose Reload program list.
5. Pick your active and inactive rates from the same menu.

That is the whole install. There is no installer and nothing runs in the background except the tool itself.

## Everyday use

There are only two things to know.

**The rates come from the tray menu.** Right click the tray icon and pick an active rate and an inactive rate. The choice is applied at once and remembered.

<img src="assets/tray-menu.png" alt="The tray menu, showing the active and inactive rate pickers, the program list actions, and the notification and autostart toggles" width="236">

Each rate opens a submenu of everything your mouse supports, with the current choice check marked.

<img src="assets/tray-menu-rates.png" alt="The active rate submenu expanded, listing 125 through 4000 Hz with 4000 Hz check marked" width="369">

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
| `settings.toml` | The two rates. Written by the tray menu. |
| `protocol.toml` | Optional, absent on a normal install. Only for a mouse other than the one the built in commands came from. |

## Troubleshooting

- The tray tooltip says the mouse was not found. Make sure the 4K dongle is plugged in, then right click and choose Reload program list. If you use a different Glorious model, set the correct `vid` and `pid` in a `protocol.toml`, see [docs/CAPTURE.md](docs/CAPTURE.md).
- The tooltip says there is no command for a rate. The tray menu only offers rates it has commands for, so this normally means a `protocol.toml` is present and incomplete. Delete it to go back to the built in commands.
- You upgraded from an earlier version and still have a `config.toml`. It is no longer read and can be deleted. Rates now live in the tray menu and programs in `processlist.cfg`.
- You want to see which HID collections the tool can find. Run `GloriousAutoPollingRate.exe --list` from a terminal. It prints every matching collection with its usage page and report lengths, which is exactly what you need while capturing.

## Build from source

You need the Rust toolchain. The self contained GNU toolchain works with no Visual Studio required.

```powershell
rustup default stable-x86_64-pc-windows-gnu
cargo build --release
```

The binary lands in `target\release\GloriousAutoPollingRate.exe`.

## How it works

- The tray icon, menu, and notifications use the Win32 shell APIs directly.
- A `WM_TIMER` every two seconds walks the process list with `CreateToolhelp32Snapshot` and stops at the first match, so the common case of a listed game being open is cheaper than a full enumeration. The rest of the time the process sits blocked in `GetMessageW`.
- A report is only sent when the target rate actually changes, so a quiet desktop session sends nothing at all after the first one.
- Rates are applied by opening the mouse vendor HID collection and sending a feature report, built in for the Model D2 Pro 4K and overridable with a `protocol.toml`.
- Start with Windows is a single value under the per user `Run` key. There is no second copy of that state in the settings file, so the menu can never disagree with what Windows will actually do.
- On startup the process trims its working set so the reported memory stays tiny.

## Disclaimer

This project is not affiliated with, endorsed by, or supported by Glorious. Glorious and Model D2 Pro are trademarks of their respective owner. The protocol is used through captured HID reports for personal interoperability. Use at your own risk.

## License

MIT. See [LICENSE](LICENSE).
