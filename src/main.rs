use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::LazyLock,
};

use clap::Parser;
use env_logger::{Builder, Env};
use esp_generate::config::find_option;
use esp_generate::config::{ActiveConfiguration, Relationships};
use esp_generate::template::{GeneratorOptionItem, Template};
use esp_metadata::Chip;
use taplo::formatter::Options;

mod check;
mod template_files;
mod tui;

static TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    serde_yaml::from_str(
        template_files::TEMPLATE_FILES
            .iter()
            .find_map(|(k, v)| if *k == "template.yaml" { Some(v) } else { None })
            .unwrap(),
    )
    .unwrap()
});

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
        for option in TEMPLATE.options.iter() {
            for opt in option.options() {
                // Remove duplicates, which usually are chip-specific variations of an option.
                // An example of this is probe-rs.
                if !all_options.contains(&opt) {
                    all_options.push(opt);
                }
            }
        }
        format!("Generation options: {} - For more information regarding the different options check the esp-generate README.md (https://github.com/esp-rs/esp-generate/blob/main/README.md).",all_options.join(", "))
    })]
    option: Vec<String>,

    /// Directory in which to generate the project
    #[arg(short = 'O', long)]
    output_path: Option<PathBuf>,

    /// Do not check for updates
    #[arg(short, long, global = true, action)]
    #[cfg(feature = "update-informer")]
    skip_update_check: bool,
}

/// Check crates.io for a new version of the application
#[cfg(feature = "update-informer")]
fn check_for_update(name: &str, version: &str) {
    use std::time::Duration;
    use update_informer::{registry, Check};
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
    #[cfg(feature = "update-informer")]
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

    // Validate options. We pass the unmodified template to the function, so that it can tell
    // the user which options are not supported for the selected chip.
    process_options(&TEMPLATE, &args);

    // Now we filterout the incompatible options, so that they are not shown and they also don't
    // screw with our position-based data model.
    let mut template = TEMPLATE.clone();
    remove_incompatible_chip_options(args.chip, &mut template.options);

    let mut selected = if !args.headless {
        let repository = tui::Repository::new(args.chip, &template.options, &args.option);

        // TUI stuff ahead
        let terminal = tui::init_terminal()?;

        // create app and run it
        let selected = tui::App::new(repository).run(terminal)?;

        tui::restore_terminal()?;
        // done with the TUI

        let Some(selected) = selected else {
            process::exit(0);
        };

        println!(
            "Selected options: --chip {}{}",
            args.chip,
            selected.iter().fold(String::new(), |mut acc, s| {
                use std::fmt::Write;
                write!(&mut acc, " -o {s}").unwrap();
                acc
            })
        );

        selected
    } else {
        args.option.clone()
    };

    // Also add the active selection groups
    for idx in 0..selected.len() {
        let option = find_option(&selected[idx], &template.options).unwrap();
        selected.push(option.selection_group.clone());
    }

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

    check::check(args.chip, selected.contains(&"probe-rs".to_string()));

    Ok(())
}

fn remove_incompatible_chip_options(chip: Chip, options: &mut Vec<GeneratorOptionItem>) {
    options.retain_mut(|opt| match opt {
        GeneratorOptionItem::Category(category) => {
            remove_incompatible_chip_options(chip, &mut category.options);
            !category.options.is_empty()
        }
        GeneratorOptionItem::Option(option) => {
            option.chips.is_empty() || option.chips.contains(&chip)
        }
    });
}

#[derive(Clone, Copy)]
enum BlockKind {
    // All lines are included
    Root,

    // (current branch to be included, any previous branches included)
    IfElse(bool, bool),
}

impl BlockKind {
    fn include_line(self) -> bool {
        match self {
            BlockKind::Root => true,
            BlockKind::IfElse(current, any) => current && !any,
        }
    }

    fn new_if(current: bool) -> BlockKind {
        BlockKind::IfElse(current, false)
    }

    fn into_else_if(self, condition: bool) -> BlockKind {
        let BlockKind::IfElse(previous, any) = self else {
            panic!("ELIF without IF");
        };
        BlockKind::IfElse(condition, any || previous)
    }

    fn into_else(self) -> BlockKind {
        let BlockKind::IfElse(previous, any) = self else {
            panic!("ELSE without IF");
        };
        BlockKind::IfElse(!any, any || previous)
    }
}

