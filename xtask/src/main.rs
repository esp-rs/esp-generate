use std::{
    collections::{HashSet, HashMap},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use std::ffi::OsStr;

use anyhow::{bail, Result, Context};
use clap::{Parser, Subcommand};
use esp_generate::{
    config::{find_option, ActiveConfiguration},
    template::{GeneratorOptionCategory, GeneratorOptionItem, Template},
};
use esp_metadata::Chip;
use log::{info, warn};

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

/// A builder for constructing cargo command line arguments.
#[derive(Clone, Debug, Default)]
pub struct CargoArgsBuilder {
    pub(crate) artifact_name: String,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) manifest_path: Option<PathBuf>,
    pub(crate) toolchain: Option<String>,
    pub(crate) subcommand: String,
    pub(crate) target: Option<String>,
    pub(crate) features: Vec<String>,
    pub(crate) args: Vec<String>,
    pub(crate) configs: Vec<String>,
    pub(crate) env_vars: HashMap<String, String>,

}

impl CargoArgsBuilder {
    pub fn new(artifact_name: String) -> Self {
    Self {
        subcommand: artifact_name.clone(),
        artifact_name,
        ..Default::default()
    }
}

    /// Set the path to the Cargo manifest file (Cargo.toml)
    #[must_use]
    pub fn manifest_path(mut self, path: PathBuf) -> Self {
        self.manifest_path = Some(path);
        self
    }

    /// Set the path to the Cargo configuration file (.cargo/config.toml)
    #[must_use]
    pub fn config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// Set the Rust toolchain to use.
    #[must_use]
    pub fn toolchain<S>(mut self, toolchain: S) -> Self
    where
        S: Into<String>,
    {
        self.toolchain = Some(toolchain.into());
        self
    }

    /// Set the cargo subcommand to use.
    #[must_use]
    pub fn subcommand<S>(mut self, subcommand: S) -> Self
    where
        S: Into<String>,
    {
        self.subcommand = subcommand.into();
        self
    }

    /// Set the compilation target to use.
    #[must_use]
    pub fn target<S>(mut self, target: S) -> Self
    where
        S: Into<String>,
    {
        self.target = Some(target.into());
        self
    }

    /// Set the cargo features to use.
    #[must_use]
    pub fn features(mut self, features: &[String]) -> Self {
        self.features = features.to_vec();
        self
    }

