fn main() {
    linker_be_nice();
    //IF option("embedded-test")
    println!("cargo:rustc-link-arg-tests=-Tembedded-test.x");
    //ENDIF
    //IF option("defmt")
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    //ENDIF
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
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
                "esp_rtos_initialized"
                | "esp_rtos_semaphore_take"
                | "esp_rtos_semaphore_give␍"
                | "esp_rtos_yield_task"
                | "esp_rtos_semaphore_create"
                | "esp_rtos_yield_task_from_isr"
                | "esp_rtos_current_task_thread_semaphore"
                | "esp_rtos_semaphore_delete"
                | "esp_rtos_queue_create"
                | "esp_rtos_queue_try_send_to_back_from_isr"
                | "esp_rtos_queue_send_to_front"
                | "esp_rtos_queue_receive"
                | "esp_rtos_queue_messages_waiting"
                | "esp_rtos_task_create"
                | "esp_rtos_schedule_task_deletion"
                | "esp_rtos_current_task"
                | "esp_rtos_max_task_priority"
                | "esp_rtos_timer_disarm"
                | "esp_rtos_timer_delete"
                | "esp_rtos_timer_create"
                | "esp_rtos_now" => {
                    eprintln!();
                    eprintln!("💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler.");
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!("💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests");
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!("💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?");
                    eprintln!();
                }
                "_defmt_write" | "_defmt_acquire" | "_defmt_release" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `rtt-target` dependency?"
                    );
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
