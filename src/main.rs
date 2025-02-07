use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
    time::Duration,
};

use clap::Parser;
use env_logger::{Builder, Env};
use esp_metadata::Chip;
use taplo::formatter::Options;
use update_informer::{registry, Check};

mod check;
mod template_files;
mod tui;

#[derive(Clone, Copy)]
pub struct GeneratorOption {
    name: &'static str,
    display_name: &'static str,
    // A number of options. The inner slice contains alternatives, one of which must be selected.
    enables: &'static [&'static [&'static str]],
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

    fn enables(&self) -> &[&[&str]] {
        match self {
            GeneratorOptionItem::Category(_) => &[&[]],
            GeneratorOptionItem::Option(option) => option.enables,
        }
    }
}

static OPTIONS: &[GeneratorOptionItem] = &[
    GeneratorOptionItem::Option(GeneratorOption {
        name: "alloc",
        display_name: "Enable allocations via the `esp-alloc` crate.",
        enables: &[],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "wifi",
        display_name: "Enable Wi-Fi via the `esp-wifi` crate. Requires `alloc`.",
        enables: &[&["alloc"]],
        disables: &[],
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
        display_name: "Enable BLE via the `esp-wifi` crate. Requires `alloc`.",
        enables: &[&["alloc"]],
        disables: &[],
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
        display_name: "Add `embassy` framework support.",
        enables: &[],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "probe-rs",
        display_name: "Use `probe-rs` instead of `espflash` for flashing and logging.",
        enables: &[&["log-backend-defmt-rtt"]],
        disables: &[],
        chips: &[],
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "logging",
        display_name: "Logging options",
        options: &[
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log-backend-defmt-rtt",
                display_name: "Use `defmt-rtt` as the log backend.",
                enables: &[&["probe-rs"]],
                disables: &["log-backend-esp-println"],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log-backend-esp-println",
                display_name:
                    "Use `esp-println` as the log backend. Can be used without a log frontend.",
                enables: &[],
                disables: &["log-backend-defmt-rtt"],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log-frontend-log",
                display_name:
                    "Use the `log` crate as the logging API. Requires `esp-println` as the backend.",
                enables: &[&["log-backend-esp-println"]],
                disables: &["log-frontend-defmt", "log-backend-defmt-rtt"],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log-frontend-defmt",
                display_name:
                    "Use the `defmt` crate as the logging API. Can be used with both backends.",
                enables: &[&["log-backend-esp-println", "log-backend-defmt-rtt"]],
                disables: &["log-frontend-log"],
                chips: &[],
            }),
        ],
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "panic",
        display_name: "Panic handler options",
        options: &[GeneratorOptionItem::Option(GeneratorOption {
            name: "panic-esp-backtrace",
            display_name: "Use `esp-backtrace`.",
            enables: &[],
            disables: &[],
            chips: &[],
        })],
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "optional",
        display_name: "Options",
        options: &[
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wokwi",
                display_name: "Add support for Wokwi simulation using VS Code Wokwi extension.",
                enables: &[],
                disables: &[],
                chips: &[
                    Chip::Esp32,
                    Chip::Esp32c3,
                    Chip::Esp32c6,
                    Chip::Esp32h2,
                    Chip::Esp32s2,
                    Chip::Esp32s3,
                ],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "dev-container",
                display_name: "Add support for VS Code Dev Containers and GitHub Codespaces.",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "ci",
                display_name: "Add GitHub Actions support with some basic checks.",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
        ],
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "editor",
        display_name: "Optional editor config files for rust-analyzer",
        options: &[
            GeneratorOptionItem::Option(GeneratorOption {
                name: "helix",
                display_name: "Add rust-analyzer settings for Helix Editor",
                enables: &[],
                disables: &[],
                chips: &[],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "vscode",
                display_name: "Add rust-analyzer settings for Visual Studio Code",
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
        format!("Generation options: {} - For more information regarding the different options check the esp-generate README.md (https://github.com/esp-rs/esp-generate/blob/main/README.md).",all_options.join(", "))
    })]
    option: Vec<String>,

    /// Directory in which to generate the project
    #[arg(short = 'O', long)]
    output_path: Option<PathBuf>,

    /// Do not check for updates
    #[arg(short, long, global = true, action)]
    skip_update_check: bool,
}

/// Check crates.io for a new version of the application
fn check_for_update(name: &str, version: &str) {
    // By setting the interval to 0 seconds we invalidate the cache with each
    // invocation and ensure we're getting up-to-date results
    let informer =
        update_informer::new(registry::Crates, name, version).interval(Duration::from_secs(0));

    if let Some(version) = informer.check_version().ok().flatten() {
        log::warn!("🚀 A new version of {name} is available: {version}");
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or(log::LevelFilter::Info.as_str()))
        .format_target(false)
        .init();

    let args = Args::parse();

    // Only check for updates once the command-line arguments have been processed,
    // to avoid printing any update notifications when the help message is
    // displayed.
    if !args.skip_update_check {
        check_for_update(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    }

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

    selected.push(args.chip.to_string());

    selected.push(if args.chip.is_riscv() {
        "riscv".to_string()
    } else {
        "xtensa".to_string()
    });

    let wokwi_devkit = match args.chip {
        Chip::Esp32 => "board-esp32-devkit-c-v4",
        Chip::Esp32c2 => "",
        Chip::Esp32c3 => "board-esp32-c3-devkitm-1",
        Chip::Esp32c6 => "board-esp32-c6-devkitc-1",
        Chip::Esp32h2 => "board-esp32-h2-devkitm-1",
        Chip::Esp32s2 => "board-esp32-s2-devkitm-1",
        Chip::Esp32s3 => "board-esp32-s3-devkitc-1",
    };

    let mut variables = vec![
        ("project-name".to_string(), args.name.clone()),
        ("mcu".to_string(), args.chip.to_string()),
        ("wokwi-board".to_string(), wokwi_devkit.to_string()),
        (
            "generate-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
    ];

    variables.push(("rust_target".to_string(), args.chip.target().to_string()));

    let project_dir = path.join(&args.name);
    fs::create_dir(&project_dir)?;

    for &(file_path, contents) in template_files::TEMPLATE_FILES.iter() {
        let mut file_path = file_path.to_string();
        if let Some(processed) = process_file(contents, &selected, &variables, &mut file_path) {
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

    // Format Cargo.toml:
    let input = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    let format_options = Options {
        align_entries: true,
        reorder_keys: true,
        reorder_arrays: true,
        ..Default::default()
    };
    let formated = taplo::formatter::format(&input, format_options);
    fs::write(project_dir.join("Cargo.toml"), formated)?;

    if should_initialize_git_repo(&project_dir) {
        // Run git init:
        Command::new("git")
            .arg("init")
            .current_dir(&project_dir)
            .output()?;
    } else {
        log::warn!("Current directory is already in a git repository, skipping git initialization");
    }

    check::check(args.chip);

    Ok(())
}

fn process_file(
    contents: &str,                 // Raw content of the file
    options: &[String],             // Selected options
    variables: &[(String, String)], // Variables and their values in tuples
    file_path: &mut String,         // File path to be modified
) -> Option<String> {
    let mut res = String::new();

    let mut replace: Option<Vec<(String, String)>> = None;
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
                if !cond.contains(" ") {
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
                } else {
                    let mut parts = cond.split_whitespace();
                    let include_file = if let Some(stripped) = parts.next() {
                        if let Some(stripped) = stripped.strip_prefix("!") {
                            !options.contains(&stripped.to_string())
                        } else {
                            options.contains(&stripped.to_string())
                        }
                    } else {
                        false
                    };
                    if !include_file {
                        return None;
                    } else {
                        let new_name = parts.next().unwrap();
                        *file_path = new_name.to_string();
                        continue;
                    }
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
            let replacements = what
                .split(" && ")
                .filter_map(|pair| {
                    let mut parts = pair.split_whitespace();
                    if let (Some(pattern), Some(var_name)) = (parts.next(), parts.next()) {
                        if let Some((_, value)) = variables.iter().find(|(key, _)| key == var_name)
                        {
                            Some((pattern.to_string(), value.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            if !replacements.is_empty() {
                replace = Some(replacements);
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
        } else if trimmed.starts_with("#ELIF ") || trimmed.starts_with("//ELIF ") {
            let cond = if trimmed.starts_with("#ELIF ") {
                trimmed.strip_prefix("#ELIF ").unwrap()
            } else {
                trimmed.strip_prefix("//ELIF ").unwrap()
            };
            let res = engine.eval::<bool>(cond).unwrap();
            let last = include.pop();
            include.push(res && !last.unwrap());
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
                line = line.replace("#+", "");
            }

            if trimmed.starts_with("//+") {
                line = line.replace("//+", "");
            }

            if let Some(replacements) = &replace {
                for (pattern, value) in replacements {
                    line = line.replace(pattern, value);
                }
            }

            res.push_str(&line);
            res.push('\n');

            replace = None;
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

            let mut requires = vec![];
            for alternatives in option_item.enables() {
                if !alternatives
                    .iter()
                    .all(|alternative| args.option.contains(&alternative.to_string()))
                {
                    requires.push(if alternatives.len() == 1 {
                        alternatives[0].to_string()
                    } else {
                        format!("one of {{{}}}", alternatives.join(", "))
                    });
                }
            }

            if !requires.is_empty() {
                log::error!(
                    "Option '{}' requires {}",
                    option_item.name(),
                    requires.join(", ")
                );
                process::exit(-1);
            }
        }
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
            &mut String::from("main.rs"),
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
            &mut String::from("main.rs"),
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
            &mut String::from("main.rs"),
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
            &mut String::from("main.rs"),
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
            &mut String::from("main.rs"),
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
