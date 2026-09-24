use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use esp_generate::{
    Loaded, TemplateSource,
    sweep::{self, Coverage, SweepOptions},
};
use log::info;

/// Parts of the bundled template that decide nothing about the generated code.
const SKIPPED_CATEGORIES: &[&str] = &["editor", "optional", "toolchain"];

/// Fifty boards sharing one code path: no coverage, enormous matrix.
const SKIPPED_GROUPS_FULL: &[&str] = &["module"];

/// The base template decides whether generated code is async, so every option
/// is worth testing against each.
const CROSSED_GROUPS: &[&str] = &["base-template"];

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
        chip: String,
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
        } => check(&workspace, &chip, all_combinations, build, dry_run),
    }
}

// ----------------------------------------------------------------------------
// CHECK

fn check(
    workspace: &Path,
    chip: &str,
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

        // Generate a project using the specified generation options:
        generate(workspace, project_path, PROJECT_NAME, &options)?;

        // Ensure that the generated project builds without errors:
        let output = Command::new("cargo")
            .args([if build { "build" } else { "check" }])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .output()?;
        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            bail!("Failed to execute cargo check subcommand")
        }

        // Ensure that the generated test project builds also:
        if options.iter().any(|o| o == "embedded-test") {
            let output = Command::new("cargo")
                .args(["test", "--no-run"])
                .env_remove("RUSTUP_TOOLCHAIN")
                .current_dir(project_path.join(PROJECT_NAME))
                .output()?;
            if !output.status.success() {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                bail!("Failed to execute cargo test subcommand")
            }
        }

        // Run clippy against the generated project to check for lint errors:
        let output = Command::new("cargo")
            .args(["clippy", "--no-deps", "--", "-Dwarnings"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .output()?;
        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            bail!("Failed to execute cargo clippy subcommand")
        }

        // Ensure that the generated project is correctly formatted:
        let output = Command::new("cargo")
            .args(["fmt", "--", "--check"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(project_path.join(PROJECT_NAME))
            .output()?;
        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            bail!("Failed to execute cargo fmt subcommand")
        }
    }

    Ok(())
}

/// The test matrix for one chip, as `-o` argument lists. The enumeration is the
/// generator's; this only adds the bundled template's exclusions.
fn options_for_chip(chip: &str, all_combinations: bool) -> Result<Vec<Vec<String>>> {
    // The same load the generator does, validations included, so xtask cannot
    // enumerate a template the generator would refuse.
    let loaded = Loaded::open(TemplateSource::Bundled)?;

    let excluded_groups: Vec<String> = if all_combinations {
        SKIPPED_GROUPS_FULL.iter().map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };

    sweep::enumerate(
        &loaded.template,
        &loaded.resolved,
        &SweepOptions {
            coverage: if all_combinations {
                Coverage::Combinations
            } else {
                Coverage::Individual
            },
            pinned: vec![chip.to_string()],
            excluded_groups,
            excluded_categories: SKIPPED_CATEGORIES.iter().map(|s| s.to_string()).collect(),
            crossed_groups: CROSSED_GROUPS.iter().map(|s| s.to_string()).collect(),
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn generate(
    workspace: &Path,
    project_path: &Path,
    project_name: &str,
    options: &[String],
) -> Result<()> {
    let mut args: Vec<String> = [
        "run",
        "--quiet",
        "--no-default-features",
        "--",
        "--headless",
        &format!("--output-path={}", project_path.display()),
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();

    for option in options {
        args.extend(["-o".to_string(), option.to_owned()]);
    }

    args.push(project_name.to_string());

    // Capture stderr (rather than discarding it via `Stdio::null()`)
    // so that any failure from the underlying `esp-generate` invocation — e.g.
    // a bad option, a filesystem error, or a panic — surfaces to the xtask
    // caller instead of silently turning into an empty project directory.
    let output = Command::new("cargo")
        .args(args)
        .current_dir(workspace)
        .stdout(Stdio::null())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprintln!("{stderr}");
        }
        bail!("esp-generate failed with options {options:?}");
    }

    Ok(())
}
