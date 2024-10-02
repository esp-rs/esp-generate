fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
    //IF option("wifi")
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF
    //IF option("ble")
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF
    //IF option("probe-rs")
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    //ENDIF
}
