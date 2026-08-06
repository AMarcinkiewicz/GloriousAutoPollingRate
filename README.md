# Glorious Auto Polling Rate

A tiny Windows tray tool that switches your Glorious mouse polling rate automatically, based on which application you are using. Set a low rate to save battery while you browse or work, and let your games jump to a high rate the moment they open or take focus.

It was built for the Glorious Model D2 Pro 4K, and it is designed to be as light as a tool like this can be. No Electron, no background service, no measurable CPU while idle.

## Why this exists

Glorious CORE lets you set a polling rate, but it is a single global value and the app itself is heavy. A high polling rate is great in game and wasteful everywhere else, because it drains the wireless battery faster and gives you nothing on the desktop. This tool flips the rate for you so you never think about it.

## Features

- Per application polling rates. Give each program its own rate, for example 4000 Hz for your main shooter and 1000 Hz for everything else.
- A global active rate and an inactive rate, if you would rather keep it simple.
- Two switching styles:
  - Focus mode: the app you are actively using decides the rate. Fully event driven, so it uses zero CPU while idle.
  - Running mode: if any listed program is running, the active rate is used. The highest match wins.
- Pause switching, reload config, and flip modes from the tray menu.
- Optional notification each time the rate changes.
- Portable. A single executable and a plain text config file. Nothing is written to the registry.

## Footprint

Measured on the release build:

| Metric | Value |
| --- | --- |
| Executable size | about 0.46 MB |
| Private memory | under 2 MB |
| Working set while idle | under 1 MB after startup trim |
| CPU while idle | 0 percent (it waits on Windows events) |

## Requirements

- Windows 10 or 11, 64 bit.
- A Glorious mouse whose polling rate command bytes you have captured once (see the note below). The defaults target the Model D2 Pro 4K dongle, USB id `258A:2036`.

## Important: the one time capture step

There is no public API to set a Glorious polling rate, so this tool speaks to the mouse directly using HID feature reports. The exact bytes for each rate are specific to your device and firmware, so they are not shipped in the binary. You capture them once and paste them into `config.toml`. This keeps the tool honest: it will never send random bytes to your hardware.

The capture takes about fifteen minutes with Wireshark. Full step by step instructions are in [docs/CAPTURE.md](docs/CAPTURE.md). Until the bytes are filled in, the tool runs and shows its tray icon, but it reports that it has no command for the requested rate.

## Download and install

1. Go to the [Releases](https://github.com/amarcinkiewicz/GloriousAutoPollingRate/releases) page and download `GloriousAutoPollingRate.exe`.
2. Put it in a folder you like, for example `C:\Tools\GloriousAutoPollingRate\`.
3. Double click it. A mouse icon appears in the system tray and a `config.toml` file is created next to the executable.
4. Follow [docs/CAPTURE.md](docs/CAPTURE.md) to capture your rate commands, then paste them into `config.toml`.
5. Right click the tray icon and choose Reload config.

That is the whole install. There is no installer and nothing runs in the background except the tool itself.

## Configuration

The config file lives next to the executable and is plain TOML. Edit it, then choose Reload config from the tray menu. Right click the tray icon and choose Open config file to jump straight to it.

```toml
# Switching strategy: "focus" or "running".
mode = "focus"

# Rate in Hz used when nothing matches. Your battery saving rate.
inactive_rate = 500

# Rate in Hz for listed programs that do not set their own rate.
active_rate = 1000

# How often running mode rescans, in milliseconds. Ignored in focus mode.
poll_interval_ms = 2000

# Show a tray notification when the rate changes.
notifications = true

# One block per program. "exe" is matched case insensitively.
# "rate" is optional and falls back to active_rate.
[[programs]]
exe = "cs2.exe"
rate = 4000

[[programs]]
exe = "valorant.exe"
rate = 2000

[[programs]]
exe = "chrome.exe"   # no rate, so active_rate is used
```

### Focus mode versus running mode

- Focus mode is the default and the lightest. When you switch to a window, the tool looks at that program and picks its rate. If the program is not listed, it uses `inactive_rate`. Great for putting the rate up only while you are actually in the game.
- Running mode ignores focus. If any listed program is running at all, its rate is applied. If several match, the highest rate wins. Useful if you want the high rate for the entire time a game is open, even when you alt tab.

## Start automatically with Windows

The tool does not touch the registry, so add it to startup yourself:

1. Press `Windows` + `R`, type `shell:startup`, and press Enter.
2. Create a shortcut to `GloriousAutoPollingRate.exe` in the folder that opens.

It will now start quietly at login.

## Troubleshooting

- The tray tooltip says the mouse was not found. Make sure the 4K dongle is plugged in, then right click and choose Reload config. If you use a different Glorious model, set the correct `vid` and `pid` in the config.
- The tooltip says there is no captured command for a rate. That rate has no bytes in `config.toml` yet. Capture it, see [docs/CAPTURE.md](docs/CAPTURE.md).
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
- Focus mode installs a single `SetWinEventHook` for foreground changes, so the tool sleeps until Windows tells it the active window changed. That is why idle CPU is zero.
- Running mode uses a low frequency timer to rescan the process list, which costs a fraction of a millisecond every couple of seconds.
- Rates are applied by opening the mouse vendor HID collection and sending a feature report. The report bytes come from your `config.toml`.
- On startup the process trims its working set so the reported memory stays tiny.

## Disclaimer

This project is not affiliated with, endorsed by, or supported by Glorious. Glorious and Model D2 Pro are trademarks of their respective owner. The protocol is used through captured HID reports for personal interoperability. Use at your own risk.

## License

MIT. See [LICENSE](LICENSE).
