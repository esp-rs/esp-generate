//INCLUDEFILE !option("embassy")
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use esp_hal::{
    clock::CpuClock,
    main,
    time::{Duration, Instant},
};
//IF option("wifi") || option("ble-bleps")
use esp_hal::timer::timg::TimerGroup;
//ENDIF
//IF option("ble-bleps")
use esp_wifi::ble::controller::BleConnector;
//ENDIF

//IF option("defmt")
//IF !option("probe-rs")
//+ use esp_println as _;
//ENDIF
//+ use defmt::info;
//ELIF option("log")
use log::info;
//ELIF option("probe-rs") // without defmt
use rtt_target::rprintln;
//ENDIF !defmt

//IF !option("panic-handler")
//+#[panic_handler]
//+fn panic(_: &core::panic::PanicInfo) -> ! {
//+    loop {}
//+}
//ELIF option("esp-backtrace")
use esp_backtrace as _;
//ELIF option("panic-rtt-target")
//+use panic_rtt_target as _;
//ENDIF

//IF option("alloc")
extern crate alloc;
//ENDIF

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    //REPLACE generate-version generate-version
    // generator version: generate-version

    //IF option("probe-rs")
    //IF option("defmt")
    rtt_target::rtt_init_defmt!();
    //ELSE
    rtt_target::rtt_init_print!();
    //ENDIF
    //ELIF option("log")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    //IF option("wifi") || option("ble-bleps")
    let peripherals = esp_hal::init(config);
    //ELSE
    //+let _peripherals = esp_hal::init(config);
    //ENDIF

    //IF option("alloc")
    //REPLACE 65536 max-dram2-uninit
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 65536);
    //IF option("wifi") && (option("ble-bleps") || option("ble-trouble"))
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(size: 64 * 1024);
    //ENDIF
    //ENDIF alloc

    //IF option("wifi") || option("ble-bleps")
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let wifi_init = esp_wifi::init(
        timg0.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
    )
    .expect("Failed to initialize Wi-Fi/BLE controller");
    //ENDIF
    //IF option("wifi")
    let (mut _wifi_controller, _interfaces) = esp_wifi::wifi::new(&wifi_init, peripherals.WIFI)
        .expect("Failed to initialize Wi-Fi controller");
    //ENDIF
    //IF option("ble-bleps")
    let _connector = BleConnector::new(&wifi_init, peripherals.BT);
    //ENDIF

    loop {
        //IF option("defmt") || option("log")
        info!("Hello world!");
        //ELIF option("probe-rs") // without defmt
        rprintln!("Hello world!");
        //ENDIF
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_millis(500) {}
    }

    //REPLACE {current-version} esp-hal-version
    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v{current-version}/examples/src/bin
}
