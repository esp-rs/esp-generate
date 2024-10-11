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

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);
    info!("Embassy initialized!");

    //IF option("wifi") || option("ble")
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let timg1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
    let _init = esp_wifi::init(
        //IF option("wifi")
        esp_wifi::EspWifiInitFor::Wifi,
        //ENDIF
        //IF option("ble")
        //+esp_wifi::EspWifiInitFor::Ble,
        //ENDIF
        timg1.timer0,
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
}
