//INCLUDEFILE embassy src/bin/main.rs
#![no_std]
#![no_main]

use esp_hal::clock::CpuClock;

//IF option("panic-esp-backtrace")
use esp_backtrace as _;
//ENDIF
//IF option("panic-panic-probe")
//+ use panic_probe as _;
//ENDIF
//IF !option("panic-esp-backtrace") && !option("panic-panic-probe")
//+ #[panic_handler]
//+ fn panic(_: &core::panic::PanicInfo) -> ! {
//+     esp_hal::system::software_reset()
//+ }
//ENDIF

//IF option("log-backend-defmt-rtt")
//+ use defmt_rtt as _;
//ENDIF
//IF option("log-frontend-defmt")
//+ use defmt::info;
//ENDIF
//IF option("log-frontend-log")
use log::info;
//ENDIF

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

//IF option("alloc")
extern crate alloc;
//ENDIF

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    //REPLACE generate-version generate-version
    // generator version: generate-version

    //IF option("log-frontend-log")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    //IF option("alloc")
    esp_alloc::heap_allocator!(72 * 1024);
    //ENDIF

    //IF !option("esp32")
    let systimer = esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);
    //ELSE
    let timer0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timer0.timer0);
    //ENDIF

    //IF option("log-frontend-defmt") || option("log-frontend-log")
    info!("Embassy initialized!");
    //ENDIF
    //IF option("log-backend-esp-println") && !option("log-frontend-defmt") && !option("log-frontend-log")
    //+esp_println::println!("Embassy initialized!");
    //ENDIF

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
        //IF option("log-frontend-defmt") || option("log-frontend-log")
        info!("Hello world!");
        //ENDIF
        //IF option("log-backend-esp-println") && !option("log-frontend-defmt") && !option("log-frontend-log")
        //+esp_println::println!("Hello world!");
        //ENDIF
        Timer::after(Duration::from_secs(1)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
