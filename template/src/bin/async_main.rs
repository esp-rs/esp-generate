//INCLUDEFILE embassy src/bin/main.rs
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use esp_hal::clock::CpuClock;
//IF !option("esp32")
use esp_hal::timer::systimer::SystemTimer;
//ENDIF
//IF option("wifi") || option("ble") || option("esp32") || option("ble-trouble")
use esp_hal::timer::timg::TimerGroup;
//ENDIF
//IF option("ble-trouble")
use esp_wifi::EspWifiController;
use static_cell::StaticCell;
//REPLACE CRATE_NAME project-name
//+use CRATE_NAME::ble::{GattServer, advertise, BleController};
//ENDIF

//IF option("defmt")
//IF !option("probe-rs")
//+ use esp_println as _;
//ENDIF
//+ use defmt::info;
//ELIF option("log")
use log::info;
//ENDIF probe-rs

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

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    //REPLACE generate-version generate-version
    // generator version: generate-version

    //IF option("probe-rs")
    //IF option("defmt")
    rtt_target::rtt_init_defmt!();
    //ELIF option("panic-rtt-target")
    rtt_target::rtt_init!();
    //ENDIF
    //ELIF option("log")
    esp_println::logger::init_logger_from_env();
    //ENDIF

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    //IF option("alloc")
    esp_alloc::heap_allocator!(size: 72 * 1024);
    //ENDIF

    //IF !option("esp32")
    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);
    //ELSE
    let timer0 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timer0.timer0);
    //ENDIF

    //IF option("defmt") || option("log")
    info!("Embassy initialized!");
    //ENDIF

    //IF (option("wifi") || option("ble")) && !option("ble-trouble")
    let timer1 = TimerGroup::new(peripherals.TIMG0);
    let _init = esp_wifi::init(
        timer1.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .unwrap();
    //ENDIF
    //IF option("ble-trouble")
    let rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let wifi_init = {
        let timg0 = TimerGroup::new(peripherals.TIMG0);
        static WIFI: StaticCell<EspWifiController<'static>> = StaticCell::new();
        WIFI.init(
            esp_wifi::init(timg0.timer0, rng, peripherals.RADIO_CLK)
                .expect("Failed to initialize WIFI/BLE controller"),
        )
    };
    let controller: BleController = {
        let device = peripherals.BT;
        let transport = esp_wifi::ble::controller::BleConnector::new(wifi_init, device);
        bt_hci::controller::ExternalController::new(transport)
    };
    //ENDIF

    //IF !option("ble-trouble")
    // TODO: Spawn some tasks
    let _ = spawner;

    loop {
        //IF option("defmt") || option("log")
        info!("Hello world!");
        //ENDIF
        Timer::after(Duration::from_secs(1)).await;
    }
    //ENDIF
    //IF option("ble-trouble")
    // find more examples https://github.com/embassy-rs/trouble/tree/main/examples/esp32
    let (server, mut peripheral) = {
        let name = "esp32-ble";
        let appearance = &trouble_host::prelude::appearance::sensor::GENERIC_SENSOR;
        GattServer::start(name, appearance, spawner, controller)
    };

    loop {
        //IF option("defmt") || option("log")
        info!("BLE advertising...");
        //ENDIF
        let adv = advertise("esp32-ble", &mut peripheral, server);
        if let Ok(conn) = adv.await {
            //IF option("defmt") || option("log")
            info!("BLE Connected");
            //ENDIF
            let gatt_task = server.start_task(&conn);
            gatt_task.await.unwrap();
        }
        Timer::after(Duration::from_secs(1)).await;
    }
    //ENDIF

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-beta.0/examples/src/bin
}
