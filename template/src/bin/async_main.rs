//INCLUDEFILE embassy
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{clock::ClockControl, peripherals::Peripherals, prelude::*, system::SystemControl};
//IF probe-rs
//+ use defmt_rtt as _;
//+ use defmt::info;
//ENDIF
//IF !probe-rs
use log::info;
//ENDIF

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

//IF alloc
extern crate alloc;
use core::mem::MaybeUninit;

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}
//ENDIF

#[main]
async fn main(spawner: Spawner) {
    let peripherals = Peripherals::take();
    let system = SystemControl::new(peripherals.SYSTEM);

    let clocks = ClockControl::max(system.clock_control).freeze();

    //IF alloc
    init_heap();
    //ENDIF

    //IF !probe-rs
    esp_println::logger::init_logger_from_env();
    //ENDIF

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0, &clocks, None);
    let timers = [esp_hal::timer::OneShotTimer::new(timg0.timer0.into())];
    let timers = mk_static!(
        [esp_hal::timer::OneShotTimer<esp_hal::timer::ErasedTimer>; 1],
        timers
    );
    esp_hal_embassy::init(&clocks, timers);
    info!("Embassy initialized!");

    //IF wifi
    #[rustfmt::skip]
    let timer = esp_hal::timer::PeriodicTimer::new(
        //REPLACE esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0 esp_wifi_timer
        esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0.into(),
    );
    let _init = esp_wifi::initialize(
        esp_wifi::EspWifiInitFor::Wifi,
        timer,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
        &clocks,
    )
    .unwrap();
    //ENDIF

    // TODO: Spawn some tasks
    let _ = spawner;

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
    }
}
