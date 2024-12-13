fn main() {
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    //IF option("probe-rs")
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    //ENDIF
}
