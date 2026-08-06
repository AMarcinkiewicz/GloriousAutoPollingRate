# Capturing your polling rate commands

This tool sends the mouse the exact HID feature report that Glorious CORE would send when you pick a polling rate.

**You do not need this page for a Model D2 Pro 4K.** Those reports are already captured and built into the binary, and [PROTOCOL.md](PROTOCOL.md) documents them. This page is for a different Glorious mouse, whose reports will differ. It takes about fifteen minutes and ends with a `protocol.toml` next to the executable.

You will use Wireshark with USBPcap to watch the USB traffic while you change the rate in Glorious CORE. Then you copy the bytes into the config.

## What you need

- Glorious CORE installed and working with your mouse.
- [Wireshark](https://www.wireshark.org/download.html) for Windows. During installation, make sure the USBPcap component is checked. If Wireshark is already installed without it, rerun the installer and enable USBPcap.
- A reboot after installing USBPcap, so the capture driver loads.

## Step 1: find the device

1. Plug in the 4K dongle.
2. Run `GloriousAutoPollingRate.exe --list`. It writes the results to `devices.txt` next to the executable. This is a windowed program with no console of its own, so running it from a terminal prints nothing: open `devices.txt`.
3. You are looking for a collection with `usage_page 0xffff` and a **non zero** `feature_len`, usually 65. That is the channel the commands travel on. If nothing lists at all, the dongle is not detected.

Expect more than one entry, and expect a decoy. On the Model D2 Pro 4K the real output looks like this, where line 1 also reports `usage_page 0xffff` but has `feature_len 0` and is not the one you want:

```
vid 0x258a pid 0x2036
found 7 HID collection(s):

1. usage_page 0xffff usage 0x0001 feature_len 0 output_len 0
2. usage_page 0x0001 usage 0x0006 feature_len 0 output_len 2
3. usage_page 0x000c usage 0x0001 feature_len 0 output_len 0
4. usage_page 0x0001 usage 0x0080 feature_len 0 output_len 0
5. usage_page 0xffff usage 0x0000 feature_len 65 output_len 0
6. usage_page 0xffa0 usage 0x0001 feature_len 0 output_len 0
7. usage_page 0x0001 usage 0x0002 feature_len 0 output_len 0
```

Line 5 is the one. The tool picks it by requiring a writable report rather than by position, which is why the decoy on line 1 does not win.

## Step 2: start the capture

1. Open Wireshark.
2. In the interface list, double click the USBPcap interface that carries your mouse. There are usually several USBPcapN entries. If you are not sure which one, pick the one that shows traffic when you move the mouse, or just capture on all of them one at a time.
3. Capture is now running and will scroll with USB packets.

Tip: to cut the noise, type this into the Wireshark display filter bar and press Enter. It hides the constant mouse movement reports and shows only the control transfers that carry settings:

```
usb.transfer_type == 0x02
```

`0x02` is the control transfer type, which is what a SET_REPORT uses.

## Step 3: change one rate

1. Leave Wireshark capturing.
2. In Glorious CORE, set the polling rate to one value, for example 1000 Hz. Apply it.
3. Watch for a small burst of control transfer packets to appear.
4. Now change to the next value, for example 2000 Hz, and apply. Another burst appears.
5. Repeat for every rate you care about: 125, 500, 1000, 2000, 4000. Do them one at a time and give each a second so the bursts stay separate and easy to tell apart.
6. Stop the capture with the red square.

## Step 4: read the bytes

For each rate you set:

1. Click the control transfer packet that carries the setting. You are looking for a SET_REPORT, which in the packet details shows:
   - `bmRequestType` of `0x21` (host to device, class, interface)
   - `bRequest` of `0x09` (SET_REPORT)
   - a data payload, usually 64 or 65 bytes long
2. In the packet details pane, expand the leftover or HID data field, right click the data bytes, and choose Copy, then Copy as a Hex Stream, or Copy as Bytes. You want the full data payload for that report.
3. Note which rate you had just set. Keep the rate and its bytes together.

If a single rate change sends more than one report, capture all of them in order. Some devices send a short command report followed by a data report. If in doubt, copy every SET_REPORT that appears in that burst and keep them in order.

## Step 5: write a protocol.toml

The Model D2 Pro 4K commands are built into the binary, so there is normally no protocol file at all. To use different ones, create a `protocol.toml` next to the executable.

Every field in that file is optional and anything you leave out keeps its built in value. That matters more than it sounds: if your mouse only differs by product id, as the Model O2 Pro 4K does, the whole file is one line.

```toml
pid = 0x2035
```

Put each captured payload into the matching entry under `[commands]`. The value is the full report as a list of byte values, including the leading report id byte if the payload has one.

```toml
vid = 0x258A
pid = 0x2036
usage_page = 0xFFFF
usage = 0
method = "feature"
report_length = 0

[commands]
"1000" = [0x05, 0x04, 0x00, 0x01]
```

You can write bytes in hex with `0x` or in plain decimal. The tool pads the report out to the device report length for you, so you only need the meaningful leading bytes, though pasting the entire payload is perfectly fine and safest.

If you do supply a `[commands]` table, it replaces the built in one wholesale rather than merging into it, so list every rate you want. The tray menu only offers rates that have commands, so anything missing from your table simply does not appear.

If your capture showed a report id at the front, keep `method = "feature"`. That is the normal case for these mice.

## Step 6: test it

1. Save `protocol.toml`.
2. Right click the tray icon and choose Reload program list, or restart the tool.
3. Switch to a program you configured, or open the game you listed. The tray tooltip should now show the new rate, and you will get a notification if notifications are on.
4. Confirm the rate really changed. You can open a polling rate checker, or simply feel the difference in game. Glorious CORE may still show its old value, because the tool talks to the mouse directly and does not update the CORE interface.

## If you get stuck

The easiest way to share a capture is to save it from Wireshark as a `.pcapng` file, or to copy the hex stream of each SET_REPORT along with the rate you set. With the rate and the bytes side by side, filling in the protocol file is quick.

A trick worth knowing: you do not have to trust that a rate code means what you think. Count the mouse input packets per second in the capture while moving the mouse, and the real rate falls out of the data. Watch out that USBPcap logs both the request and its completion for every transfer, so filter to completions or you will read exactly double the true rate. That is how the mapping in [PROTOCOL.md](PROTOCOL.md) was pinned down.

Do not send bytes you did not capture from your own device. Random bytes will not set a rate, and there is no reason to trust someone else's capture over your own.
