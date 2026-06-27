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
