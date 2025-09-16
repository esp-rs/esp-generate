fn main() {
    linker_be_nice();
    //IF option("embedded-test")
    println!("cargo:rustc-link-arg-tests=-Tembedded-test.x");
    //ENDIF
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    //IF option("defmt")
    // defmt.x can't be the last linker script, or you'll likely get this error:
    // `failed to demangle defmt symbol `__global_pointer$`: expected value at line 1 column 1`
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    //ENDIF
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!("💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`");
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                "esp_wifi_preempt_enable"
                | "esp_wifi_preempt_yield_task"
                | "esp_wifi_preempt_task_create" => {
                    eprintln!();
                    eprintln!("💡 `esp-wifi` has no scheduler enabled. Make sure you have the `builtin-scheduler` feature enabled, or that you provide an external scheduler.");
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!("💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests");
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    //IF option("xtensa")
    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
    //ELIF option("riscv")
    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
    //ENDIF
}
