use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use esp_generate::{
    config::{find_option, ActiveConfiguration},
    template::{GeneratorOptionItem, Template},
};
use esp_metadata::Chip;

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
    },

    /// Prints all valid combinations of options for a given chip
    Options {
        /// Target chip to check
        #[arg(value_enum)]
        chip: Chip,
        /// Verify all possible options combinations
        #[arg(short, long)]
        all_combinations: bool,
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
        } => check(&workspace, chip, all_combinations),

        Commands::Options {
            chip,
            all_combinations,
        } => {
            for options in options_for_chip(chip, all_combinations)? {
                println!("{:?}", options);
            }
            Ok(())
        }
    }
}

// ----------------------------------------------------------------------------
// CHECK

fn check(workspace: &Path, chip: Chip, all_combinations: bool) -> Result<()> {
    log::info!("CHECK: {chip}");

    const PROJECT_NAME: &str = "test";
    for options in options_for_chip(chip, all_combinations)? {
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
            .args(["check", "--release"])
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            project_dir.close()?;
            bail!("Failed to execute cargo check subcommand")
        }

        // Run clippy against the generated project to check for lint errors:
        let output = Command::new("cargo")
            .args(["clippy", "--no-deps", "--", "-Dwarnings"])
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            project_dir.close()?;
            bail!("Failed to execute cargo clippy subcommand")
        }

        // Ensure that the generated project is correctly formatted:
        let output = Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .current_dir(project_path.join(PROJECT_NAME))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            project_dir.close()?;
            bail!("Failed to execute cargo fmt subcommand")
        }

        project_dir.close()?;
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

    if !config.requirements_met(option) {
        return Ok(());
    }

    config.select(option.name.to_string());

    Ok(())
}

fn is_valid(config: &ActiveConfiguration) -> bool {
    for item in config.selected.iter() {
        let option = find_option(item, &config.options).unwrap();
        if !config.requirements_met(option) {
            return false;
        }
    }
    true
}

fn options_for_chip(chip: Chip, all_combinations: bool) -> Result<Vec<Vec<String>>> {
    let options = include_str!("../../template/template.yaml");
    let template = serde_yaml::from_str::<Template>(options)?;

    // A list of each option, along with its dependencies
    let mut available_options = vec![];
    for item in template.options.iter() {
        let mut config = ActiveConfiguration {
            chip,
            selected: vec![],
            options: &template.options,
        };

        if let GeneratorOptionItem::Option(option) = item {
            enable_config_and_dependencies(&mut config, &option.name)?;
        }

        if is_valid(&config) {
            available_options.push(config.selected);
        }
    }

    available_options.sort();
    available_options.dedup();

    if !all_combinations {
        return Ok(available_options);
    }

    // Return all the combination of availble options
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
        "--release",
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