fn process_file(
    contents: &str,                 // Raw content of the file
    options: &[String],             // Selected options
    variables: &[(String, String)], // Variables and their values in tuples
    file_path: &mut String,         // File path to be modified
) -> Option<String> {
    let mut res = String::new();

    let mut replace: Option<Vec<(String, String)>> = None;
    let mut include = vec![BlockKind::Root];
    let mut first_line = true;

    // Create a new Rhai engine and scope
    let mut engine = rhai::Engine::new();

    // Define a custom function to check if conditions of the options.
    let options_clone: Vec<String> = options.to_vec();
    engine.register_fn("option", move |cond: &str| -> bool {
        let cond = cond.to_string();
        options_clone.contains(&cond)
    });

    for (line_no, line) in contents.lines().enumerate() {
        let line_no = line_no + 1;
        let trimmed: &str = line.trim();

        // We check for the first line to see if we should include the file
        if first_line {
            // IGNOREFILE can be used to exclude the file from the output.
            if trimmed.starts_with("//IGNOREFILE") || trimmed.starts_with("#IGNOREFILE") {
                return None;
            }

            // Determine if the line starts with a known include directive
            let include = trimmed
                .strip_prefix("//INCLUDEFILE ")
                .or_else(|| trimmed.strip_prefix("#INCLUDEFILE "));

            if let Some(cond) = include {
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
            let last = *include.last().unwrap();

            // Only evaluate condition if this IF is in a branch that should be included
            let current = if last.include_line() {
                engine.eval::<bool>(cond).unwrap()
            } else {
                false
            };

            include.push(BlockKind::new_if(current));
        } else if trimmed.starts_with("#ELIF ") || trimmed.starts_with("//ELIF ") {
            let cond = if trimmed.starts_with("#ELIF ") {
                trimmed.strip_prefix("#ELIF ").unwrap()
            } else {
                trimmed.strip_prefix("//ELIF ").unwrap()
            };
            let last = include.pop().unwrap();

            // Only evaluate condition if no other branches evaluated to true
            let current = if matches!(last, BlockKind::IfElse(false, false)) {
                engine.eval::<bool>(cond).unwrap()
            } else {
                false
            };

            include.push(last.into_else_if(current));
        } else if trimmed.starts_with("#ELSE") || trimmed.starts_with("//ELSE") {
            let last = include.pop().unwrap();
            include.push(last.into_else());
        } else if trimmed.starts_with("#ENDIF") || trimmed.starts_with("//ENDIF") {
            let prev = include.pop();
            assert!(
                matches!(prev, Some(BlockKind::IfElse(_, _))),
                "ENDIF without IF in {}:{}",
                file_path,
                line_no
            );
        // Trim #+ and //+
        } else if include.iter().all(|v| v.include_line()) {
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

fn process_options(template: &Template, args: &Args) {
    let mut success = true;
    let all_options = template.all_options();

    let selected_config = ActiveConfiguration {
        chip: args.chip,
        selected: args.option.clone(),
        options: &template.options,
    };

    for option in &selected_config.selected {
        // Find the matching option in the template
        let mut option_found = false;
        let mut option_found_for_chip = false;
        for &option_item in all_options.iter().filter(|item| item.name == *option) {
            option_found = true; // The input refers to an existing option.

            // Check if the chip is supported. If the chip list is empty, all chips are supported.
            // We don't immediately fail in case the option is not present for the chip, because
            // it may exist as a separate entry (e.g. with different properties).
            if !option_item.chips.iter().any(|chip| chip == &args.chip)
                && !option_item.chips.is_empty()
            {
                continue;
            }

            option_found_for_chip = true;

            // Is the option allowed to be selected?
            if selected_config.is_option_active(option_item) {
                continue;
            }

            // Something is wrong, print the constraints that are not met.
            success = false;
            let o = GeneratorOptionItem::Option(option_item.clone());
            let Relationships {
                requires,
                disabled_by,
                ..
            } = selected_config.collect_relationships(&o);

            if !requires
                .iter()
                .all(|requirement| args.option.iter().any(|r| r == requirement))
            {
                log::error!(
                    "Option '{}' requires {}",
                    option_item.name,
                    option_item.requires.join(", ")
                );
            }

            for disabled in disabled_by {
                log::error!("Option '{}' is disabled by {}", option_item.name, disabled);
            }
        }

        if !option_found {
            log::error!("Unknown option '{}'", option);
            success = false;
        } else if !option_found_for_chip {
            log::error!(
                "Option '{}' is not supported for chip {}",
                option,
                args.chip
            );
            success = false;
        }
    }

    if !success {
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

    #[test]
    fn test_basic_elseif() {
        let template = r#"
        #IF option("opt1")
        opt1
        #ELIF option("opt2")
        opt2
        #ELIF option("opt3")
        opt3
        #ELSE
        opt4
        #ENDIF
        "#;

        const PAIRS: &[(&[&str], &str)] = &[
            (&["opt1"], "opt1"),
            (&["opt1", "opt2"], "opt1"),
            (&["opt1", "opt3"], "opt1"),
            (&["opt1", "opt2", "opt3"], "opt1"),
            (&["opt2"], "opt2"),
            (&["opt2", "opt3"], "opt2"),
            (&["opt3"], "opt3"),
            (&["opt4"], "opt4"),
            (&[], "opt4"),
        ];

        for (options, expected) in PAIRS.iter().cloned() {
            let res = process_file(
                template,
                &options.iter().map(|o| o.to_string()).collect::<Vec<_>>(),
                &[],
                &mut String::from("main.rs"),
            )
            .unwrap();
            assert_eq!(expected, res.trim(), "options: {:?}", options);
        }
    }
}
