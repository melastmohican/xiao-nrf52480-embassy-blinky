//! A Matter On/Off light over Thread on the Seeed Studio XIAO nRF52840, with BLE commissioning
//! running concurrently with Thread (`EmbassyThreadMatterStack::run_coex`).
//!
//! Based on `examples/nrf/src/bin/light_thread_coex.rs` from `rs-matter-embassy`, with the emulated
//! light replaced by the board's red LED (P0.26, active low).
//!
//! # Before you run it
//!
//! 1. **Board.** An Seeed Studio XIAO nRF52840 on a debug probe that `probe-rs` supports, with the MBR and the S140 v7.3.0
//!    SoftDevice on it (and normally the Seeed/Adafruit-style UF2 bootloader): `memory.x` links the app at `0x27000`, behind them, and ends it at
//!    `0xED000` so it can never overwrite the user-data area or the bootloader. If your board lacks them, restore the stock Seeed
//!    bootloader first (see the README in this directory).
//! 2. **Toolchain.** The prerequisites in the README in this directory: `cmake`, `clang` with `libclang`, the Rust target and `probe-rs`.
//! 3. **Your network.** Home Assistant with the Matter integration (Matter Server) and a Thread Border Router on the same network,
//!    and the Home Assistant app on a phone with Bluetooth on. The phone commissions the device over BLE, so keep it next to the
//!    board.
//! 4. **Remove the old node.** The pairing is not stored: every reset or reflash makes the board a new device with a new code. In
//!    Home Assistant remove this board's previous node (Matter Server page, the node, Remove) before pairing again, otherwise it
//!    stays behind as an unavailable duplicate.
//!
//! # Run it
//!
//! ```text
//! cd matter-examples
//! cargo run --release --example matter_thread_coex_light
//! ```
//!
//! This builds, flashes and runs through `probe-rs`, and streams the device log (RTT) to the terminal. Near the top of the log the
//! device prints `SetupQRCode: [MT:...]` and `PairingCode: [....-...-....]`. Both are random on every boot, so always use the
//! latest ones.
//!
//! # After it starts: pair it and check it
//!
//! 1. In the Home Assistant app: Settings, Devices & services, Add device, Add Matter device, new device. Scan the QR code, or
//!    choose to enter the code manually and type the pairing code. The QR payload is text, so turn it into an image first, for
//!    example `qrencode -t ansiutf8 'MT:...'`, or paste it into any QR generator. Menu names differ a little between app versions.
//! 2. When the app warns that the accessory is uncertified (it uses the Matter test certificates), choose **Add anyway**.
//! 3. Stay near the board with Bluetooth on until the app is done, about a minute. In the log you should see `Role detached ->
//!    child` (it joined Thread) and `Commissioning complete`.
//! 4. In Home Assistant the new device "Matter Accessory" has a light. Toggle it: the red LED (P0.26) follows. If the light shows
//!    as unavailable although the Matter Server still lists the node as online, reload the Matter integration.
//! 5. Ctrl-C stops `probe-rs`, not the board. To pair again after any reset, power-cycle or reflash, remove the device in Home
//!    Assistant first and start over from "Run it".
#![no_std]
#![no_main]
#![recursion_limit = "256"]

use core::mem::MaybeUninit;
use core::pin::pin;
use core::ptr::addr_of_mut;

use embassy_executor::Spawner;
use embassy_nrf::bind_interrupts;
use embassy_nrf::gpio::{Level, Output, OutputDrive};

use embedded_alloc::LlffHeap;

use defmt::{info, unwrap};

