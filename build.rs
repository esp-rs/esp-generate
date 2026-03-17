mod template_embed {
    include!("src/template_embed.rs");
}

fn main() {
    let code = template_embed::generate_template_files_code();
    std::fs::write("src/template_files.rs", code.as_bytes()).unwrap();
    println!("cargo:rerun-if-changed=template");
}
