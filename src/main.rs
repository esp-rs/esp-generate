use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

use clap::Parser;
use env_logger::{Builder, Env};
use esp_metadata::Chip;

mod template_files;
mod tui;

#[derive(Clone, Copy)]
pub struct GeneratorOption {
    name: &'static str,
    display_name: &'static str,
    enables: &'static [&'static str],
    disables: &'static [&'static str],
    chips: &'static [Chip],
}

impl GeneratorOption {
    fn options(&self) -> Vec<String> {
        vec![self.name.to_string()]
    }
}

#[derive(Clone, Copy)]
pub struct GeneratorOptionCategory {
    name: &'static str,
    display_name: &'static str,
    options: &'static [GeneratorOptionItem],
}

impl GeneratorOptionCategory {
    fn options(&self) -> Vec<String> {
        let mut res = Vec::new();
        for option in self.options {
            res.extend(option.options());
        }
        res
    }
}

#[derive(Clone, Copy)]
pub enum GeneratorOptionItem {
    Category(GeneratorOptionCategory),
    Option(GeneratorOption),
}

impl GeneratorOptionItem {
    fn title(&self) -> String {
        match self {
            GeneratorOptionItem::Category(category) => category.display_name.to_string(),
            GeneratorOptionItem::Option(option) => option.display_name.to_string(),
        }
    }

    fn name(&self) -> String {
        match self {
            GeneratorOptionItem::Category(category) => category.name.to_string(),
            GeneratorOptionItem::Option(option) => option.name.to_string(),
        }
    }

    fn options(&self) -> Vec<String> {
        match self {
            GeneratorOptionItem::Category(category) => category.options(),
            GeneratorOptionItem::Option(option) => option.options(),
        }
    }

    fn is_category(&self) -> bool {
        matches!(self, GeneratorOptionItem::Category(_))
    }

    fn chips(&self) -> &'static [Chip] {
        match self {
            GeneratorOptionItem::Category(_) => &[],
            GeneratorOptionItem::Option(option) => option.chips,
        }
    }
}