use rs_matter_embassy::matter::crypto::{default_crypto, Crypto, Rng};
use rs_matter_embassy::matter::dm::clusters::app::on_off::{self, EffectVariantEnum, OnOffHooks};
use rs_matter_embassy::matter::dm::clusters::basic_info::BasicInfoConfig;
use rs_matter_embassy::matter::dm::clusters::decl::on_off as on_off_cluster;
use rs_matter_embassy::matter::dm::clusters::decl::on_off::Feature;
use rs_matter_embassy::matter::dm::clusters::desc::{self, ClusterHandler as _};
use rs_matter_embassy::matter::dm::devices::test::{
    DAC_PRIVKEY, TEST_DEV_ATT, TEST_DEV_COMM, TEST_DEV_DET,
};
use rs_matter_embassy::matter::dm::devices::DEV_TYPE_ON_OFF_LIGHT;
use rs_matter_embassy::matter::dm::endpoints::ROOT_ENDPOINT_ID;
use rs_matter_embassy::matter::dm::{Async, Cluster, Dataver, EmptyHandler, Endpoint, Node};
use rs_matter_embassy::matter::persist::{DummyKvBlobStore, VENDOR_KEYS_START};
use rs_matter_embassy::matter::utils::cell::RefCell;
use rs_matter_embassy::matter::utils::init::InitMaybeUninit;
use rs_matter_embassy::matter::utils::sync::blocking::Mutex;
use rs_matter_embassy::matter::{clusters, devices, root_endpoint, BasicCommData};
use rs_matter_embassy::stack::rand::reseeding_csprng;
use rs_matter_embassy::wireless::nrf::{
    EguInterruptHandler, LpTimerInterruptHandler, NrfIeee802154Peripherals, NrfMpslPeripherals,
    NrfSdcPeripherals, NrfThreadClockInterruptHandler, NrfThreadHighPrioInterruptHandler,
    NrfThreadLowPrioInterruptHandler, NrfThreadMpslRadioDriver,
};
use rs_matter_embassy::wireless::{EmbassyThread, EmbassyThreadMatterStack};

use panic_rtt_target as _;

use tinyrlibc as _;

macro_rules! mk_static {
    ($t:ty) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.uninit()
    }};
    ($t:ty,$val:expr) => {{
        mk_static!($t).write($val)
    }};
}

bind_interrupts!(struct Irqs {
    // MPSL's low-priority handler and the 802.15.4 notifications share EGU0_SWI0
    EGU0_SWI0 => NrfThreadLowPrioInterruptHandler, EguInterruptHandler;
    // MPSL clock handler
    CLOCK_POWER => NrfThreadClockInterruptHandler;
    // MPSL high-priority handlers
    RADIO => NrfThreadHighPrioInterruptHandler;
    TIMER0 => NrfThreadHighPrioInterruptHandler;
    RTC0 => NrfThreadHighPrioInterruptHandler;
    // 802.15.4 LP timer
    RTC2 => LpTimerInterruptHandler;
});

const BUMP_SIZE: usize = 23000;

#[global_allocator]
static HEAP: LlffHeap = LlffHeap::empty();

/// We need a bigger log ring-buffer or else the device QR code printout is half-lost
const LOG_RINGBUF_SIZE: usize = 2048;

/// The On/Off cluster hooks of our light: the library's `OnOffHandler` owns and persists the
/// cluster state, we only mirror it on the LED.
pub struct OnOffLedHandler {
    led: Mutex<RefCell<Output<'static>>>,
}

impl OnOffLedHandler {
    pub fn new(led: Output<'static>) -> Self {
        Self {
            led: Mutex::new(RefCell::new(led)),
        }
    }
}

impl OnOffHooks for OnOffLedHandler {
    // The On/Off Light device type requires the `LT` (Lighting) feature, which gates the
    // `GlobalSceneControl`/`OnTime`/`OffWaitTime`/`StartUpOnOff` attributes and the
    // `OffWithEffect`/`OnWithRecallGlobalScene`/`OnWithTimedOff` commands. The library
    // `OnOffHandler` implements all of them; we opt in via the cluster metadata.
    const CLUSTER: Cluster<'static> = on_off_cluster::FULL_CLUSTER
        .with_revision(6)
        .with_features(Feature::LIGHTING.bits())
        .with_attrs(rs_matter_embassy::matter::with!(
            required;
            on_off_cluster::AttributeId::OnOff
                | on_off_cluster::AttributeId::GlobalSceneControl
                | on_off_cluster::AttributeId::OnTime
                | on_off_cluster::AttributeId::OffWaitTime
                | on_off_cluster::AttributeId::StartUpOnOff
        ))
        .with_cmds(rs_matter_embassy::matter::with!(
            on_off_cluster::CommandId::Off
                | on_off_cluster::CommandId::On
                | on_off_cluster::CommandId::Toggle
                | on_off_cluster::CommandId::OffWithEffect
                | on_off_cluster::CommandId::OnWithRecallGlobalScene
                | on_off_cluster::CommandId::OnWithTimedOff
        ));

