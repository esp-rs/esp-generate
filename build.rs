fn main() {
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let cwd = cwd.to_str().unwrap();
    let mut files: Vec<(String, String)> = Vec::new();

    for file in walkdir::WalkDir::new("template") {
        let path = file.unwrap();

        if !path.file_type().is_file() {
            continue;
        }

        let path = path.path();

        let relative_path = path
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix(cwd)
            .unwrap()
            .to_string()
            .replace("\\", "/");

        let relative_path = relative_path
            .strip_prefix("/template/")
            .unwrap()
            .to_string();

        if relative_path.starts_with("target/") {
            continue;
        }

        if path.file_name() == Some(std::ffi::OsStr::new("template.yaml")) {
            continue;
        }

        println!("{:?} {}", path, relative_path);
        let content = std::fs::read_to_string(path).unwrap();

        files.push((relative_path, content));
    }

    let mut files_code = Vec::new();
    for (file, content) in files {
        files_code.push(quote::quote! { (#file, #content) })
    }

    let code = quote::quote! {
        pub static TEMPLATE_FILES: &[(&str, &str)] = &[
            #(#files_code),*
        ];
    };

    std::fs::write("src/template_files.rs", code.to_string().as_bytes()).unwrap();
    println!("cargo:rerun-if-changed=template");
}
