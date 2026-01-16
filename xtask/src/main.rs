use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use esp_generate::{
    config::{ActiveConfiguration, find_option, flatten_options},
    modules::populate_module_category,
    template::{GeneratorOptionCategory, GeneratorOptionItem, Template},
};
use esp_metadata::Chip;
use itertools::Itertools;
use log::info;

// Unfortunate hard-coded list of non-codegen options.
const IGNORED_CATEGORIES: &[&str] = &["editor", "optional", "toolchain"];
// The module selector generates way too many test cases to check with --all-combinations.
const IGNORED_CATEGORIES_FULL: &[&str] = &["editor", "optional", "toolchain", "module"];

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate a project; ensure that it builds, lints pass, and that it is
    /// formatted correctly
    Check {
        /// Target chip to check
        #[arg(value_enum)]
        chip: Chip,
        /// Verify all possible options combinations
        #[arg(short, long)]
        all_combinations: bool,
        /// Actually build projects, instead of just checking them
        #[arg(short, long)]
        build: bool,
        /// Just print what would be tested
        #[arg(short, long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::new()
        .filter_module("xtask", log::LevelFilter::Info)
        .init();

    // The directory containing the Cargo manifest for the 'xtask' package is
    // a subdirectory within the workspace:
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = workspace.parent().unwrap().canonicalize()?;

    match Cli::parse().command {
        Commands::Check {
            chip,
            all_combinations,
            build,
            dry_run,
        } => check(&workspace, chip, all_combinations, build, dry_run),
    }
}

// ----------------------------------------------------------------------------
// CHECK

