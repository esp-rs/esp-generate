use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use esp_generate::{
    config::{find_option, ActiveConfiguration},
    template::{GeneratorOptionCategory, GeneratorOptionItem, Template},
};
use esp_metadata::Chip;
use log::info;

// Unfortunate hard-coded list of non-codegen options
const IGNORED_CATEGORIES: &[&str] = &["editor", "optional"];

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

    const PROJECT_NAME: &str = "test";
    for options in to_check {
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
            .args([if build { "build" } else { "check" }])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
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
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
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
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            bail!("Failed to execute cargo clippy subcommand")
        }

        // Ensure that the generated project is correctly formatted:
        let output = Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            bail!("Failed to execute cargo fmt subcommand")
        }
    }

    Ok(())
}

fn enable_config_and_dependencies(config: &mut ActiveConfiguration, option: &str) -> Result<()> {
    if config.selected.contains(&option.to_string()) {
        return Ok(());
    }

    let option = find_option(option, &config.options)
        .ok_or_else(|| anyhow::anyhow!("Option not found: {option}"))?;

    for dependency in option.requires.iter() {
        if dependency.starts_with('!') {
            continue;
        }
        enable_config_and_dependencies(config, dependency)?;
    }

    if !config.is_option_active(option) {
        return Ok(());
    }

    config.select(option.name.to_string());

    Ok(())
}

fn is_valid(config: &ActiveConfiguration) -> bool {
    let mut groups = HashSet::new();

    for item in config.selected.iter() {
        let option = find_option(item, &config.options).unwrap();

        // Option could not have been selected on UI.
        if !config.is_option_active(option) {
            return false;
        }

        // Reject combination if a selection group contains two selected options. This prevents
        // testing mutually exclusive options like defmt and log.
        if !option.selection_group.is_empty() && !groups.insert(option.selection_group.clone()) {
            return false;
        }
    }

    true
}

fn options_for_chip(chip: Chip, all_combinations: bool) -> Result<Vec<Vec<String>>> {
    let options = include_str!("../../template/template.yaml");
    let template = serde_yaml::from_str::<Template>(options)?;

    fn collect(all_options: &mut Vec<String>, category: &GeneratorOptionCategory) {
        for option in &category.options {
            match option {
                GeneratorOptionItem::Option(option) => {
                    all_options.push(option.name.clone());
                }
                GeneratorOptionItem::Category(category)
                    if !IGNORED_CATEGORIES.contains(&category.name.as_str()) =>
                {
                    collect(all_options, category)
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
                    all_options.push(option.name.clone());
                }
            }
            GeneratorOptionItem::Category(category)
                if !IGNORED_CATEGORIES.contains(&category.name.as_str()) =>
            {
                collect(&mut all_options, &category)
            }
            _ => {}
        }
    }

    // A list of each option, along with its dependencies
    let mut available_options = vec![vec![]];

    for base_template in &template_selectors {
        for option in &all_options {
            let option = find_option(&option, &template.options).unwrap();
            let mut config = ActiveConfiguration {
                chip,
                selected: vec![],
                options: &template.options,
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
        return Ok(available_options);
    }

    // Return all the combination of available options
    let mut result = vec![];
    for i in 0..(1 << available_options.len()) {
        let mut config = ActiveConfiguration {
            chip,
            selected: vec![],
            options: &template.options,
        };

        for j in 0..available_options.len() {
            if i & (1 << j) != 0 {
                config.selected.extend(available_options[j].clone());
            }
        }
        config.selected.sort();
        config.selected.dedup();

        if is_valid(&config) {
            result.push(config.selected);
        }
    }

    result.sort();
    result.dedup();

    Ok(result)
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
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()?;

    Ok(())
}
