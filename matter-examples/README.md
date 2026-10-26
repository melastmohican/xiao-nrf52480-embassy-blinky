# Matter over Thread examples (Seeed Studio XIAO nRF52840)

Ported from the Adafruit Feather nRF52840 `matter-examples` crate. A separate crate from the blinky in the repo root because
the Matter stack needs a different dependency set (`embassy-nrf` 0.11, an allocator, `rs-matter-embassy` with pinned
`[patch.crates-io]` revisions).

- `matter_thread_coex_light`: BLE + Thread concurrent mode. Use this one, it pairs.
- `matter_thread_light`: non-concurrent mode. Experimental, phone pairing does not work (see the header of the file).

Both expose a Matter On/Off light on the built-in red LED (P0.26, active low).

## Prerequisites

`cmake`, `clang` with `libclang`, the `thumbv7em-none-eabihf` Rust target, `probe-rs` >= 0.32.0. The board keeps the
MBR + S140 v7.3.0 SoftDevice and the bootloader; `memory.x` links the app at `0x27000..0xED000`.

## Device identity

`TEST_DEV_DET` from `rs-matter` has the same serial number (`123456789`) on every board, and Home Assistant merges nodes
that report the same serial into one device. So `basic_info()` in each example takes the serial number and unique ID from
the chip's `FICR` `DEVICEID` (16 hex characters). That is also the USB serial number the bootloader reports, and you can
read it through the probe with `probe-rs read b32 0x10000060 2 --chip nRF52840_xxAA` (DEVICEID[0] is the low word).

The bootloader reports "XIAO nRF52840 Sense" for the plain and the Sense variant alike, so that string does not tell
which board you have.

## Run

```text
cd matter-examples
cargo run --release --example matter_thread_coex_light
```

The log prints `SetupQRCode` and `PairingCode` (random on every boot). Pair with Home Assistant (Matter Server + Thread
Border Router); remove the old node first after every reflash.


Flashing takes about 2 minutes (erase 34 s, program 82 s over the Debug Mate). The log prints `Light on` / `Light off` when
the light is toggled. A `You have requested to use P0.09 and P0.10 pins for NFC` warning at boot is harmless.

## Troubleshooting

- **Pairing stops after `Thread scan complete`, with no `Found Thread network` line.** The scan saw no Thread network, and
  phone commissioners abort on an empty result. Move the board closer to the Border Router (the XIAO has a small ceramic
  chip antenna) and pair again with the QR code from the latest boot.
- **The new board joins an old Home Assistant device.** Remove the old node (Matter Server page) and the device, then pair
  again. Every reset or reflash makes a new node, because nothing is persisted.
