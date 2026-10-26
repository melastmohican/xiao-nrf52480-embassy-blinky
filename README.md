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

### I2C scan (`examples/i2c_scan.rs`)

Probes every 7-bit address on two buses and logs the ones that answer: the external bus on D4/D5, and the onboard IMU
bus (P0.07/P0.27, with the IMU powered through P1.08). With the AHT30 attached you should see `0x38` on `I2C0`. The
IMU bus finds `0x6A` only on the Sense board; on the plain XIAO nRF52840 it finds nothing. If SDA is stuck low at
start, it clocks SCL nine times to free the bus.

```bash
cargo run --release --example i2c_scan
```

### AHT30 sensor (`examples/aht30_i2c.rs`)

Reads temperature and humidity from an AHT30 (I2C address `0x38`) every 2 seconds, using the `aht20-driver` crate.
Wire it like this (a Qwiic/STEMMA QT cable cut open works too):

| AHT30 | XIAO nRF52840 |
|-------|---------------|
| GND   | GND           |
| VCC   | 3V3           |
| SDA   | D4 (P0.04)    |
| SCL   | D5 (P0.05)    |

```bash
cargo run --release --example aht30_i2c
```

```text
Temperature: 23.19 °C, Humidity: 47.93 %
```

### BLE environmental sensor (`examples/ble_peripheral.rs`)

Advertises as `XIAO-AHT30-XXXX` (the last two bytes of the chip's address) and serves the AHT30 readings over the
standard Environmental Sensing service (`0x181A`), so generic BLE apps show them without a custom UUID table:

| Characteristic | UUID     | Format                   |
|----------------|----------|--------------------------|
| Temperature    | `0x2A6E` | sint16, units of 0.01 °C |
| Humidity       | `0x2A6F` | uint16, units of 0.01 %  |

Both support read and notify. While a central is connected the sensor is sampled every 2 seconds and subscribers get the
new values. Wire the AHT30 as above, then flash:

```bash
cargo run --release --example ble_peripheral
```

The sensor is read with a small async driver inside the example (not `aht20-driver`), because that driver's blocking
80 ms wait would stall the BLE host on every sample.

**Testing it without a second board.** Either use a phone app (nRF Connect, LightBlue: connect to `XIAO-AHT30-XXXX` and
enable notifications on the two characteristics), or run the Python client from your Mac. It needs
[uv](https://docs.astral.sh/uv/) and Bluetooth permission for your terminal (macOS asks on first run):

```bash
uv run scripts/xiao_env_client.py
```

It scans for 10 seconds, connects, reads both values and prints notifications for 15 seconds:

```text
Found XIAO-AHT30-22AF (F60F946E-E4CB-BC8E-2530-3E56D5D2EAB1)
Read: 24.06 C, 46.66 %
Notify temperature: 24.06 C
Notify humidity:    46.63 %
```

The board advertises to one central at a time, so close any phone app that is connected first. macOS caches device
names, so a board that previously ran other firmware can keep its old name in some tools; the script prints the name
from the advertisement instead.

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
| D4 (SDA)        | P0_04         | External I2C (AHT30)        |
| D5 (SCL)        | P0_05         | External I2C (AHT30)        |
| IMU I2C (Sense) | P0_07 / P0_27 | Onboard LSM6DS3, powered by P1_08 |

## Hardware Links

- [XIAO nRF52840 Sense](https://www.seeedstudio.com/Seeed-Studio-XIAO-nRF52840-Sense-p-5421.html)
- [Embassy Framework](https://embassy.dev/)
- [embassy-nrf](https://crates.io/crates/embassy-nrf)
