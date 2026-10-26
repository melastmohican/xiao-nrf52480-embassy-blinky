# xiao-nrf52480-embassy-blinky

Rust async examples for the [Seeed Studio XIAO nRF52840 (Sense) board](https://www.seeedstudio.com/Seeed-Studio-XIAO-nRF52840-Sense-p-5421.html) using the [Embassy](https://embassy.dev/) framework.

This project is built using the official [embassy-nrf](https://crates.io/crates/embassy-nrf) HAL, which provides native Embassy support for the Nordic Semiconductor nRF52840 chip. The time driver is automatically configured using RTC1.

This project was initially generated with https://github.com/lulf/embassy-template

## Project Setup

To build the project, run this command:

```bash
cargo build --release
```

To flash this project, run this command (assuming you are using the XIAO Debug Mate or another SWD/J-Link debugger and `probe-rs`):

```bash
cargo run --release
```

This will automatically invoke `probe-rs run` with the correct `nRF52840_xxAA` chip target.

If `defmt` logging does not appear, ensure that your `DEFMT_LOG` environment variable is set in `.cargo/config.toml`:

```toml
[env]
DEFMT_LOG = "info"
```

## Examples

### Main Application (Blinky)

The main application cycles the built-in RGB LED pins: Red on **P0_26**, Green on **P0_30**, and Blue on **P0_06**.

```bash
cargo run
```

The LEDs toggle sequentially every 1000ms using the async `Timer::after_millis()` function provided by Embassy.

### BLE scan (`examples/ble_scan.rs`)

Scans for nearby BLE advertisers and logs each new device once, with what its advertisement says about it. It uses the
[trouble](https://github.com/embassy-rs/trouble) BLE host on Nordic's SoftDevice Controller (`nrf-sdc` / `nrf-mpsl`),
modelled on the Pico 2 W `ble_scan` example. The BLE crates are `[dev-dependencies]`, so the blinky does not build them.

```bash
cargo run --release --example ble_scan
```

Example output:

```text
77:0f:c3:63:70:a1 (random, beacon) RSSI -49 dBm, name: -, maker: Apple, service: -
4b:6b:9b:63:f5:08 (random, scannable) RSSI -84 dBm, name: -, maker: -, service: 0xfe9f
```

- **Address:** most significant byte first. Phones, watches and earbuds use random addresses that rotate every few
  minutes, so one device can show up as several.
- **Type:** `connectable`, `scannable`, `beacon` (non-connectable), `directed` or `scan-response`.
- **RSSI:** signal strength in dBm; closer to 0 is nearer (about -40 is a few centimetres, -90 is barely audible).
- **Name:** the local name, if the device broadcasts one. Apple devices usually do not, so `-` is normal. A device is
  printed again if its name arrives later in a scan response.
- **Maker:** the manufacturer from the advertisement data, for the common Bluetooth SIG company IDs; `other` otherwise.
- **Service:** the first 16-bit service UUID (for example `0xfe9f` is a Google service), or `-`.

The scan runs on the single-core critical section the rest of the crate uses, and ran for minutes without MPSL asserts.
If that ever misbehaves, enable `nrf-mpsl`'s `critical-section-impl` feature and drop `cortex-m`'s
`critical-section-single-core` for the example.

### Matter over Thread (`matter-examples/`)

A separate crate with a Matter On/Off light over Thread on the built-in red LED, using
[rs-matter-embassy](https://github.com/sysgrok/rs-matter-embassy). It is separate because it needs a different dependency set
(`embassy-nrf` 0.11, an allocator, pinned git patches). See [matter-examples/README.md](matter-examples/README.md).

```bash
cd matter-examples
cargo run --release --example matter_thread_coex_light
```

## Pin Mapping Reference

| Connector / Pin | XIAO nRF52840 | Function |
|-----------------|---------------|----------|
| Red LED         | P0_26         | Digital Output (Active Low) |
| Green LED       | P0_30         | Digital Output (Active Low) |
| Blue LED        | P0_06         | Digital Output (Active Low) |

## Hardware Links

- [XIAO nRF52840 Sense](https://www.seeedstudio.com/Seeed-Studio-XIAO-nRF52840-Sense-p-5421.html)
- [Embassy Framework](https://embassy.dev/)
- [embassy-nrf](https://crates.io/crates/embassy-nrf)
