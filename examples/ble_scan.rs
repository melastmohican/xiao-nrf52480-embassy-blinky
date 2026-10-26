//! # BLE Scan Example for the Seeed Studio XIAO nRF52840
//!
//! Scans for BLE advertisers with the nRF52840 radio and logs each new device once, with what its advertisement
//! says about it: address, signal strength (RSSI), advertising type, local name, manufacturer and first service UUID.
//! Many phones and earbuds use a random address that changes every few minutes, so they can show up as new devices.
//!
//! It uses the `trouble` BLE host on top of Nordic's SoftDevice Controller (`nrf-sdc`) and the multiprotocol
//! service layer (`nrf-mpsl`) that schedules the radio. This is the same controller stack the Matter examples use.
//! Nothing from the S140 SoftDevice is enabled: the MBR and the bootloader stay below `0x27000` as before.
//!
//! ## Run
//!
//! ```bash
//! cargo run --release --example ble_scan
//! ```

#![no_std]
#![no_main]

use core::cell::RefCell;

use bt_hci::param::{AddrKind, BdAddr, LeAdvEventKind, LeAdvReportsIter};
use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_nrf::interrupt::Priority;
use embassy_nrf::{bind_interrupts, peripherals, rng};
use embassy_time::{Duration, Timer};
use heapless::Deque;
use nrf_mpsl::{MultiprotocolServiceLayer, Peripherals as MpslPeripherals, raw as mpsl_raw};
use nrf_sdc::{self as sdc, Peripherals as SdcPeripherals, SoftdeviceController};
use static_cell::StaticCell;
use trouble::prelude::*;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
    EGU0_SWI0 => nrf_mpsl::LowPrioInterruptHandler;
    CLOCK_POWER => nrf_mpsl::ClockInterruptHandler;
    RADIO => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_mpsl::HighPrioInterruptHandler;
});

/// Memory the SoftDevice Controller needs for scanning (it logs the needed size at start-up if this is too big or small).
const SDC_MEM_SIZE: usize = 2136;

#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static MultiprotocolServiceLayer<'static>) -> ! {
    mpsl.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("BLE Scan Example starting...");

    // MPSL owns the highest interrupt priorities, so keep the Embassy drivers below them.
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(config);

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
        unwrap!(unwrap!(sdc::Builder::new()).support_scan().support_central().central_count(1))
            .build(sdc_p, rng, mpsl, SDC_MEM.init(sdc::Mem::new()))
    );

    // --- BLE host setup ---
    // A random static address derived from the chip's factory device address (the top two bits must be set).
    let ficr = embassy_nrf::pac::FICR;
    let lo = ficr.deviceaddr(0).read().to_le_bytes();
    let hi = ficr.deviceaddr(1).read().to_le_bytes();
    let address = Address::random([lo[0], lo[1], lo[2], lo[3], hi[0], hi[1] | 0xc0]);
    info!("Our address = {:?}", address);

    static RESOURCES: StaticCell<HostResources<DefaultPacketPool, 1, 1>> = StaticCell::new();
    let res = RESOURCES.init(HostResources::new());

    let stack = trouble::new(controller, res).set_random_address(address).build();
    let mut central = stack.central();
    let mut runner = stack.runner();

    let printer = Printer { seen: RefCell::new(Deque::new()) };
    let mut scanner = Scanner::new(&mut central);

    info!("Starting BLE scan...");

    let _ = join(runner.run_with_handler(&printer), async {
        let config = ScanConfig {
            active: true,
            interval: Duration::from_millis(100),
            window: Duration::from_millis(100),
            ..Default::default()
        };

        // scan() returns a session that must be kept alive
        let _session = unwrap!(scanner.scan(&config).await);

        loop {
            Timer::after(Duration::from_secs(10)).await;
            info!("Running...");
        }
    })
    .await;
}

