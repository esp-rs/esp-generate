//INCLUDEFILE option("embassy") && option("embedded-test")
//INCLUDE_AS tests/hello_test.rs
//! Demo test suite using embedded-test
//!
//! You can run this using `cargo test` as usual.

#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(test)]
#[embedded_test::tests(executor = esp_rtos::embassy::Executor::new())]
mod tests {
    //IF option("defmt")
    use defmt::assert_eq;
    //ENDIF

    #[init]
    fn init() {
        let peripherals = esp_hal::init(esp_hal::Config::default());

        let timg1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
        //IF option("esp32") || option("esp32s2") || option("esp32s3")
        esp_rtos::start(timg1.timer0);
        //ELSE
        let sw_interrupt =
            esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
        esp_rtos::start(timg1.timer0, sw_interrupt.software_interrupt0);
        //ENDIF

        //IF option("defmt")
        rtt_target::rtt_init_defmt!();
        //ENDIF
    }

    #[test]
    async fn hello_test() {
        //IF option("defmt")
        defmt::info!("Running test!");
        //ENDIF

        embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
        assert_eq!(1 + 1, 2);
    }
}