    fn set_on_off(&self, on: bool) {
        info!("Light {}", if on { "on" } else { "off" });
        self.led.lock(|led| {
            let mut led = led.borrow_mut();
            // The XIAO's LEDs are active low
            if on {
                led.set_low();
            } else {
                led.set_high();
            }
        });
    }

    async fn handle_off_with_effect(&self, _effect: EffectVariantEnum) {
        // no effect
    }
}

#[embassy_executor::main]
async fn main(_s: Spawner) {
    // `rs-matter` uses the `x509` crate which (still) needs a few kilos of heap space
    {
        const HEAP_SIZE: usize = 8192;

        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(addr_of_mut!(HEAP_MEM) as usize, HEAP_SIZE) }
    }

    // Necessary `nrf-hal` initialization boilerplate

    rtt_target::rtt_init_defmt!(rtt_target::ChannelMode::NoBlockSkip, LOG_RINGBUF_SIZE);

    info!("Starting...");

    let p = init();

    // The red LED on the XIAO nRF52840 (P0.26, active low)
    let led_red = Output::new(p.P0_26, Level::High, OutputDrive::Standard);

    let trng = embassy_nrf::rng::Rng::new_blocking(p.RNG);

    // Create the crypto provider, using the NRF hardware TRNG as the source of randomness for a reseeding CSPRNG.
    let crypto = default_crypto(reseeding_csprng(trng, 1000).unwrap(), DAC_PRIVKEY);

    let mut weak_rand = crypto.weak_rand().unwrap();

    // Use a random/unique Matter discriminator for this session,
    // in case there are left-overs from our previous registrations in Thread SRP
    let discriminator = (weak_rand.next_u32() & 0xfff) as u16;

    // A random IEEE EUI-64 on every boot, as there is no persistent storage in this example
    let mut ieee_eui64 = [0; 8];
    weak_rand.fill_bytes(&mut ieee_eui64);

    // Allocate the Matter stack.
    // For MCUs, it is best to allocate it statically, so as to avoid program stack blowups (its memory footprint is ~ 35 to 50KB).
    // It is also (currently) a mandatory requirement when the wireless stack variation is used.
    let stack = mk_static!(EmbassyThreadMatterStack<BUMP_SIZE, ()>).init_with(
        EmbassyThreadMatterStack::init(
            basic_info(),
            BasicCommData {
                password: TEST_DEV_COMM.password,
                discriminator,
            },
            &TEST_DEV_ATT,
        ),
    );

    let (mpsl_p, sdc_p, ieee802154_p) = (
        NrfMpslPeripherals::new(p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31),
        NrfSdcPeripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24,
            p.PPI_CH25, p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        ),
        NrfIeee802154Peripherals::new(p.EGU0, p.TIMER2, p.RTC2),
    );

    let thread_driver = NrfThreadMpslRadioDriver::new(
        p.RADIO,
        mpsl_p,
        sdc_p,
        ieee802154_p,
        crypto.rand().unwrap(),
        Irqs,
    );

    // Our "light" on-off cluster, mirrored on the red LED
    let on_off = on_off::OnOffHandler::new_standalone(
        Dataver::new_rand(&mut weak_rand),
        LIGHT_ENDPOINT_ID,
        VENDOR_KEYS_START + 0x10,
        OnOffLedHandler::new(led_red),
    );

    // Chain our endpoint clusters
    let handler = EmptyHandler
        // The Endpoint 0 system clusters that are ours to provide.
        // The stack adds the operational network clusters (Network Commissioning,
        // General Commissioning, General Diagnostics and Wifi/Thread/Ethernet
        // Diagnostics) on top, because only it knows the network driver state.
        // Chain any extra Endpoint 0 clusters of your own the same way.
        .chain(
            |e, _| e == ROOT_ENDPOINT_ID,
            Async(EmbassyThreadMatterStack::<0, ()>::root_handler(
                &(),
                &mut weak_rand,
            )),
        )
        // Our on-off cluster, on Endpoint 1
        .chain(
            |e, c| e == LIGHT_ENDPOINT_ID && c == OnOffLedHandler::CLUSTER.id,
            Async(on_off::HandlerAdaptor(&on_off)),
        )
        // Each Endpoint needs a Descriptor cluster too
        // Just use the one that `rs-matter` provides out of the box
        .chain(
            |e, c| e == LIGHT_ENDPOINT_ID && c == desc::DescHandler::CLUSTER.id,
            Async(desc::DescHandler::new(Dataver::new_rand(&mut weak_rand)).adapt()),
        );

    // Create a KV BLOB store and load any previously saved state of `rs-matter`
    // `SeqMapKvBlobStore` saves to a user-supplied NOR Flash region
    // However, for this demo and for simplicity, we use a dummy KV BLOB store that does nothing
    let mut store = DummyKvBlobStore;
    stack.startup(&crypto, &mut store).await.unwrap();

    let kv = stack.matter().kv(store);

    // Run the Matter stack with our handler
    // Using `pin!` is completely optional, but reduces the size of the final future
    //
    // This step can be repeated in that the stack can be stopped and started multiple times, as needed.
    let matter = pin!(stack.run_coex(
        // The Matter stack needs to instantiate `openthread`
        EmbassyThread::new(
            thread_driver,
            crypto.rand().unwrap(),
            ieee_eui64,
            &kv,
            stack,
            true, // Use a random BLE address
        ),
        // The crypto provider
        &crypto,
        // Our `AsyncHandler` + `AsyncMetadata` impl
        (NODE, handler),
        // The Matter stack needs a blob store to store its state
        &kv,
        // No user future to run
        (),
    ));

    // Run Matter
    unwrap!(matter.await);
}

