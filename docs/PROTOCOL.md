# The Glorious Model D2 Pro 4K polling rate protocol

What the tool sends your mouse, written down so nothing is a black box. These
reports were captured from Glorious CORE over USB, then verified against the
mouse. They are built into the binary, in `BUILTIN_COMMANDS` in
[`src/config.rs`](../src/config.rs).

## The channel

| | |
| --- | --- |
| USB id | `258A:2036` |
| Interface | `MI_02` |
| HID usage page | `0xFFFF` (vendor defined) |
| HID usage | `0x0000` |
| Feature report length | 65 bytes |
| Delivered with | `HidD_SetFeature` |

One detail matters when picking the collection to open. The composite device
exposes several vendor collections, and `MI_01&COL05` also reports usage page
`0xFFFF` while enumerating *before* the one you want. It has a feature report
length of 0, so requiring a writable report is what selects the right
collection. That check lives in `Device::open` and is not optional.

## The report

65 bytes: a leading report id of `0x00`, then 64 bytes of payload, zero padded.
The payload for a polling rate change is nine meaningful bytes.

```
00  00 00  02  03  01  0A  01  XX XX  00 00 ... 00
^   ^      ^   ^   ^   ^   ^   ^      ^
|   |      |   |   |   |   |   |      padding to 65 bytes
|   |      |   |   |   |   |   rate code, sent twice
|   |      |   |   |   |   count
|   |      |   |   |   setting id, 0x0A is polling rate
|   |      |   |   bank
|   |      |   payload length following the setting id
|   |      write command
|   header
report id
```

The same envelope carries other settings, with a different id at the `0x0A`
position. Byte 3 is the length of what follows, so a longer setting simply
carries a longer payload. No checksum is involved.

## Rate codes

| Rate | Code | Full payload after the report id |
| --- | --- | --- |
| 125 Hz | `0x08` | `00 00 02 03 01 0A 01 08 08` |
| 250 Hz | `0x04` | `00 00 02 03 01 0A 01 04 04` |
| 500 Hz | `0x02` | `00 00 02 03 01 0A 01 02 02` |
| 1000 Hz | `0x01` | `00 00 02 03 01 0A 01 01 01` |
| 2000 Hz | `0x20` | `00 00 02 03 01 0A 01 20 20` |
| 4000 Hz | `0x40` | `00 00 02 03 01 0A 01 40 40` |

The codes are not an ascending scale, so do not try to compute them. Reading
`0x01` through `0x08` as a halving sequence from 1000 Hz happens to work, but
2000 and 4000 break the pattern, and the gap at `0x10` is unexplained. The 4K
dongle does not offer 8000 Hz.

## How the mapping was established

Sniffing CORE gives you the codes but not their meaning, and CORE sends a burst
of seven reports on every change, only one of which is the rate. The meaning was
settled by measuring instead of guessing: count the mouse's own input reports
per second in the capture after each change, while the mouse is moving. A rate
of exactly 125 reports per second is not something you have to interpret.

One trap is worth repeating, because it produces a clean and completely wrong
answer. USBPcap logs both the request and its completion for every transfer, so
counting raw packets reports exactly double the real rate. Filter to completions
first. In Wireshark terms, for this device:

```
usb.device_address == 7 && usb.transfer_type == 0x01
  && usb.endpoint_address == 0x81 && usb.data_len == 8
```

## Overriding this

Drop a `protocol.toml` next to the executable and it replaces all of the above.
See [CAPTURE.md](CAPTURE.md) for capturing your own on a different mouse.
