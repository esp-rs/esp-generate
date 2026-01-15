//INCLUDEFILE option("embassy")
//INCLUDE_AS src/bin/main.rs
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;

//IF option("ble-trouble") || option("ble-bleps")
use esp_radio::ble::controller::BleConnector;
//ENDIF
//IF option("ble-trouble")
use bt_hci::controller::ExternalController;
use trouble_host::prelude::*;
//ENDIF

//IF option("defmt")
//IF !option("probe-rs")
//+use esp_println as _;
//ENDIF
//+use defmt::info;
//ELIF option("log")
use log::info;
//ELIF option("probe-rs") // without defmt
use rtt_target::rprintln;
//ENDIF !defmt

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

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

//IF option("ble-trouble")
const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 1;
//ENDIF

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
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
    let peripherals = esp_hal::init(config);

    //IF option("module-selected")
    // Reserved GPIOs - directly connected to flash/PSRAM
    //REPLACE __RESERVED_GPIO_CODE__ reserved_gpio_code
    __RESERVED_GPIO_CODE__
    //ENDIF

    //IF option("alloc")
    //REPLACE 65536 max-dram2-uninit
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);
    //IF option("wifi") && (option("ble-bleps") || option("ble-trouble"))
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(size: 64 * 1024);
    //ENDIF
    //ENDIF alloc

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    //IF option("esp32") || option("esp32s2") || option("esp32s3")
    esp_rtos::start(timg0.timer0);
    //ELSE
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    //ENDIF

    //IF option("defmt") || option("log")
    info!("Embassy initialized!");
    //ELIF option("probe-rs") // without defmt
    rprintln!("Embassy initialized!");
    //ENDIF

    //IF option("ble-trouble") || option("ble-bleps") || option("wifi")
    let radio_init = esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller");
    //ENDIF
    //IF option("wifi")
    let (mut _wifi_controller, _interfaces) =
        esp_radio::wifi::new(&radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");
    //ENDIF
    //IF option("ble-trouble")
    // find more examples https://github.com/embassy-rs/trouble/tree/main/examples/esp32
    let transport = BleConnector::new(&radio_init, peripherals.BT, Default::default()).unwrap();
    let ble_controller = ExternalController::<_, 1>::new(transport);
    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let _stack = trouble_host::new(ble_controller, &mut resources);
    //ELIF option("ble-bleps")
    let _connector = BleConnector::new(&radio_init, peripherals.BT, Default::default());
    //ENDIF

    // TODO: Spawn some tasks
    let _ = spawner;

    loop {
        //IF option("defmt") || option("log")
        info!("Hello world!");
        //ELIF option("probe-rs") // without defmt
        rprintln!("Hello world!");
        //ENDIF
        Timer::after(Duration::from_secs(1)).await;
    }

    //REPLACE {current-version} esp-hal-version-full
    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v{current-version}/examples
}
