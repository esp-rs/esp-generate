use std::{
    collections::HashMap,
    error::Error,
    fmt::Display,
    ops::Range,
    path::{Path, PathBuf},
};

use clap::Parser;
use env_logger::{Builder, Env};
use serde::Deserialize;
use walkdir::WalkDir;

mod tui;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Root of the project
    #[arg(short = 'P', long)]
    path: Option<PathBuf>,

    /// Do not check for updates
    #[arg(short, long, global = true, action)]
    #[cfg(feature = "update-informer")]
    skip_update_check: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateConfig {
    name: String,
    options: Vec<ConfigOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfigOption {
    name: String,
    description: String,
    default_value: Value,
    actual_value: Value,
    constraint: Option<Constraint>,
}

/// Supported configuration value types.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum Value {
    /// Booleans.
    Bool(bool),
    /// Integers.
    Integer(i128),
    /// Strings.
    String(String),
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Value::Bool(v) => format!("{v}"),
                Value::Integer(i) => format!("{i}"),
                Value::String(s) => s.to_string(),
            }
        )
        .ok();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum Constraint {
    /// Only allow negative integers, i.e. any values less than 0.
    NegativeInteger,
    /// Only allow non-negative integers, i.e. any values greater than or equal
    /// to 0.
    NonNegativeInteger,
    /// Only allow positive integers, i.e. any values greater than to 0.
    PositiveInteger,
    /// Ensure that an integer value falls within the specified range.
    IntegerInRange(Range<i128>),
    /// String-Enumeration. Only allows one of the given Strings.
    Enumeration(Vec<String>),
    // everything else we don't know or can't handle
    #[serde(other)]
    Other,
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

    let work_dir = args.path.clone().unwrap_or(".".into());

    ensure_fresh_build(&work_dir)?;

    let mut configs = parse_configs(&work_dir)?;
    let initial_configs = configs.clone();
    let mut errors_to_show = None;

    loop {
        let repository = tui::Repository::new(configs.clone());

        // TUI stuff ahead
        let terminal = tui::init_terminal()?;

        // create app and run it
        let updated_cfg = tui::App::new(errors_to_show, repository).run(terminal)?;

        tui::restore_terminal()?;

        // done with the TUI
        if let Some(updated_cfg) = updated_cfg {
            configs = updated_cfg.clone();
            apply_config(&work_dir, updated_cfg)?;
        } else {
            println!("Reverted configuration...");
            apply_config(&work_dir, initial_configs)?;
            break;
        }

        if let Some(errors) = check_build_after_changes(&work_dir) {
            errors_to_show = Some(errors);
        } else {
            println!("Updated configuration...");
            break;
        }
    }

    Ok(())
}

fn apply_config(path: &Path, updated_cfg: Vec<CrateConfig>) -> Result<(), Box<dyn Error>> {
    let config_toml = path.join(".cargo/config.toml");

    let mut config = std::fs::read_to_string(&config_toml)?
        .as_str()
        .parse::<toml::Table>()?;

    if !config.contains_key("env") {
        config.insert("env".to_string(), toml::Value::Table(toml::map::Map::new()));
    }

    let envs = config.get_mut("env").unwrap().as_table_mut().unwrap();

    for cfg in updated_cfg {
        let prefix = cfg.name.to_ascii_uppercase().replace("-", "_");
        for option in cfg.options {
            let key = format!(
                "{prefix}_CONFIG_{}",
                option.name.to_ascii_uppercase().replace("-", "_")
            );

            if option.actual_value != option.default_value {
                let value = toml::value::Value::String(format!("{}", option.actual_value));

                envs.insert(key, value);
            } else {
                envs.remove(&key);
            }
        }
    }

    // this will replace the whole file - including reformat and shaving off comments
    // consider just replacing the ENV section?
    std::fs::write(&config_toml, config.to_string().as_bytes())?;

    Ok(())
}

fn parse_configs(path: &Path) -> Result<Vec<CrateConfig>, Box<dyn Error>> {
    // we cheat by just trying to find the latest version of the config files
    // this should be fine since we force a fresh build before
    let mut candidates: Vec<_> = WalkDir::new(path.join("target"))
        .into_iter()
        .filter_entry(|entry| {
            entry.file_type().is_dir() || {
                if let Some(name) = entry.file_name().to_str() {
                    name.ends_with("_config_data.json")
                } else {
                    false
                }
            }
        })
        .filter(|entry| !entry.as_ref().unwrap().file_type().is_dir())
        .map(|entry| entry.unwrap())
        .collect();
    candidates.sort_by_key(|entry| entry.metadata().unwrap().modified().unwrap());

    let mut crate_config_table_to_json: HashMap<String, PathBuf> = HashMap::new();

    for e in candidates {
        if e.file_name()
            .to_str()
            .unwrap()
            .ends_with("_config_data.json")
        {
            let crate_name = e
                .file_name()
                .to_str()
                .unwrap()
                .replace("_config_data.json", "")
                .replace("_", "-");
            crate_config_table_to_json.insert(crate_name.clone(), e.path().to_path_buf());
        }
    }

    let mut configs = Vec::new();

    for (crate_name, path) in crate_config_table_to_json {
        configs.push(CrateConfig {
            name: crate_name,
            options: serde_json::from_str(std::fs::read_to_string(&path)?.as_str()).map_err(
                |_| {
                    format!(
                        "Unable to read config file {:?} - try `cargo clean` first",
                        path
                    )
                },
            )?,
        });
    }
    configs.sort_by_key(|entry| entry.name.clone());

    if configs.is_empty() {
        return Err("No config files found.".into());
    }

    Ok(configs)
}

fn ensure_fresh_build(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let status = std::process::Command::new("cargo")
        .arg("build")
        .current_dir(path)
        .status()?;

    if !status.success() {
        return Err("Your project doesn't build. Fix the errors first.".into());
    }

    Ok(())
}

fn check_build_after_changes(path: &PathBuf) -> Option<String> {
    println!("Check configuration...");

    let status = std::process::Command::new("cargo")
        .arg("build")
        .current_dir(path)
        .stdout(std::process::Stdio::inherit())
        .output();

    if let Ok(status) = &status {
        if status.status.success() {
            return None;
        }
    }

    let mut errors = String::new();

    for line in String::from_utf8(status.unwrap().stderr)
        .unwrap_or_default()
        .lines()
    {
        if line.contains("the evaluated program panicked at '") {
            let error = line[line.find('\'').unwrap() + 1..].to_string();
            let error = error[..error.find("',").unwrap_or(error.len())].to_string();
            errors.push_str(&format!("{error}\n"));
        }
    }

    Some(errors)
}
