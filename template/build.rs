fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
    //IF wifi
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF
    //IF ble
    println!("cargo:rustc-link-arg-bins=-Trom_functions.x");
    //ENDIF    
    //IF probe-rs
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    //ENDIF
}
