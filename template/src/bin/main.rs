//INCLUDEFILE !embassy
#![no_std]
#![no_main]

use esp_hal::{
    clock::CpuClock,
    main,
    time::{Duration, Instant},
};
//IF option("wifi") || option("ble")
use esp_hal::timer::timg::TimerGroup;
//ENDIF

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

//IF option("alloc")
extern crate alloc;
//ENDIF

#[main]
fn main() -> ! {
    //REPLACE generate-version generate-version
    // generator version: generate-version

    //IF option("log-frontend-log")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    //IF option("wifi") || option("ble")
    let peripherals = esp_hal::init(config);
    //ELSE
    //+let _peripherals = esp_hal::init(config);
    //ENDIF

    //IF option("alloc")
    esp_alloc::heap_allocator!(72 * 1024);
    //ENDIF

    //IF option("wifi") || option("ble")
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
        //IF option("log-frontend-defmt") || option("log-frontend-log")
        info!("Hello world!");
        //ENDIF
        //IF option("log-backend-esp-println") && !option("log-frontend-defmt") && !option("log-frontend-log")
        //+esp_println::println!("Hello world!");
        //ENDIF
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_millis(500) {}
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/v0.23.1/examples/src/bin
}
