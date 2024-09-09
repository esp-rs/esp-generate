//INCLUDEFILE !embassy
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{delay::Delay, prelude::*};
//IF wifi
use esp_hal::timer::timg::TimerGroup;
//ENDIF
//IF ble
//+ use esp_hal::timer::timg::TimerGroup;
//ENDIF

//IF probe-rs
//+ use defmt_rtt as _;
//+ use defmt::info;
//ENDIF
//IF !probe-rs
use log::info;
//ENDIF

//IF alloc
extern crate alloc;
//ENDIF

#[entry]
fn main() -> ! {
    //IF !probe-rs
    esp_println::logger::init_logger_from_env();
    //ENDIF

    //IF alloc
    esp_alloc::heap_allocator!(72 * 1024);
    //ENDIF

    //IF wifi
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let _init = esp_wifi::initialize(
        esp_wifi::EspWifiInitFor::Wifi,
        timg0.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .unwrap();
    //ENDIF

    //IF ble
    //+ let peripherals = esp_hal::init(esp_hal::Config::default());
    //+
    //+ let timg0 = TimerGroup::new(peripherals.TIMG0);
    //+ let _init = esp_wifi::initialize(
    //+         esp_wifi::EspWifiInitFor::Ble,
    //+ timg0.timer0,
    //+ esp_hal::rng::Rng::new(peripherals.RNG),
    //+ peripherals.RADIO_CLK,
    //+ )
    //+ .unwrap();
    //ENDIF

    let delay = Delay::new();
    loop {
        info!("Hello world!");
        delay.delay(500.millis());
    }
}
