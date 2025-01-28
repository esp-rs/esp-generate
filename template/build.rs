fn main() {
    //IF option("probe-rs")
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    //ENDIF
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}
