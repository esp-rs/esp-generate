fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
    //IF option("probe-rs")
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    //ENDIF
}
