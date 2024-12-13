//INCLUDEFILE embassy
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::prelude::*;
//IF option("probe-rs")
//+ use defmt_rtt as _;
//+ use defmt::info;
//ENDIF
//IF !option("probe-rs")
use log::info;
//ENDIF

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

//IF option("alloc")
extern crate alloc;
//ENDIF

#[main]
async fn main(spawner: Spawner) {
    //REPLACE generate-version generate-version
    // generator version: generate-version

    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    //IF option("alloc")
    esp_alloc::heap_allocator!(72 * 1024);
    //ENDIF

    //IF !option("probe-rs")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    //IF !option("esp32")
    let timer0 = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER)
        .split::<esp_hal::timer::systimer::Target>();
    esp_hal_embassy::init(timer0.alarm0);
    //ELSE
    let timer0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timer0.timer0);
    //ENDIF

    info!("Embassy initialized!");

    //IF option("wifi") || option("ble")
    let timer1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    let _init = esp_wifi::init(
        timer1.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .unwrap();
    //ENDIF

    // TODO: Spawn some tasks
    let _ = spawner;

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.22.0/examples/src/bin

}
