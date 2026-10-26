//! # BLE Environmental Sensor (GATT peripheral) for the Seeed Studio XIAO nRF52840
//!
//! Advertises as `XIAO-AHT30-XXXX` and serves the AHT30 temperature and humidity readings over the standard
//! Environmental Sensing service (0x181A), so generic apps such as nRF Connect or LightBlue show them without a
//! custom UUID table:
//!
//! | Characteristic | UUID   | Format                          |
//! |----------------|--------|---------------------------------|
//! | Temperature    | 0x2A6E | sint16, units of 0.01 °C        |
//! | Humidity       | 0x2A6F | uint16, units of 0.01 %         |
//!
//! Both support read and notify. While a central is connected the sensor is sampled every 2 seconds and the
//! values are pushed to subscribers. The `XXXX` in the name is the end of the chip's factory address, so several
//! boards can be told apart.
//!
//! ## Hardware
//!
//! Wire the AHT30 as in `aht30_i2c.rs`: SDA to D4 (P0.04), SCL to D5 (P0.05), 3V3 and GND.
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example ble_peripheral
//! ```
//!
//! Then run `uv run scripts/xiao_env_client.py` on a Mac, or open nRF Connect on a phone, connect to `XIAO-AHT30-XXXX`
//! and enable notifications on the two characteristics.
//!
//! The AHT30 is read with a small async driver (a plain I2C transaction, a 80 ms wait, and a CRC check) instead of
//! `aht20-driver`: that driver waits with a blocking delay, which would stall the BLE host for 80 ms every sample.

#![no_std]
#![no_main]

use defmt::{info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::twim::{self, Twim};
use embassy_nrf::{bind_interrupts, peripherals, rng};
use embassy_time::Timer;
use embedded_hal_async::i2c::I2c;
use nrf_mpsl::{MultiprotocolServiceLayer, Peripherals as MpslPeripherals, raw as mpsl_raw};
use nrf_sdc::{self as sdc, Peripherals as SdcPeripherals, SoftdeviceController};
use static_cell::StaticCell;
use trouble_host::prelude::*;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
    EGU0_SWI0 => nrf_mpsl::LowPrioInterruptHandler;
    CLOCK_POWER => nrf_mpsl::ClockInterruptHandler;
    RADIO => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_mpsl::HighPrioInterruptHandler;
});

/// Memory the SoftDevice Controller needs for one advertising set and one connection. If this is off, the
/// controller logs the exact size it needs when it starts.
const SDC_MEM_SIZE: usize = 4672;

const L2CAP_TXQ: u8 = 3;
const L2CAP_RXQ: u8 = 3;

const AHT30_ADDRESS: u8 = 0x38;

#[gatt_service(uuid = "181a")]
struct EnvironmentService {
    /// Temperature in units of 0.01 °C.
    #[characteristic(uuid = "2a6e", read, notify)]
    temperature: i16,
    /// Relative humidity in units of 0.01 %.
    #[characteristic(uuid = "2a6f", read, notify)]
    humidity: u16,
}

#[gatt_server]
struct Server {
    environment: EnvironmentService,
}

#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static MultiprotocolServiceLayer<'static>) -> ! {
    mpsl.run().await
}

/// A temperature and humidity reading, both in hundredths.
struct Reading {
    temperature: i16,
    humidity: u16,
}

/// Minimal async AHT20/AHT30 driver.
struct Aht30<I> {
    i2c: I,
}

impl<I: I2c> Aht30<I> {
    /// Waits for the sensor to power up and calibrates it if needed.
    async fn init(mut i2c: I) -> Result<Self, I::Error> {
        Timer::after_millis(100).await;
        let mut status = [0u8; 1];
        i2c.write_read(AHT30_ADDRESS, &[0x71], &mut status).await?;
        if status[0] & 0x18 != 0x18 {
            i2c.write(AHT30_ADDRESS, &[0xbe, 0x08, 0x00]).await?;
            Timer::after_millis(10).await;
        }
        Ok(Self { i2c })
    }

