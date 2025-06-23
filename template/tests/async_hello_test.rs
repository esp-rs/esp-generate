//INCLUDEFILE option("embassy") && option("embedded-test")
//INCLUDE_AS tests/hello_test.rs
//! Demo test suite using embedded-test
//!
//! You can run this using `cargo test` as usual.

#![no_std]
#![no_main]

#[cfg(test)]
#[embedded_test::tests(executor = esp_hal_embassy::Executor::new())]
mod tests {
    //IF option("defmt")
    use defmt::assert_eq;
    //ENDIF
    //IF !option("esp32")
    use esp_hal::timer::systimer::SystemTimer;
    //ELSE
    //+use esp_hal::timer::timg::TimerGroup;
    //ENDIF

    #[init]
    fn init() {
        let peripherals = esp_hal::init(esp_hal::Config::default());

        //IF !option("esp32")
        let timer0 = SystemTimer::new(peripherals.SYSTIMER);
        esp_hal_embassy::init(timer0.alarm0);
        //ELSE
        //+let timer0 = TimerGroup::new(peripherals.TIMG1);
        //+esp_hal_embassy::init(timer0.timer0);
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
