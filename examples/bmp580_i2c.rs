//! # BMP580 Pressure & Temperature Sensor Example
//!
//! Reads atmospheric pressure and temperature from an Adafruit BMP580 sensor over I2C (TWIM0).
//!
//! ## Hardware
//!
//! - **Board:** Seeed XIAO nRF52840 (Sense)
//! - **Sensor:** Adafruit BMP580 (high-performance barometric pressure sensor), I2C address 0x47
//!
//! ## Wiring with Qwiic/STEMMA QT
//!
//! The XIAO has no Qwiic connector, so cut a Qwiic cable or use jumper wires:
//! ```
//!        BMP580 -> XIAO nRF52840
//! (black)  GND  -> GND
//! (red)    VCC  -> 3V3
//! (yellow) SCL  -> D5 (P0.05)
//! (blue)   SDA  -> D4 (P0.04)
//! ```
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example bmp580_i2c
//! ```
//!
//! ## About BMP580
//!
//! The Bosch BMP580 is a high-accuracy barometric pressure sensor:
//! - Pressure range: 300 to 1250 hPa
//! - Temperature range: -40°C to +85°C
//! - High precision: ±0.5 hPa absolute accuracy
//! - Very low noise and low power consumption

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_nrf::twim::{self, Twim};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::Timer;
use embedded_hal_async::i2c::I2c;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

/// BMP580 I2C address
/// Adafruit breakout defaults to 0x47. Bosch default is 0x47 or 0x46.
const BMP580_ADDR: u8 = 0x47;

// Registers
const REG_CHIP_ID: u8 = 0x01;
const REG_TEMP_DATA_XLSB: u8 = 0x1D;
const REG_OSR_CONFIG: u8 = 0x36;
const REG_ODR_CONFIG: u8 = 0x37;
const REG_CMD: u8 = 0x7E;

// Commands
const CMD_SOFT_RESET: u8 = 0xB6;

// Expected Chip ID
const CHIP_ID_BMP580: u8 = 0x50;

/// BMP580 Driver
struct Bmp580<I> {
    i2c: I,
}

impl<I: I2c> Bmp580<I> {
    /// Create a new BMP580 driver and initialize the sensor
    async fn new(i2c: I) -> Result<Self, I::Error> {
        let mut sensor = Bmp580 { i2c };
        sensor.init().await?;
        Ok(sensor)
    }

    /// Initialize the sensor
    async fn init(&mut self) -> Result<(), I::Error> {
        // Read chip ID to verify connection
        let mut id = [0u8];
        self.i2c.write_read(BMP580_ADDR, &[REG_CHIP_ID], &mut id).await?;

        if id[0] != CHIP_ID_BMP580 {
            warn!(
                "Unexpected Chip ID: 0x{:02x} (expected 0x{:02x})",
                id[0], CHIP_ID_BMP580
            );
            // Continue anyway, the register map may still match
        } else {
            info!("BMP580 detected (Chip ID: 0x{:02x})", id[0]);
        }

        // Soft reset
        self.i2c.write(BMP580_ADDR, &[REG_CMD, CMD_SOFT_RESET]).await?;
        Timer::after_millis(10).await; // Wait for reset to complete

        // Configure oversampling and enable pressure
        // Register OSR_CONFIG (0x36)
        // Bit 6: press_en (1 = enable)
        // Bits 5-3: osr_p (010 = 4x oversampling)
        // Bits 2-0: osr_t (000 = 1x oversampling)
        // 0x40 (en) | 0x10 (4x) | 0x00 (1x) = 0x50
        self.i2c.write(BMP580_ADDR, &[REG_OSR_CONFIG, 0x50]).await?;

        // Configure ODR and power mode
        // Bit 7: deep standby disable (1 = disable)
        // Bits 6-2: ODR (0 = 240 Hz, the default)
        // Bits 1-0: power mode (0x01 = normal mode)
        // 0x81 = deep standby disabled, normal mode, default ODR
        self.i2c.write(BMP580_ADDR, &[REG_ODR_CONFIG, 0x81]).await?;
        Timer::after_millis(10).await;

        Ok(())
    }

    /// Read pressure and temperature data
    /// Returns (pressure_Pa, temperature_millidegrees_C)
    async fn read_data(&mut self) -> Result<(u32, i32), I::Error> {
        let mut buf = [0u8; 6];
        // Burst read 6 bytes starting from TEMP_DATA_XLSB (0x1D)
        // [T_XLSB, T_LSB, T_MSB, P_XLSB, P_LSB, P_MSB]
        self.i2c.write_read(BMP580_ADDR, &[REG_TEMP_DATA_XLSB], &mut buf).await?;

        // Temperature: signed 24-bit, 1/65536 °C per LSB
        let t_raw = (i32::from_le_bytes([buf[0], buf[1], buf[2], 0]) << 8) >> 8;
        let temp_milli_c = ((t_raw as i64 * 1000) / 65536) as i32;

        // Pressure: unsigned 24-bit, 1/64 Pa per LSB
        let p_raw = u32::from_le_bytes([buf[3], buf[4], buf[5], 0]);
        let pressure_pa = p_raw / 64;

        Ok((pressure_pa, temp_milli_c))
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("BMP580 Sensor Example for XIAO nRF52840");

    let p = embassy_nrf::init(Default::default());

    // The XIAO breakout has no I2C pull-ups on D4/D5; the BMP580 breakout provides them.
    let mut tx_buf = [0u8; 16];
    let i2c = Twim::new(p.TWISPI0, Irqs, p.P0_04, p.P0_05, twim::Config::default(), &mut tx_buf);

    let mut sensor = match Bmp580::new(i2c).await {
        Ok(s) => s,
        Err(e) => {
            error!(
                "Failed to initialize BMP580: {:?}. Check wiring and I2C address!",
                defmt::Debug2Format(&e)
            );
            loop {
                Timer::after_secs(60).await;
            }
        }
    };

    info!("BMP580 initialized successfully!");
    info!("Starting measurements...");

    loop {
        match sensor.read_data().await {
            Ok((pressure_pa, temp_milli_c)) => {
                // Pa -> hPa with two decimals, millidegrees -> °C with two decimals
                let hpa_x100 = pressure_pa as i32;
                let temp_x100 = temp_milli_c / 10;
                info!(
                    "Pressure: {}.{=i32:02} hPa, Temperature: {}.{=i32:02} °C",
                    hpa_x100 / 100,
                    (hpa_x100 % 100).abs(),
                    temp_x100 / 100,
                    (temp_x100 % 100).abs()
                );
            }
            Err(e) => error!("Error reading BMP580 sensor: {:?}", defmt::Debug2Format(&e)),
        }

        Timer::after_secs(2).await;
    }
}