    /// Triggers a measurement and returns it, or `None` if the sensor was busy or the CRC did not match.
    async fn measure(&mut self) -> Result<Option<Reading>, I::Error> {
        self.i2c.write(AHT30_ADDRESS, &[0xac, 0x33, 0x00]).await?;
        Timer::after_millis(80).await;
        let mut data = [0u8; 7];
        self.i2c.read(AHT30_ADDRESS, &mut data).await?;

        if data[0] & 0x80 != 0 || crc8(&data[..6]) != data[6] {
            return Ok(None);
        }
        let raw_humidity = (u32::from(data[1]) << 12) | (u32::from(data[2]) << 4) | (u32::from(data[3]) >> 4);
        let raw_temperature = ((u32::from(data[3]) & 0x0f) << 16) | (u32::from(data[4]) << 8) | u32::from(data[5]);
        // The raw values are 20 bits wide: humidity = raw / 2^20 * 100 %, temperature = raw / 2^20 * 200 - 50 °C.
        Ok(Some(Reading {
            humidity: ((u64::from(raw_humidity) * 10_000) >> 20) as u16,
            temperature: (((u64::from(raw_temperature) * 20_000) >> 20) as i32 - 5_000) as i16,
        }))
    }
}

/// CRC-8 with polynomial 0x31 and initial value 0xFF, as used by the AHT20/AHT30.
fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0xffu8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x31 } else { crc << 1 };
        }
    }
    crc
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("BLE Environmental Sensor starting...");

    // MPSL owns the highest interrupt priorities, so keep the Embassy drivers below them.
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(config);
    interrupt::TWISPI0.set_priority(Priority::P3);

    let mpsl_p = MpslPeripherals::new(p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31);
    let lfclk_cfg = mpsl_raw::mpsl_clock_lfclk_cfg_t {
        source: mpsl_raw::MPSL_CLOCK_LF_SRC_RC as u8,
        rc_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_CTIV as u8,
        rc_temp_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_TEMP_CTIV as u8,
        accuracy_ppm: 500,
        skip_wait_lfclk_started: mpsl_raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
    };
    static MPSL: StaticCell<MultiprotocolServiceLayer> = StaticCell::new();
    let mpsl = MPSL.init(unwrap!(MultiprotocolServiceLayer::new(mpsl_p, Irqs, lfclk_cfg)));
    spawner.spawn(unwrap!(mpsl_task(mpsl)));

    let sdc_p = SdcPeripherals::new(
        p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24, p.PPI_CH25,
        p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
    );
    static RNG: StaticCell<rng::Rng<'static, embassy_nrf::mode::Async>> = StaticCell::new();
    let rng = RNG.init(rng::Rng::new(p.RNG, Irqs));
    static SDC_MEM: StaticCell<sdc::Mem<SDC_MEM_SIZE>> = StaticCell::new();
    let controller: SoftdeviceController<'static> = unwrap!(
        unwrap!(
            unwrap!(
                unwrap!(sdc::Builder::new())
                    .support_adv()
                    .support_peripheral()
                    .peripheral_count(1)
            )
            .buffer_cfg(
                DefaultPacketPool::MTU as u16,
                DefaultPacketPool::MTU as u16,
                L2CAP_TXQ,
                L2CAP_RXQ
            )
        )
        .build(sdc_p, rng, mpsl, SDC_MEM.init(sdc::Mem::new()))
    );

    // --- Sensor ---
    static TX_BUF: StaticCell<[u8; 16]> = StaticCell::new();
    let i2c = Twim::new(
        p.TWISPI0,
        Irqs,
        p.P0_04,
        p.P0_05,
        twim::Config::default(),
        TX_BUF.init([0; 16]),
    );
    let mut sensor = match Aht30::init(i2c).await {
        Ok(sensor) => sensor,
        Err(e) => panic!("AHT30 not found at 0x38: {:?}", defmt::Debug2Format(&e)),
    };
    info!("AHT30 initialized");

    // --- BLE host setup ---
    // A random static address derived from the chip's factory device address (the top two bits must be set).
    let ficr = embassy_nrf::pac::FICR;
    let lo = ficr.deviceaddr(0).read().to_le_bytes();
    let hi = ficr.deviceaddr(1).read().to_le_bytes();
    let address = Address::random([lo[0], lo[1], lo[2], lo[3], hi[0], hi[1] | 0xc0]);
    info!("Our address = {:?}", address);

    // "XIAO-AHT30-" followed by the last two bytes of the printed address, in hex.
    let mut name_buf = *b"XIAO-AHT30-0000";
    for (i, byte) in [lo[1], lo[0]].iter().enumerate() {
        name_buf[11 + 2 * i] = b"0123456789ABCDEF"[(byte >> 4) as usize];
        name_buf[12 + 2 * i] = b"0123456789ABCDEF"[(byte & 0xf) as usize];
    }
    static NAME: StaticCell<[u8; 15]> = StaticCell::new();
    let name: &'static str = core::str::from_utf8(NAME.init(name_buf)).unwrap_or("XIAO-AHT30");
    info!("Device name = {}", name);

    static RESOURCES: StaticCell<HostResources<DefaultPacketPool, 1, 2>> = StaticCell::new();
    let res = RESOURCES.init(HostResources::new());
    let stack = trouble_host::new(controller, res).set_random_address(address).build();
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();

    let server = unwrap!(Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name,
        appearance: &appearance::sensor::GENERIC_SENSOR,
    })));

    let _ = join(
        async {
            loop {
                if let Err(e) = runner.run().await {
                    panic!("BLE runner error: {:?}", defmt::Debug2Format(&e));
                }
            }
        },
        async {
            loop {
                let conn = match advertise(name, &mut peripheral, &server).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        warn!("Advertising failed: {:?}", defmt::Debug2Format(&e));
                        Timer::after_secs(1).await;
                        continue;
                    }
                };
                info!("Connected");
                // Serve GATT requests and push sensor readings until the central disconnects.
                select(handle_events(&conn), publish_readings(&server, &conn, &mut sensor)).await;
                info!("Disconnected");
            }
        },
    )
    .await;
}

