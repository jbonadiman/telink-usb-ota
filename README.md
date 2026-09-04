# telink-usb-ota

A dependency-free Rust host tool for Telink's USB-HID OTA update protocol —
the one hiding behind the "firmware updater" `.exe` that ships with a lot of
budget mechanical keyboards, mice, and other TC32-based peripherals. It talks
directly to a Linux `hidraw` node; no Windows, no vendor `.exe`, no `hidapi`
or other external crate.

## Why this exists

Telink sells reference MCUs (TLSR8xxx / TC32 core) to a huge number of
peripheral vendors, who rebadge Telink's own `usb_ota_tool` as their own
branded updater without changing the wire protocol underneath it. If you own
one of these devices and the vendor ever stops shipping updates — or never
supported Linux in the first place — this is the same protocol, reimplemented
from scratch by reverse-engineering the vendor `.exe` and confirming every
detail against real hardware.

**Tested against a real device**: a CIDOO V65 v2 mechanical keyboard
(TLSR8278 / B87). Every protocol detail below was recovered by disassembling
that keyboard's vendor updater and its firmware, then live-confirmed against
the real board, including a full successful firmware flash. If you're using
this against a different Telink-based device, the framing is very likely
identical (it's Telink's own tool, not the vendor's), but verify with
`version` (read-only) before ever running `flash` for real.

## What it does

- `version` — read-only. Sends the OTA version-query opcode and prints the
  raw response.
- `flash <firmware.bin> --confirm-flash` — writes an entire image: `START`,
  every 16-byte chunk with a retry loop, then `FINISH`. This is the only
  operation that writes to the device, and it refuses to even open the
  hidraw node without `--confirm-flash`.

```
telink-ota <hidraw-device> version
telink-ota <hidraw-device> flash <firmware.bin> --confirm-flash
```

Add `--verbose` (or `-v`) to see raw report bytes for every exchange, not
just a summary line. `flash` shows a live progress bar with percentage,
transfer rate, and an ETA; colored output follows `NO_COLOR` and disables
itself automatically when stdout isn't a terminal.

## Protocol notes

Report framing (33-byte HID report, report ID 5):

```
byte 0      report ID (0x05)
byte 1      len + 9
byte 2      0x01
byte 3      len + 7
byte 4      len + 3
bytes 5-8   magic 04 52 28 00
bytes 9+    payload
```

Payload for a data chunk (20 bytes): 2-byte little-endian address
(`byte_offset >> 4`), 16 payload bytes, then a CRC-16/MODBUS over those 18
bytes. Control packets (`version`/`start`/`finish`) reuse the address field
as a 16-bit little-endian opcode — `0xFF00`/`0xFF01`/`0xFF02` — which can
never collide with a real chunk address on any image under 2 MB.

A chunk-write ack's success is **not** an echo of the address you sent — it's
the *next expected* chunk index, little-endian, at response bytes `[9, 10]`.
This was the first thing static analysis got wrong and live testing caught;
see `src/packet.rs` for the corrected check.

The device firmware writes every chunk into whichever flash slot isn't
currently active (a dual-slot / A-B layout), and that slot's boot flag is
forced invalid the instant chunk 0 is written, regardless of payload
content. The flag is only re-armed by the `FINISH` command, so an
interrupted or failed `flash` run cannot leave the device pointed at a
half-written image — it just keeps booting the slot it was already on.

## Building

```
cargo build --release
```

Binary at `target/release/telink-ota`. Standard library only — no `hidapi`,
no `clap`, nothing to vendor or audit beyond `rustc` itself.

## Finding your device

```
lsusb -v -d <vid>:<pid> | grep -A5 'HID Report Descriptor'
ls -la /dev/hidraw*
```

The `hidraw` node backing the OTA interface is usually root-owned; either run
as root or add a udev rule granting your user access.

## Testing

```
cargo test
cargo clippy --all-targets
```

All protocol-framing logic (`packet.rs`) and the progress/ETA/color
formatting (`progress.rs`) are pure functions with unit tests — none of them
touch real hardware, so `cargo test` runs anywhere.

## Known limitations

- `flash` has no reset-and-resume logic. If the device drops off the USB bus
  mid-write, it aborts rather than reconnecting.
- No downgrade protection has been confirmed or ruled out on the device side.
  Flash at your own version's risk in either direction.
- There is no firmware read-back / dump path. This tool only writes. If a
  bad write bricks your device, recovery depends entirely on whatever
  hardware debug interface (Swire/SWS on Telink parts) your specific chip
  exposes — which this project does not provide.

## License

AGPL-3.0-only. See `LICENSE`.
