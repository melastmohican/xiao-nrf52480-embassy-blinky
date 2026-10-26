//! # I2C Bus Scanner
//!
//! Scans both I2C buses for connected devices and prints their addresses via RTT.
//!
//! ## Hardware
//!
//! - **Board:** Seeed XIAO nRF52840 (Sense)
//!
//! ## Wiring
//!
//! ### External bus (TWIM0)
//! - **SDA:** D4 (P0.04)
//! - **SCL:** D5 (P0.05)
//!
//! ### Onboard IMU bus (TWIM1, Sense only)
//! - **SDA:** P0.07
//! - **SCL:** P0.27
//! - **IMU power enable:** P1.08 (driven high so the LSM6DS3TR-C powers up)
//!
//! The IMU should show up at 0x6A. On a non-Sense board that scan finds nothing.
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example i2c_scan
//! ```

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::twim::{self, Twim};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::Timer;
use embedded_hal::i2c::I2c;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
    TWISPI1 => twim::InterruptHandler<peripherals::TWISPI1>;
});

/// Probe every 7-bit address with a 1-byte read and log the ones that ACK.
fn scan<I: I2c>(i2c: &mut I, bus: &str) {
    for addr in 0..=127u8 {
        let mut readbuf = [0u8; 1];
        if i2c.read(addr, &mut readbuf).is_ok() {
            info!("{}: Device found at address 0x{=u8:X}", bus, addr);
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut p = embassy_nrf::init(Default::default());

    // --- Scan the external bus ---
    info!("Scanning I2C0 on D4 (SDA) and D5 (SCL)...");

    // Check the line levels first. A device that was reset mid-transfer can hold SDA low.
    let sda_high = Input::new(p.P0_04.reborrow(), Pull::Up).is_high();
    let scl_high = Input::new(p.P0_05.reborrow(), Pull::Up).is_high();
    info!("I2C0 initial state: SDA={}, SCL={}", sda_high, scl_high);

    if !sda_high {
        info!("SDA is stuck low! Attempting 9-pulse SCL clocking to free the bus...");
        let mut scl = Output::new(p.P0_05.reborrow(), Level::High, OutputDrive::Standard0Disconnect1);
        for _ in 0..9 {
            scl.set_low();
            Timer::after_micros(5).await;
            scl.set_high();
            Timer::after_micros(5).await;
        }
    }

    // The sensor breakout provides the pull-ups; the pins are not pulled up internally.
    let mut tx_buf0 = [0u8; 16];
    let mut i2c0 = Twim::new(p.TWISPI0, Irqs, p.P0_04, p.P0_05, twim::Config::default(), &mut tx_buf0);
    scan(&mut i2c0, "I2C0");
    info!("Finished scanning I2C0.");

    // --- Scan the onboard IMU bus ---
    info!("Scanning I2C1 (onboard IMU) on P0.07 (SDA) and P0.27 (SCL)...");

    // Power the IMU, then give it time to boot.
    let _imu_power = Output::new(p.P1_08, Level::High, OutputDrive::Standard);
    Timer::after_millis(50).await;

    let mut tx_buf1 = [0u8; 16];
    let mut i2c1 = Twim::new(p.TWISPI1, Irqs, p.P0_07, p.P0_27, twim::Config::default(), &mut tx_buf1);
    scan(&mut i2c1, "I2C1");
    info!("Finished scanning I2C1.");

    info!("Finished all scans.");
    loop {
        Timer::after_secs(60).await;
    }
}
