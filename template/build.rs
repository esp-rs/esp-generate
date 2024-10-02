fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
    //IF contains_option(wifi)
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF
    //IF contains_option(ble)
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF
    //IF contains_option(probe-rs)
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    //ENDIF
}