fn check(
    workspace: &Path,
    chip: Chip,
    all_combinations: bool,
    build: bool,
    dry_run: bool,
) -> Result<()> {
    if build {
        log::info!("BUILD: {chip}");
    } else {
        log::info!("CHECK: {chip}");
    }

    info!("Going to check");
    let to_check = options_for_chip(chip, all_combinations)?;
    for check in &to_check {
        info!("\"{}\"", check.join(", "));
    }

    if dry_run {
        return Ok(());
    }

    let target_dir =
        PathBuf::from(std::env::var("CARGO_TARGET_DIR").unwrap_or("target".to_string()));
    let mut counter = 0;
    const PROJECT_NAME: &str = "test";
    for options in to_check {
        counter += 1;
        if counter >= 100 {
            // don't use `cargo clean` since it will fail because it can't delete the xtask executable
            for f in std::fs::read_dir(&target_dir)? {
                let f = f?.path();

                // don't fail just because we can't remove a directory or file
                if f.is_dir() {
                    let _ = std::fs::remove_dir_all(f);
                } else {
                    let _ = std::fs::remove_file(f);
                }
            }

            counter = 0;
        }

        log::info!("WITH OPTIONS: {options:?}");

        // We will generate the project in a temporary directory, to avoid
        // making a mess when this subcommand is executed locally:
        let project_dir = tempfile::tempdir()?;
        let project_path = project_dir.path();
        log::info!("PROJECT PATH: {project_path:?}");

        // Generate a project targeting the specified chip and using the
        // specified generation options:
        generate(workspace, &project_path, PROJECT_NAME, chip, &options)?;

        // Ensure that the generated project builds without errors:
        let output = Command::new("cargo")
            .args([if build { "build" } else { "check" }, "--quiet"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            bail!("Failed to execute cargo check subcommand")
        }

        // Ensure that the generated test project builds also:
        if options.iter().any(|o| o == "embedded-test") {
            let output = Command::new("cargo")
                .args(["test", "--no-run"])
                .env_remove("RUSTUP_TOOLCHAIN")
                .current_dir(project_path.join(PROJECT_NAME))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output()?;
            if !output.status.success() {
                bail!("Failed to execute cargo test subcommand")
            }
        }

        // Run clippy against the generated project to check for lint errors:
        let output = Command::new("cargo")
            .args(["clippy", "--no-deps", "--", "-Dwarnings"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            bail!("Failed to execute cargo clippy subcommand")
        }

        // Ensure that the generated project is correctly formatted:
        let output = Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            bail!("Failed to execute cargo fmt subcommand")
        }
    }

    Ok(())
}

fn enable_config_and_dependencies(config: &mut ActiveConfiguration, option: &str) -> Result<()> {
    let (idx, option) = find_option(option, &config.flat_options)
        .ok_or_else(|| anyhow::anyhow!("Option not found: {option}"))?;

    if config.selected.contains(&idx) {
        return Ok(());
    }

    // We copy `requires` into so that the borrow from `find_option`
    // ends before we recursive call and later
    // mutate `config`. Not doing so would make
    // the borrow checker sad.
    for dependency in option.requires.clone() {
        if dependency.starts_with('!') {
            continue;
        }
        enable_config_and_dependencies(config, &dependency)?;
    }

    let option = &config.flat_options[idx];

    if !config.is_option_active(option) {
        return Ok(());
    }

    config.select_idx(idx);

    Ok(())
}

fn is_valid(config: &ActiveConfiguration) -> bool {
    let mut groups = HashSet::new();

    for item in config.selected.iter() {
        let option = &config.flat_options[*item];

        // Option could not have been selected on UI.
        if !config.is_option_active(option) {
            return false;
        }

        // Reject combination if a selection group contains two selected options. This prevents
        // testing mutually exclusive options like defmt and log.
        if !option.selection_group.is_empty() && !groups.insert(&option.selection_group) {
            return false;
        }
    }

    true
}

fn options_for_chip(chip: Chip, all_combinations: bool) -> Result<Vec<Vec<String>>> {
    let ignored_categories = if all_combinations {
        IGNORED_CATEGORIES_FULL
    } else {
        IGNORED_CATEGORIES
    };

    let options = include_str!("../../template/template.yaml");
    let mut template = serde_yaml::from_str::<Template>(options)?;

    // Populate the module category with chip-specific modules
    populate_module_category(chip, &mut template.options);
    let flat_options = flatten_options(&template.options);

    fn collect<'data>(
        all_options: &mut Vec<&'data str>,
        category: &'data GeneratorOptionCategory,
        ignored_categories: &[&str],
    ) {
        for option in &category.options {
            match option {
                GeneratorOptionItem::Option(option) => {
                    all_options.push(option.name.as_str());
                }
                GeneratorOptionItem::Category(category)
                    if !ignored_categories.contains(&category.name.as_str()) =>
                {
                    collect(all_options, category, ignored_categories)
                }
                _ => {}
            }
        }
    }

    let mut all_options = vec![];
    // When no base template is selected, the blocking one is used, which doesn't have any visible
    // options, so we need to add a placeholder for it.
    let mut template_selectors = vec![None];

    for option in &template.options {
        match option {
            GeneratorOptionItem::Option(option) => {
                if option.selection_group == "base-template" {
                    template_selectors.push(Some(option.name.clone()));
                } else {
                    all_options.push(option.name.as_str());
                }
            }
            GeneratorOptionItem::Category(category)
                if !ignored_categories.contains(&category.name.as_str()) =>
            {
                collect(&mut all_options, &category, ignored_categories)
            }
            _ => {}
        }
    }

    // A list of each option, along with its dependencies
    let mut available_options = vec![vec![]];

    for base_template in &template_selectors {
        for option in &all_options {
            let (_idx, option) = find_option(&option, &flat_options)
                .unwrap_or_else(|| panic!("Option not found: {}", option));
            let mut config = ActiveConfiguration {
                chip,
                selected: vec![],
                flat_options: flat_options.clone(),
                options: template.options.clone(),
            };

            if let Some(base_template) = base_template {
                enable_config_and_dependencies(&mut config, &base_template)?;
            }

            enable_config_and_dependencies(&mut config, &option.name)?;

            if is_valid(&config) {
                config.selected.sort();
                available_options.push(config.selected);
            }
        }
    }

    available_options.sort();
    available_options.dedup();

    if !all_combinations {
        return Ok(available_options
            .into_iter()
            .map(|idxs| {
                idxs.into_iter()
                    .map(|idx| flat_options[idx].name.clone())
                    .collect()
            })
            .collect());
    }

    // Return all the combination of available options
    let start = Instant::now();
    let mut result = vec![];
    // Avoid cloning the template for each checked configuration.
    let mut template_options = Some(template.options);
    let mut flat_options = Some(flat_options);
    for options in available_options.iter().map(|v| v.as_slice()).powerset() {
        let mut config = ActiveConfiguration {
            chip,
            selected: options
                .into_iter()
                .flatten()
                .collect::<HashSet<_>>() // We don't need iteration order stability, slightly faster than `.unique()`
                .into_iter()
                .cloned()
                .collect(),
            options: template_options.take().unwrap(),
            flat_options: flat_options.take().unwrap(),
        };

        if is_valid(&config) {
            config.selected.sort();
            result.push(config.selected);
        }

        template_options = Some(config.options);
        flat_options = Some(config.flat_options);
    }

    result.sort();
    result.dedup();

    let elapsed = start.elapsed();
    log::info!(
        "Generated {} test configurations in {:?}",
        result.len(),
        elapsed
    );

    let flat_options = flat_options.unwrap();

    Ok(result
        .into_iter()
        .map(|idxs| {
            idxs.into_iter()
                .map(|idx| flat_options[idx].name.clone())
                .collect()
        })
        .collect())
}

fn generate(
    workspace: &Path,
    project_path: &Path,
    project_name: &str,
    chip: Chip,
    options: &[String],
) -> Result<()> {
    let mut args = vec![
        "run",
        "--quiet",
        "--no-default-features",
        "--",
        "--headless",
        &format!("--chip={chip}"),
        &format!("--output-path={}", project_path.display()),
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect::<Vec<_>>();

    for option in options {
        args.extend(["-o".to_string(), option.to_owned()]);
    }

    args.push(project_name.to_string());

    Command::new("cargo")
        .args(args)
        .current_dir(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()?;

    Ok(())
}