/// Starts connectable advertising and waits for a central to connect.
async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut adv_data = [0; 31];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            // The Environmental Sensing service, so scanners can tell what this device offers.
            AdStructure::CompleteServiceUuids16(&[[0x1a, 0x18]]),
        ],
        &mut adv_data[..],
    )?;
    let mut scan_data = [0; 31];
    let scan_len = AdStructure::encode_slice(&[AdStructure::CompleteLocalName(name.as_bytes())], &mut scan_data[..])?;

    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..adv_len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    info!("Advertising as {}", name);
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    Ok(conn)
}

/// Answers GATT requests until the connection drops.
async fn handle_events<P: PacketPool>(conn: &GattConnection<'_, '_, P>) {
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("Disconnect reason: {:?}", reason);
                break;
            }
            GattConnectionEvent::Gatt { event } => {
                // Reads are answered from the values stored in the table; accept sends the response.
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("GATT error: {:?}", defmt::Debug2Format(&e)),
                }
            }
            _ => {}
        }
    }
}

/// Samples the sensor every 2 seconds, stores the values for reads and notifies subscribers.
async fn publish_readings<I: I2c, P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    sensor: &mut Aht30<I>,
) {
    let env = &server.environment;
    loop {
        match sensor.measure().await {
            Ok(Some(reading)) => {
                info!(
                    "Temperature: {}.{=i16:02} °C, Humidity: {}.{=u16:02} %",
                    reading.temperature / 100,
                    (reading.temperature % 100).abs(),
                    reading.humidity / 100,
                    reading.humidity % 100
                );
                // `store = true` writes the value into the table so a later read returns it too.
                if env.temperature.notify(conn, &reading.temperature, true).await.is_err()
                    || env.humidity.notify(conn, &reading.humidity, true).await.is_err()
                {
                    warn!("Notify failed");
                }
            }
            Ok(None) => warn!("AHT30 busy or CRC mismatch, skipping this sample"),
            Err(e) => warn!("AHT30 read failed: {:?}", defmt::Debug2Format(&e)),
        }
        Timer::after_secs(2).await;
    }
}
