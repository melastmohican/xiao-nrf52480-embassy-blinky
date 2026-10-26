//! # AHT30 Temperature/Humidity Sensor Example
//!
//! Reads temperature and humidity from an AHT30 sensor over I2C (TWIM0).
//!
//! ## Hardware
//!
//! - **Board:** Seeed XIAO nRF52840 (Sense)
//! - **Sensor:** AHT30 Temperature & Humidity Sensor (or compatible), I2C address 0x38
//!
//! ## Wiring with Qwiic/STEMMA QT
//!
//! The XIAO has no Qwiic connector, so cut a Qwiic cable or use jumper wires:
//! ```
//!        Sensor -> XIAO nRF52840
//! (black)  GND  -> GND
//! (red)    VCC  -> 3V3
//! (yellow) SCL  -> D5 (P0.05)
//! (blue)   SDA  -> D4 (P0.04)
//! ```
//!
//! The Sense board's onboard IMU uses a separate I2C bus (P0.07/P0.27), so D4/D5 are free.
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example aht30_i2c
//! ```

#![no_std]
#![no_main]

use aht20_driver::{AHT20, SENSOR_ADDRESS};
use defmt::*;
use embassy_executor::Spawner;
use embassy_nrf::twim::{self, Twim};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::{Delay, Timer};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("AHT30 Sensor Example for XIAO nRF52840");

    let p = embassy_nrf::init(Default::default());

    // The XIAO breakout has no I2C pull-ups on D4/D5; the AHT30 breakout provides them.
    let mut tx_buf = [0u8; 16];
    let i2c = Twim::new(p.TWISPI0, Irqs, p.P0_04, p.P0_05, twim::Config::default(), &mut tx_buf);

    let mut delay = Delay;
    let mut aht20_uninit = AHT20::new(i2c, SENSOR_ADDRESS);

    // Calibrate sensor (consumes the uninit struct and returns an initialized one)
    let mut aht = match aht20_uninit.init(&mut delay) {
        Ok(sensor) => {
            info!("AHT30 initialized and calibrated successfully!");
            sensor
        }
        Err(e) => {
            error!("Failed to initialize AHT30: {:?}", defmt::Debug2Format(&e));
            loop {
                Timer::after_secs(60).await;
            }
        }
    };

    info!("Starting measurements...");

    loop {
        match aht.measure(&mut delay) {
            Ok(measurement) => {
                let temp_scaled = (measurement.temperature * 100.0) as i32;
                let hum_scaled = (measurement.humidity * 100.0) as i32;
                info!(
                    "Temperature: {}.{=i32:02} °C, Humidity: {}.{=i32:02} %",
                    temp_scaled / 100,
                    (temp_scaled % 100).abs(),
                    hum_scaled / 100,
                    (hum_scaled % 100).abs()
                );
            }
            Err(e) => error!("Error reading AHT30 sensor: {:?}", defmt::Debug2Format(&e)),
        }

        Timer::after_secs(2).await;
    }
}
