//INCLUDEFILE !embassy
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{delay::Delay, prelude::*};
//IF option("wifi")
use esp_hal::timer::timg::TimerGroup;
//ENDIF
//IF option("ble")
//+ use esp_hal::timer::timg::TimerGroup;
//ENDIF

//IF option("probe-rs")
//+ use defmt_rtt as _;
//+ use defmt::info;
//ELSE
use log::info;
//ENDIF

//IF option("alloc")
extern crate alloc;
//ENDIF

#[entry]
fn main() -> ! {
    //IF !option("probe-rs")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    //IF option("alloc")
    esp_alloc::heap_allocator!(72 * 1024);
    //ENDIF

    //IF option("wifi") || option("ble")
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let _init = esp_wifi::init(
        timg0.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .unwrap();
    //ENDIF

    let delay = Delay::new();
    loop {
        info!("Hello world!");
        delay.delay(500.millis());
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.22.0/examples/src/bin

}