static OPTIONS: &[GeneratorOptionItem] = &[
    GeneratorOptionItem::Option(GeneratorOption {
        name: "alloc",
        display_name: "Enables allocations via the `esp-alloc` crate.",
        enables: &[],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "wifi",
        display_name: "Enables Wi-Fi via the `esp-wifi` crate. Requires `alloc`.",
        enables: &["alloc"],
        disables: &["ble"],
        chips: &[
            Chip::Esp32,
            Chip::Esp32c2,
            Chip::Esp32c3,
            Chip::Esp32c6,
            Chip::Esp32s2,
            Chip::Esp32s3,
        ],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "ble",
        display_name: "Enables BLE via the `esp-wifi` crate. Requires `alloc`.",
        enables: &["alloc"],
        disables: &["wifi"],
        chips: &[
            Chip::Esp32,
            Chip::Esp32c2,
            Chip::Esp32c3,
            Chip::Esp32c6,
            Chip::Esp32h2,
            Chip::Esp32s3,
        ],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "embassy",
        display_name: "Adds `embassy` framework support.",
        enables: &[],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "probe-rs",
        display_name: "Enables `defmt` and flashes using `probe-rs` instead of `espflash`.",
        enables: &[],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "optional",
        display_name: "Options",
        options: &[
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wokwi",
                display_name: "Adds support for Wokwi simulation using VS Code Wokwi extension.",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "dev-container",
                display_name: "Adds support for VS Code Dev Containers and GitHub Codespaces.",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "ci",
                display_name: "Adds GitHub Actions support with some basics checks.",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
        ],
    }),
];

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the project to generate
    name: String,

    /// Chip to target
    #[arg(short, long)]
    chip: Chip,

    /// Run in headless mode (i.e. do not use the TUI)
    #[arg(long)]
    headless: bool,

    /// Generation options
    #[arg(short, long, help = {
        let mut all_options = Vec::new();
        for option in OPTIONS {
            all_options.extend(option.options());
        }
        format!("Generation options: {}",all_options.join(","))
    })]
    option: Vec<String>,

    /// Directory in which to generate the project
    #[arg(short = 'O', long)]
    output_path: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or(log::LevelFilter::Info.as_str()))
        .format_target(false)
        .init();

    let args = Args::parse();

    let path = &args
        .output_path
        .clone()
        .unwrap_or_else(|| env::current_dir().unwrap());

    if !path.is_dir() {
        log::error!("Output path must be a directory");
        process::exit(-1);
    }

    if path.join(&args.name).exists() {
        log::error!("Directory already exists");
        process::exit(-1);
    }

    // Validate options
    process_options(&args);

    let mut selected = if !args.headless {
        let repository = tui::Repository::new(args.chip, OPTIONS, &args.option);

        // TUI stuff ahead
        let terminal = tui::init_terminal()?;

        // create app and run it
        let selected = tui::App::new(repository).run(terminal)?;

        tui::restore_terminal()?;
        // done with the TUI

        if let Some(selected) = selected {
            selected
        } else {
            process::exit(-1);
        }
    } else {
        args.option.clone()
    };

    selected.push(if args.chip.is_riscv() {
        "riscv".to_string()
    } else {
        "xtensa".to_string()
    });

    let mut variables = vec![
        ("project-name".to_string(), args.name.clone()),
        ("mcu".to_string(), args.chip.to_string()),
    ];

    variables.push(("rust_target".to_string(), args.chip.target().to_string()));

    let project_dir = path.join(&args.name);
    fs::create_dir(&project_dir)?;

    for &(file_path, contents) in template_files::TEMPLATE_FILES.iter() {
        if let Some(processed) = process_file(contents, &selected, &variables) {
            let file_path = project_dir.join(file_path);

            fs::create_dir_all(file_path.parent().unwrap())?;
            fs::write(file_path, processed)?;
        }
    }

    // Run cargo fmt:
    Command::new("cargo")
        .args([
            "fmt",
            "--",
            "--config",
            "group_imports=StdExternalCrate",
            "--config",
            "imports_granularity=Module",
        ])
        .current_dir(&project_dir)
        .output()?;

    if should_initialize_git_repo(&project_dir) {
        // Run git init:
        Command::new("git")
            .arg("init")
            .current_dir(&project_dir)
            .output()?;
    } else {
        log::warn!("Current directory is already in a git repository, skipping git initialization");
    }

    Ok(())
}

