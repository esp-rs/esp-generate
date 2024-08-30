//INCLUDEFILE !embassy
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    clock::ClockControl, delay::Delay, peripherals::Peripherals, prelude::*, system::SystemControl,
};
//IF probe-rs
//+ use defmt_rtt as _;
//+ use defmt::info;
//ENDIF
//IF !probe-rs
use log::info;
//ENDIF

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

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = SystemControl::new(peripherals.SYSTEM);

    let clocks = ClockControl::max(system.clock_control).freeze();
    let delay = Delay::new(&clocks);

    //IF alloc
    init_heap();
    //ENDIF

    //IF !probe-rs
    esp_println::logger::init_logger_from_env();
    //ENDIF

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

    loop {
        info!("Hello world!");
        delay.delay(500.millis());
    }
}