/// Basic info about our device, with the serial number and unique ID taken from the chip's FICR `DEVICEID`.
/// Both the matter stack as well as our mDNS-to-SRP bridge need this, hence extracted out.
///
/// The test serial number of `TEST_DEV_DET` is the same on every board, and controllers such as Home Assistant
/// merge nodes that report the same serial number into one device, so each board needs its own.
fn basic_info() -> &'static BasicInfoConfig<'static> {
    let ficr = embassy_nrf::pac::FICR;
    let id = ((ficr.deviceid(1).read() as u64) << 32) | ficr.deviceid(0).read() as u64;

    let mut hex = [0u8; 16];
    for (i, b) in hex.iter_mut().enumerate() {
        *b = b"0123456789ABCDEF"[((id >> (60 - 4 * i)) & 0xf) as usize];
    }
    let hex: &'static [u8; 16] = mk_static!([u8; 16], hex);
    let id_str = core::str::from_utf8(hex).unwrap();

    mk_static!(
        BasicInfoConfig<'static>,
        BasicInfoConfig {
            sai: Some(500),
            product_name: "rs-matter xiao nrf52840 coex light",
            serial_no: id_str,
            unique_id: id_str,
            ..TEST_DEV_DET
        }
    )
}

/// Endpoint 0 (the root endpoint) always runs
/// the hidden Matter system clusters, so we pick ID=1
const LIGHT_ENDPOINT_ID: u16 = 1;

/// The Matter Light device Node
const NODE: Node = Node {
    endpoints: &[
        root_endpoint!(thread, thread_diag(mle_counts, mac_counts)),
        Endpoint::new(
            LIGHT_ENDPOINT_ID,
            devices!(DEV_TYPE_ON_OFF_LIGHT),
            clusters!(desc::DescHandler::CLUSTER, OnOffLedHandler::CLUSTER),
        ),
    ],
};

/// Initialize `embassy-nrf` with a configuration MPSL and the 802.15.4 driver
/// can live with.
fn init() -> embassy_nrf::Peripherals {
    let mut config = embassy_nrf::config::Config::default();
    config.hfclk_source = embassy_nrf::config::HfclkSource::ExternalXtal;

    embassy_nrf::init(config)
}
