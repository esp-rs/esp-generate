//INCLUDEFILE !embassy tests/hello_test.rs
//! Demo test suite using embedded-test
//!
//! You can run this using `cargo test` as usual.

#![no_std]
#![no_main]

#[cfg(test)]
#[embedded_test::tests]
mod tests {
    //IF option("defmt")
    use defmt::assert_eq;
    //ENDIF
    use esp_hal as _;

    #[init]
    fn init() {
        let _ = esp_hal::init(esp_hal::Config::default());

        //IF option("defmt")
        rtt_target::rtt_init_defmt!();
        //ENDIF
    }

    #[test]
    fn hello_test() {
        //IF option("defmt")
        defmt::info!("Running test!");
        //ENDIF

        assert_eq!(1 + 1, 2);
    }
}
