/// Embeds template files from the `template/` directory into the binary.
///
/// Returns a static slice of tuples containing (relative_path, file_content).
pub fn embed_templates() -> &'static [(&'static str, &'static str)] {
    embed_templates_from_path(std::path::Path::new("template"))
}

/// Embeds template files from a custom directory path.
///
/// Returns a static slice of tuples containing (relative_path, file_content).
pub fn embed_templates_from_path(
    template_path: &std::path::Path,
) -> &'static [(&'static str, &'static str)] {
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let cwd_str = cwd.to_str().unwrap();

    let mut files: Vec<(&'static str, &'static str)> = Vec::new();

    for file in walkdir::WalkDir::new(template_path) {
        let path = file.unwrap();

        if !path.file_type().is_file() {
            continue;
        }

        let path = path.path();

        let canonical_path = path.canonicalize().unwrap();
        let canonical_str = canonical_path.to_str().unwrap();

        let relative_path = match canonical_str.strip_prefix(cwd_str) {
            Some(r) => r.to_string(),
            None => {
                if template_path.is_absolute() {
                    let template_base = template_path.canonicalize().unwrap();
                    let template_base_str = template_base.to_str().unwrap();
                    match canonical_str.strip_prefix(template_base_str) {
                        Some(r) => r.to_string(),
                        None => continue,
                    }
                } else {
                    continue
                }
            }
        }
        .replace("\\", "/");

        let relative_path = if relative_path.starts_with("/template/") {
            relative_path.strip_prefix("/template/").unwrap().to_string()
        } else if relative_path.starts_with('/') {
            relative_path.strip_prefix('/').unwrap().to_string()
        } else {
            relative_path
        };

        if relative_path.starts_with("target/") {
            continue;
        }

        let content = std::fs::read_to_string(path).unwrap();

        // Leak the strings to make them 'static
        let relative_path = Box::leak(relative_path.into_boxed_str());
        let content = Box::leak(content.into_boxed_str());

        files.push((relative_path, content));
    }

    Box::leak(files.into_boxed_slice())
}

pub fn generate_template_files_code() -> String {
    let files = embed_templates();

    let mut files_code = Vec::new();
    for (file, content) in files.iter() {
        files_code.push(quote::quote! { (#file, #content) })
    }

    let code = quote::quote! {
        pub static TEMPLATE_FILES: &[(&str, &str)] = &[
            #(#files_code),*
        ];
    };

    code.to_string()
}