    /// Add a single argument to the cargo command line.
    #[must_use]
    pub fn arg<S>(mut self, arg: S) -> Self
    where
        S: Into<String>,
    {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments to the cargo command line.
    #[must_use]
    pub fn args<S>(mut self, args: &[S]) -> Self
    where
        S: Clone + Into<String>,
    {
        for arg in args {
            self.args.push(arg.clone().into());
        }
        self
    }

    /// Add a single argument to the cargo command line.
    pub fn add_arg<S>(&mut self, arg: S) -> &mut Self
    where
        S: Into<String>,
    {
        self.args.push(arg.into());
        self
    }

    /// Adds a raw configuration argument (--config, -Z, ...)
    #[must_use]
    pub fn config<S>(mut self, arg: S) -> Self
    where
        S: Into<String>,
    {
        self.add_config(arg);
        self
    }

    /// Adds a raw configuration argument (--config, -Z, ...)
    pub fn add_config<S>(&mut self, arg: S) -> &mut Self
    where
        S: Into<String>,
    {
        self.configs.push(arg.into());
        self
    }

    /// Adds an environment variable
    pub fn add_env_var<S>(&mut self, key: S, value: S) -> &mut Self
    where
        S: Into<String>,
    {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Build the final list of cargo command line arguments.
    #[must_use]
    pub fn build(&self) -> Vec<String> {
        let mut args = vec![];

        if let Some(ref toolchain) = self.toolchain {
            args.push(format!("+{toolchain}"));
        }

        args.push(self.subcommand.clone());

        if let Some(manifest_path) = &self.manifest_path {
            args.push("--manifest-path".to_string());
            args.push(manifest_path.display().to_string());
        }

        if let Some(config_path) = &self.config_path {
            args.push("--config".to_string());
            args.push(config_path.display().to_string());
        }

        if let Some(ref target) = self.target {
            args.push(format!("--target={target}"));
        }

        for config in self.configs.iter() {
            args.push(config.clone());
        }

        if !self.features.is_empty() {
            args.push(format!("--features={}", self.features.join(",")));
        }

        for arg in self.args.iter() {
            args.push(arg.clone());
        }

        log::debug!("Built cargo args: {:?}", args);
        args
    }
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

        let current_dir = project_path.join(PROJECT_NAME);

        // batcher **per project**
        let mut commands = CargoCommandBatcher::new();

        // Ensure that the generated project builds without errors:
        commands.push(
            CargoArgsBuilder::new(if build { "build".to_string() } else { "check".to_string() })
                .target(chip.target()),
        );

        // Ensure that the generated test project builds also:
        if options.iter().any(|o| o == "embedded-test") {
            commands.push(
                CargoArgsBuilder::new("test".to_string())
                    .args(&["--no-run".to_string()])
                    .target(chip.target()),
            );
        }

        // Run clippy against the generated project to check for lint errors:
        commands.push(
            CargoArgsBuilder::new("clippy".to_string())
                .args(&["--no-deps".to_string(), "--".to_string(), "-Dwarnings".to_string()])
                .target(chip.target()),
        );

        // TODO get me back
        // commands.push(CargoArgsBuilder::new("fmt".to_string())
        //     .args(&["--".to_string(), "--check".to_string()]));

        for c in commands.build(false) {
            println!("Command: cargo {}", c.command.join(" ").replace("---", "\n    ---"));
            c.run(false, &current_dir)?;
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


#[derive(Debug, PartialEq, Eq, Hash, Clone)]
struct BatchKey {
    config_file: String,
    toolchain: Option<String>,
    config: Vec<String>,
    env_vars: Vec<(String, String)>,
}

impl BatchKey {
    fn from_command(command: &CargoArgsBuilder) -> Self {
        let config_file = if let Some(config_path) = &command.config_path {
            std::fs::read_to_string(config_path).unwrap_or_default()
        } else {
            String::new()
        };

        Self {
            toolchain: command.toolchain.clone(),
            config: command.configs.clone(),
            config_file,
            env_vars: {
                let mut env_vars = command
                    .env_vars
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>();

                env_vars.sort();
                env_vars
            },
        }
    }
}

#[derive(Debug)]
pub struct CargoCommandBatcher {
    commands: HashMap<BatchKey, Vec<CargoArgsBuilder>>,
}

#[derive(Debug, Clone)]
pub struct BuiltCommand {
    pub artifact_name: String,
    pub command: Vec<String>,
    pub env_vars: Vec<(String, String)>,
}

impl BuiltCommand {
    pub fn run(&self, capture: bool, dir: &PathBuf) -> Result<String> {
        run_with_env(&self.command, &dir, self.env_vars.clone(), capture)
    }
}

fn run_with_env<I, K, V>(args: &[String], cwd: &Path, envs: I, capture: bool) -> Result<String>
where
    I: IntoIterator<Item = (K, V)> + core::fmt::Debug,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    if !cwd.is_dir() {
        bail!("The `cwd` argument MUST be a directory");
    }

    #[cfg(target_os = "windows")]
    fn windows_safe_path(p: &Path) -> &Path {
        if let Ok(stripped) = p.strip_prefix(r"\\?\") {
            stripped
        } else {
            p
        }
    }
    #[cfg(not(target_os = "windows"))]
    fn windows_safe_path(p: &Path) -> &Path {
        p
    }

    let cwd = windows_safe_path(cwd);

    log::debug!(
        "Running `cargo {}` in {:?} - Environment {:?}",
        args.join(" "),
        cwd,
        envs
    );

    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("RUSTUP_TOOLCHAIN")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    if args.iter().any(|a| a.starts_with('+')) {
        command.env_remove("CARGO");
    }

    let output = command
        .stdin(Stdio::inherit())
        .output()
        .with_context(|| format!("Couldn't get output for command {command:?}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        bail!(
            "Failed to execute cargo subcommand `cargo {}`",
            args.join(" "),
        )
    }
}

impl CargoCommandBatcher {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn push(&mut self, command: CargoArgsBuilder) {
        let key = BatchKey::from_command(&command);
        self.commands.entry(key).or_default().push(command);
    }

    fn build_for_cargo_batch(&self) -> Vec<BuiltCommand> {
        let mut all = Vec::new();

        for (key, group) in self.commands.iter() {
            if group.len() == 1 {
                all.push(Self::build_one_for_cargo(&group[0]));
                continue;
            }

            let mut command = Vec::new();
            let mut batch_len = 0;
            let mut commands_in_batch = 0;

            // Windows be Windows, it has a command length limit.
            let limit = if cfg!(target_os = "windows") {
                Some(8191)
            } else {
                None
            };

            for item in group.iter() {
                // Only some commands can be batched
                let batchable = ["build", "doc", "check"];
                if !batchable
                    .iter()
                    .any(|&subcommand| subcommand == item.subcommand)
                {
                    all.push(Self::build_one_for_cargo(item));
                    continue;
                }

                let mut c = item.clone();

                c.toolchain = None;
                c.configs = Vec::new();
                c.config_path = None;

                let args = c.build();

                let command_chars = 4 + args.iter().map(|arg| arg.len() + 1).sum::<usize>();

                if !command.is_empty()
                    && let Some(limit) = limit
                    && batch_len + command_chars > limit
                {
                    all.push(BuiltCommand {
                        artifact_name: String::from("batch"),
                        command: std::mem::take(&mut command),
                        env_vars: key.env_vars.clone(),
                    });
                }

                if command.is_empty() {
                    if let Some(tc) = key.toolchain.as_ref() {
                        command.push(format!("+{tc}"));
                    }

                    command.push("batch".to_string());
                    if !key.config_file.is_empty()
                        && let Some(config_path) = &group[0].config_path
                    {
                        command.push("--config".to_string());
                        command.push(config_path.display().to_string());
                    }
                    command.extend_from_slice(&key.config);

                    commands_in_batch = 0;
                    batch_len = command.iter().map(|s| s.len() + 1).sum::<usize>() - 1;
                }

                command.push("---".to_string());
                command.extend_from_slice(&args);

                commands_in_batch += 1;
                batch_len += command_chars;
            }

            if commands_in_batch > 0 {
                all.push(BuiltCommand {
                    artifact_name: String::from("batch"),
                    command,
                    env_vars: key.env_vars.clone(),
                });
            }
        }

        all
    }

    fn build_for_cargo(&self) -> Vec<BuiltCommand> {
        let mut all = Vec::new();

        for group in self.commands.values() {
            for item in group.iter() {
                all.push(Self::build_one_for_cargo(item));
            }
        }

        all
    }

    pub fn build_one_for_cargo(item: &CargoArgsBuilder) -> BuiltCommand {
        BuiltCommand {
            artifact_name: item.artifact_name.clone(),
            command: {
                let mut args = item.build();

                if item.args.iter().any(|arg| arg == "--artifact-dir") {
                    args.push("-Zunstable-options".to_string());
                }

                args
            },
            env_vars: item
                .env_vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    pub fn build(&self, no_batch: bool) -> Vec<BuiltCommand> {
        let cargo_batch_available = Command::new("cargo")
            .arg("batch")
            .arg("-h")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if cargo_batch_available && !no_batch {
            self.build_for_cargo_batch()
        } else {
            if !no_batch {
                log::warn!("You don't have cargo batch installed. Falling back to cargo.");
                log::warn!("You should really install cargo-batch.");
                log::warn!(
                    "cargo install --git https://github.com/embassy-rs/cargo-batch cargo --bin cargo-batch --locked"
                );
            }
            self.build_for_cargo()
        }
    }
}

impl Drop for CargoCommandBatcher {
    fn drop(&mut self) {}
}