/// Remembers which devices were already reported, and whether their name was known at that point.
///
/// A device is printed on first sight, and once more when its name shows up: with active scanning the name usually
/// arrives in a separate scan response, after the first advertisement.
struct Printer {
    seen: RefCell<Deque<(BdAddr, bool), 32>>,
}

/// The Bluetooth SIG company identifier of a manufacturer, for the common ones.
fn company_name(id: u16) -> &'static str {
    match id {
        0x0006 => "Microsoft",
        0x004c => "Apple",
        0x0059 => "Nordic",
        0x0075 => "Samsung",
        0x0087 => "Garmin",
        0x009e => "Bose",
        0x00e0 => "Google",
        0x02e5 => "Espressif",
        _ => "other",
    }
}

/// A 16-bit service UUID formatted as `0xXXXX`, without allocating.
#[derive(Default)]
struct ServiceUuid {
    text: [u8; 6],
    set: bool,
}

impl ServiceUuid {
    fn set(&mut self, uuid: Option<u16>) {
        if let Some(uuid) = uuid {
            self.text[0] = b'0';
            self.text[1] = b'x';
            for (i, c) in self.text[2..].iter_mut().enumerate() {
                *c = b"0123456789abcdef"[((uuid >> (12 - 4 * i)) & 0xf) as usize];
            }
            self.set = true;
        }
    }

    fn as_str(&self) -> &str {
        if self.set { core::str::from_utf8(&self.text).unwrap_or("-") } else { "-" }
    }
}

impl EventHandler for Printer {
    fn on_adv_reports(&self, mut it: LeAdvReportsIter<'_>) {
        let mut seen = self.seen.borrow_mut();
        while let Some(Ok(report)) = it.next() {
            // Pull the interesting fields out of the advertising data.
            let mut name = None;
            let mut company = None;
            let mut service = None;
            for ad in AdStructure::decode(report.data).flatten() {
                match ad {
                    AdStructure::CompleteLocalName(n) | AdStructure::ShortenedLocalName(n) => {
                        name = core::str::from_utf8(n).ok();
                    }
                    AdStructure::ManufacturerSpecificData { company_identifier, .. } => {
                        company = Some(company_identifier);
                    }
                    AdStructure::CompleteServiceUuids16(u) | AdStructure::IncompleteServiceUuids16(u) => {
                        service = u.first().map(|b| u16::from_le_bytes(*b));
                    }
                    _ => {}
                }
            }

            let entry = seen.iter_mut().find(|(a, _)| a.raw() == report.addr.raw());
            let first_time = entry.is_none();
            let name_arrived = matches!(&entry, Some((_, false))) && name.is_some();
            if !first_time && !name_arrived {
                continue;
            }
            match entry {
                Some(e) => e.1 = name.is_some(),
                None => {
                    if seen.is_full() {
                        seen.pop_front();
                    }
                    seen.push_back((report.addr, name.is_some())).ok();
                }
            }

            // The first 16-bit service UUID in hex, e.g. "0xfe9f", or "-".
            let mut service_uuid = ServiceUuid::default();
            service_uuid.set(service);

            let kind = match report.event_kind {
                LeAdvEventKind::AdvInd => "connectable",
                LeAdvEventKind::AdvDirectInd => "directed",
                LeAdvEventKind::AdvScanInd => "scannable",
                LeAdvEventKind::AdvNonconnInd => "beacon",
                LeAdvEventKind::ScanRsp => "scan-response",
            };
            let addr_kind = if report.addr_kind == AddrKind::PUBLIC { "public" } else { "random" };
            // `BdAddr` is stored least significant byte first; addresses are written most significant first.
            let a = report.addr.raw();
            info!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ({}, {}) RSSI {} dBm, name: {}, maker: {}, service: {}",
                a[5],
                a[4],
                a[3],
                a[2],
                a[1],
                a[0],
                addr_kind,
                kind,
                report.rssi,
                name.unwrap_or("-"),
                company.map_or("-", company_name),
                service_uuid.as_str(),
            );
        }
    }
}