fn process_file(
    contents: &str,                 // Raw content of the file
    options: &[String],             // Selected options
    variables: &[(String, String)], // Variables and their values in tuples
) -> Option<String> {
    let mut res = String::new();

    let mut replace = None;
    let mut replacement = None;
    let mut include = vec![true];
    let mut first_line = true;

    // Create a new Rhai engine and scope
    let mut engine = rhai::Engine::new();
    let mut scope = rhai::Scope::new();
    scope.push("options", options.to_vec());

    // Define a custom function to check if conditions of the options.
    let options_clone: Vec<String> = options.iter().map(|v| v.to_owned()).collect();
    engine.register_fn("option", move |cond: &str| -> bool {
        let cond = cond.to_string();
        options_clone.contains(&cond)
    });

    for line in contents.lines() {
        let trimmed: &str = line.trim();

        // We check for the first line to see if we should include the file
        if first_line {
            // Determine if the line starts with a known include directive
            let cond = trimmed
                .strip_prefix("//INCLUDEFILE ")
                .or_else(|| trimmed.strip_prefix("#INCLUDEFILE "));

            if let Some(cond) = cond {
                let include_file = if let Some(stripped) = cond.strip_prefix("!") {
                    !options.contains(&stripped.to_string())
                } else {
                    options.contains(&cond.to_string())
                };

                if !include_file {
                    return None;
                } else {
                    continue;
                }
            }
        }
        first_line = false;

        // that's a bad workaround
        if trimmed == "#[rustfmt::skip]" {
            log::info!("Skipping rustfmt");
            continue;
        }

        // Check if we should replace the next line with the key/value of a variable
        if let Some(what) = trimmed
            .strip_prefix("#REPLACE ")
            .or_else(|| trimmed.strip_prefix("//REPLACE "))
        {
            let mut split = what.split_terminator(" ");
            replace = Some(split.next().unwrap().to_string());
            let var = split.next().unwrap().to_string();

            // Find the replacement value from the variables map
            if let Some((_, value)) = variables.iter().find(|(key, _)| key == &var) {
                replacement = Some(value);
            }
        // Check if we should include the next line(s)
        } else if trimmed.starts_with("#IF ") || trimmed.starts_with("//IF ") {
            let cond = if trimmed.starts_with("#IF ") {
                trimmed.strip_prefix("#IF ").unwrap()
            } else {
                trimmed.strip_prefix("//IF ").unwrap()
            };
            let res = engine.eval::<bool>(cond).unwrap();
            include.push(res && *include.last().unwrap());
        } else if trimmed.starts_with("#ELSE") || trimmed.starts_with("//ELSE") {
            let res = !*include.last().unwrap();
            include.pop();
            include.push(res);
        } else if trimmed.starts_with("#ENDIF") || trimmed.starts_with("//ENDIF") {
            include.pop();
        // Trim #+ and //+
        } else if include.iter().all(|v| *v) {
            let mut line = line.to_string();

            if trimmed.starts_with("#+") {
                line = line.replace("#+", "").to_string();
            }

            if trimmed.starts_with("//+") {
                line = line.replace("//+", "").to_string();
            }

            if let (Some(replace), Some(replacement)) = (replace, replacement) {
                line = line.replace(&replace, replacement);
            }

            res.push_str(&line);
            res.push('\n');

            replace = None;
            replacement = None;
        }
    }

    Some(res)
}

fn process_options(args: &Args) {
    for option in &args.option {
        // Find the matching option in OPTIONS
        if let Some(option_item) = OPTIONS.iter().find(|item| item.name() == *option) {
            // Check if the chip is supported. If the chip list is empty,
            // all chips are supported:
            if !option_item.chips().iter().any(|chip| chip == &args.chip)
                && !option_item.chips().is_empty()
            {
                log::error!(
                    "Option '{}' is not supported for chip {}",
                    option,
                    args.chip
                );
                process::exit(-1);
            }
        }
    }

    if args.option.contains(&String::from("ble")) && args.option.contains(&String::from("wifi")) {
        log::error!("Options 'ble' and 'wifi' are mutually exclusive");
        process::exit(-1);
    }
}

fn should_initialize_git_repo(mut path: &Path) -> bool {
    loop {
        let dotgit_path = path.join(".git");
        if dotgit_path.exists() && dotgit_path.is_dir() {
            return false;
        }

        if let Some(parent) = path.parent() {
            path = parent;
        } else {
            break;
        }
    }

    true
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_nested_if_else1() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &["opt1".to_string(), "opt2".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(
            r#"
        opt1
        opt2
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else2() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &[],
            &[],
        )
        .unwrap();

        assert_eq!(
            r#"
        !opt1
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else3() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &["opt1".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(
            r#"
        opt1
        !opt2
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else4() {
        let res = process_file(
            r#"
        #IF option("opt1")
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        opt1
        #ENDIF
        "#,
            &["opt1".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(
            r#"
        !opt2
        opt1
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else5() {
        let res = process_file(
            r#"
        #IF option("opt1")
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        opt1
        #ENDIF
        "#,
            &["opt2".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(
            r#"
        "#
            .trim(),
            res.trim()
        );
    }
}
