use core::str;
use std::path::{Path, PathBuf};

use ratatui::crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use esp_generate::contract;

/// Host tool/toolchain versions. The *type* is shared with the SDK's contract
/// versions (one `semver::Version` for the whole workspace), but the *parsing*
/// is deliberately not: see [`parse_lenient`].
pub use semver::Version;

/// Parse a version reported by a host tool, or declared as a `rust-version`.
///
/// Strict semver is wrong here, for two reasons:
///
/// - **Components may be missing.** `rust-version = "1.95"` in the template's
///   `Cargo.toml` is a valid Cargo MSRV but not a valid semver version;
///   `Version::parse` rejects it. Missing minor/patch default to 0.
/// - **Prereleases should pass.** Someone running `probe-rs 0.31.0-rc.1`
///   has the features of 0.31.0 for our purposes, so the suffix is dropped
///   rather than being allowed to sort the version below the requirement.
///
/// Neither leniency is acceptable for contract versions, where a prerelease
/// sorting below a final release is the entire point of
/// `contract::is_compatible` — which is why that path uses
/// `semver::Version::parse` directly and this function is not shared with it.
pub fn parse_lenient(s: &str) -> Option<Version> {
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');

    let major: u64 = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;

    // Reject trailing junk like "1.2.3.4".
    if parts.next().is_some() {
        return None;
    }

    Some(Version::new(major, minor, patch))
}

/// Minimum versions the generator pre-flights, declared once so the check, the
/// install prompt, and the help text cannot drift apart. `esp-config` is
/// advisory; the rest gate generation when their tool is required.
///
/// (`contract::release` is borrowed only as a `const` constructor —
/// `Version::new` is not `const`. These are host-tool versions, and the
/// leniency/strictness split described on [`parse_lenient`] still applies.)
const ESPFLASH_MIN: Version = contract::release(3, 3, 0);
const PROBE_RS_MIN: Version = contract::release(0, 31, 0);
const ESP_CONFIG_MIN: Version = contract::release(0, 5, 0);

#[derive(Debug, PartialEq, Eq)]
enum CheckResult {
    Ok(Version),
    WrongVersion,
    NotFound,
}

pub fn check(
    is_xtensa: bool,
    probe_rs_required: bool,
    msrv: Option<Version>,
    requires_nightly: bool,
    headless: bool,
    selected_toolchain: Option<&str>,
) {
    let rust_toolchain: String = if let Some(name) = selected_toolchain {
        name.to_string()
    } else if is_xtensa {
        "esp".to_string()
    } else if requires_nightly {
        "nightly".to_string()
    } else {
        "stable".to_string()
    };

    let rust_toolchain_tool = if is_xtensa { "espup" } else { "rustup" };

    if rust_toolchain_tool == "espup" {
        // We don't enforce a minimum espup version here, we just care that it exists.
        let _ = get_version_or_install(
            "espup",
            &[],
            headless,
            Some(&["cargo", "install", "espup", "--locked"]),
            None,
        );
    }

    let rust_install_cmd: &[&str] = if rust_toolchain_tool == "espup" {
        &["espup", "install"]
    } else {
        &["rustup", "toolchain", "install", &rust_toolchain]
    };

    let rust_version = get_version_or_install(
        "rustc",
        &[format!("+{rust_toolchain}").as_str()],
        headless,
        Some(rust_install_cmd),
        msrv.clone(),
    );

    let espflash_version = if !probe_rs_required {
        get_version_or_install(
            "espflash",
            &[],
            headless,
            Some(&["cargo", "install", "espflash", "--locked"]),
            Some(ESPFLASH_MIN),
        )
    } else {
        get_version("espflash", &[])
    };

    let probers_version = if probe_rs_required {
        get_version_or_install(
            "probe-rs",
            &[],
            headless,
            Some(&["cargo", "install", "probe-rs-tools", "--locked"]),
            Some(PROBE_RS_MIN),
        )
    } else {
        get_version("probe-rs", &[])
    };

    let esp_config_version = get_version_or_install(
        "esp-config",
        &[],
        headless,
        Some(&[
            "cargo",
            "install",
            "esp-config",
            "--features=tui",
            "--locked",
        ]),
        Some(ESP_CONFIG_MIN),
    );

    let probers_suggestion_kind = if probe_rs_required {
        "required"
    } else {
        "suggested"
    };

    println!(
        "{}",
        create_check_results(
            probe_rs_required,
            msrv,
            &rust_toolchain,
            rust_version,
            rust_toolchain_tool,
            espflash_version,
            probers_version,
            esp_config_version,
            probers_suggestion_kind,
        )
    );
}

#[allow(clippy::too_many_arguments)]
fn create_check_results(
    probe_rs_required: bool,
    msrv: Option<Version>,
    rust_toolchain: &str,
    rust_version: Option<Version>,
    rust_toolchain_tool: &str,
    espflash_version: Option<Version>,
    probers_version: Option<Version>,
    esp_config_version: Option<Version>,
    probers_suggestion_kind: &str,
) -> String {
    let mut result = String::new();

    result.push_str("\nChecking installed versions\n");

    // A template that is not a Cargo project declares no MSRV, so the toolchain
    // is checked for presence only.
    let (rust_result, rust_too_old) = match &msrv {
        Some(msrv) => (
            check_version(rust_version.as_ref(), msrv),
            format!(
                "minimum required version is {msrv} - run `{rust_toolchain_tool} update` to upgrade"
            ),
        ),
        None => (
            rust_version.map_or(CheckResult::NotFound, CheckResult::Ok),
            String::new(),
        ),
    };

    let mut requirements_unsatisfied = false;
    requirements_unsatisfied |= format_result(
        false,
        &format!("Rust ({rust_toolchain})"),
        rust_result,
        rust_too_old,
        format!("not found - use `{rust_toolchain_tool}` to install"),
        true,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        false,
        "espflash",
        check_version(espflash_version.as_ref(), &ESPFLASH_MIN),
        format!(
            "minimum required version is {ESPFLASH_MIN} - see https://crates.io/crates/espflash"
        ),
        "not found - see https://crates.io/crates/espflash for installation instructions",
        true,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        !probe_rs_required,
        "probe-rs",
        check_version(probers_version.as_ref(), &PROBE_RS_MIN),
        format!(
            "minimum {probers_suggestion_kind} version is {PROBE_RS_MIN} - see https://probe.rs/docs/getting-started/installation/ for how to upgrade"
        ),
        format!(
            "not found - see https://probe.rs/docs/getting-started/installation/ for how to install ({probers_suggestion_kind})"
        ),
        probe_rs_required,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        true,
        "esp-config",
        check_version(esp_config_version.as_ref(), &ESP_CONFIG_MIN),
        format!("minimum suggested version is {ESP_CONFIG_MIN}"),
        "not found - use `cargo install esp-config --features=tui --locked` to install (installation is optional)",
        probe_rs_required,
        &mut result,
    );

    if requirements_unsatisfied {
        result.push_str("\nFor more details see https://docs.espressif.com/projects/rust/book/\n")
    }

    result
}

fn format_result(
    friendly: bool,
    name: &str,
    check_result: CheckResult,
    wrong_version_help: impl AsRef<str>,
    not_found_help: impl AsRef<str>,
    required: bool,
    message: &mut String,
) -> bool {
    let emojis = if friendly {
        "🆗💡💡"
    } else {
        "🆗🛑❌"
    };
    let wrong_version_help = wrong_version_help.as_ref();
    let not_found_help = not_found_help.as_ref();

    match check_result {
        CheckResult::Ok(found) => {
            message.push_str(&format!(
                "{} {name}: {found}\n",
                emojis.chars().next().unwrap()
            ));
            false
        }
        CheckResult::WrongVersion => {
            message.push_str(&format!(
                "{} {name} ({wrong_version_help})\n",
                emojis.chars().nth(1).unwrap()
            ));
            required
        }
        CheckResult::NotFound => {
            message.push_str(&format!(
                "{} {name} ({not_found_help})\n",
                emojis.chars().nth(2).unwrap()
            ));
            required
        }
    }
}

fn check_version(version: Option<&Version>, required: &Version) -> CheckResult {
    match version {
        Some(v) if v < required => CheckResult::WrongVersion,
        Some(v) => CheckResult::Ok(v.clone()),
        None => CheckResult::NotFound,
    }
}

pub(crate) fn get_version(cmd: &str, args: &[&str]) -> Option<Version> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .arg("--version")
        .output();

    let Ok(output) = output else {
        return None;
    };

    if !output.status.success() {
        return None;
    }

    str::from_utf8(&output.stdout)
        .ok()
        .and_then(|s| extract_version(cmd, s))
}

fn extract_version(cmd: &str, output: &str) -> Option<Version> {
    for line in output.lines() {
        if let Some(version) = try_extract_version(cmd, line) {
            return Some(version);
        }
    }

    None
}

fn try_extract_version(cmd: &str, line: &str) -> Option<Version> {
    let mut parts = line.split_whitespace();
    let name = parts.next();

    if name != Some(cmd) {
        return None;
    }

    let version = parts.next()?;

    parse_lenient(version)
}

pub fn offensive_cargo_config_check(path: &Path) -> bool {
    let mut current = if let Some(parent) = path.parent() {
        PathBuf::from(parent)
    } else {
        return false;
    };

    loop {
        if current.join(".cargo/config.toml").exists() {
            return true;
        }

        current = if let Some(parent) = current.parent() {
            if parent == current {
                break;
            }
            parent.to_path_buf()
        } else {
            return false;
        };
    }

    false
}

/// A combination of `get_version` and `prompt_install`: if the tool is not found
/// or does not meet the minimum version (when provided) and an install command
/// is provided, it will prompt the user to install/upgrade it and then re-check.
fn get_version_or_install(
    cmd: &str,
    args: &[&str],
    headless: bool,
    install_cmd: Option<&[&str]>,
    min_version: Option<Version>,
) -> Option<Version> {
    let version = get_version(cmd, args);

    if headless {
        return version;
    }

    match min_version {
        Some(min) => match check_version(version.as_ref(), &min) {
            CheckResult::Ok(_) => return version, // nothing to do - tool exists and version is above minimal allowed
            CheckResult::WrongVersion | CheckResult::NotFound => {
                let Some(install_cmd) = install_cmd else {
                    // no way to offer an automatic install/upgrade
                    return version;
                };
                prompt_install(cmd, install_cmd);
            }
        },
        None => {
            if version.is_some() {
                // we don't know minimum version and the tool exists – nothing to do
                return version;
            }
            // tool doesn't exist - prompt to install it
            let install_cmd = install_cmd?;
            prompt_install(cmd, install_cmd);
        }
    }

    get_version(cmd, args)
}

fn prompt_install(name: &str, cmd: &[&str]) {
    let command_str = cmd.join(" ");
    println!("🛑 {name} is not installed or is below the required version.");

    if name == "probe-rs" && cfg!(target_os = "linux") {
        println!(
            "💡 On Linux, probe-rs requires additional setup before installation.\n\
            See https://probe.rs/docs/getting-started/installation/ for details."
        );
    }

    println!("Do you want to run `{command_str}` now? [y/N]");

    if let Err(err) = enable_raw_mode() {
        println!(
            "Failed to enter raw mode for install prompt: {err}.\n\
            You can run `{command_str}` manually if you want to install the tool."
        );
        return;
    }

    //default: don't run anything unless user explicitly presses 'y'
    let mut run_cmd: bool = false;

    loop {
        match event::read() {
            Ok(Event::Key(key)) => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        run_cmd = true;
                        break;
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        break;
                    }
                    _ => {
                        // ignore other keys
                    }
                }
            }
            Ok(_) => {
                // ignore other events
            }
            Err(err) => {
                println!(
                    "Failed to read key press for `{command_str}` prompt: {err}.\n\
                    You can run the command manually if you wish to install the tool."
                );
                break;
            }
        }
    }

    if let Err(err) = disable_raw_mode() {
        println!(
            "Failed to leave raw mode cleanly after selection: {err}.\n\
            You may need to reset your terminal."
        );
    }

    if run_cmd {
        match std::process::Command::new(cmd[0]).args(&cmd[1..]).status() {
            Ok(status) if status.success() => {
                println!("✅ `{command_str}` finished successfully");
            }
            Ok(status) => {
                println!("❌ `{command_str}` failed with status {status}");
            }
            Err(err) => {
                println!("❌ Failed to run `{command_str}`: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_version() {
        // Ok
        let version = Version::new(1, 84, 0);
        assert_eq!(
            check_version(Some(&version), &Version::new(1, 84, 0)),
            CheckResult::Ok(Version::new(1, 84, 0))
        );
        // Wrong major
        let version = Version::new(0, 85, 0);
        assert_eq!(
            check_version(Some(&version), &Version::new(1, 84, 0)),
            CheckResult::WrongVersion
        );
        // Wrong minor
        let version = Version::new(1, 83, 0);
        assert_eq!(
            check_version(Some(&version), &Version::new(1, 84, 0)),
            CheckResult::WrongVersion
        );
        // Wrong patch
        let version = Version::new(1, 84, 0);
        assert_eq!(
            check_version(Some(&version), &Version::new(1, 84, 1)),
            CheckResult::WrongVersion
        );
        // Not found
        assert_eq!(
            check_version(None, &Version::new(1, 84, 0)),
            CheckResult::NotFound
        );
    }

    #[test]
    fn test_extract_version() {
        let input = r#"New version of espflash is available: v3.3.0

espflash 1.7.0"#;

        let output = extract_version("espflash", input);
        assert_eq!(output, Some(Version::new(1, 7, 0)));
    }

    #[test]
    fn lenient_parsing_accepts_what_host_tools_actually_report() {
        // Cargo MSRVs routinely omit the patch — the bundled template's
        // `rust-version` is one of these, and strict semver rejects it.
        assert_eq!(parse_lenient("1.95"), Some(Version::new(1, 95, 0)));
        assert_eq!(parse_lenient("3"), Some(Version::new(3, 0, 0)));
        assert_eq!(parse_lenient("1.88.0"), Some(Version::new(1, 88, 0)));
        assert!(Version::parse("1.95").is_err(), "strict semver rejects it");

        // A tool prerelease is treated as its base release: someone on
        // `probe-rs 0.31.0-rc.1` has what we need. This is the opposite of the
        // contract-floor rule, which is exactly why the two do not share a
        // parser.
        assert_eq!(parse_lenient("0.31.0-rc.1"), Some(Version::new(0, 31, 0)));
        assert_eq!(parse_lenient("3.3.0+g1234"), Some(Version::new(3, 3, 0)));
        assert!(
            check_version(
                parse_lenient("0.31.0-rc.1").as_ref(),
                &Version::new(0, 31, 0)
            ) != CheckResult::WrongVersion,
            "a tool rc must not be reported as too old"
        );

        // Junk is still rejected rather than silently becoming 0.0.0.
        assert_eq!(parse_lenient("1.2.3.4"), None);
        assert_eq!(parse_lenient("x"), None);
        assert_eq!(parse_lenient(""), None);
    }

    #[test]
    fn test_ui_all_good() {
        assert_eq!(
            create_check_results(
                /*probe_rs_required*/ true,
                /*msrv*/
                Some(Version::new(1, 88, 0)),
                /*rust_toolchain*/ "nightly",
                /*rust_version*/
                Some(Version::new(1, 88, 0)),
                /*rust_toolchain_tool*/ "rustup",
                /*espflash_version*/
                Some(ESPFLASH_MIN),
                /*probers_version*/
                Some(PROBE_RS_MIN),
                /*esp_config_version*/
                Some(ESP_CONFIG_MIN),
                /*probers_suggestion_kind*/ "required",
            ),
            "
Checking installed versions
🆗 Rust (nightly): 1.88.0
🆗 espflash: 3.3.0
🆗 probe-rs: 0.31.0
🆗 esp-config: 0.5.0
"
            .to_string()
        );
    }

    #[test]
    fn test_ui_all_good_probe_rs_optional_not_installed() {
        assert_eq!(
            create_check_results(
                /*probe_rs_required*/ false,
                /*msrv*/
                Some(Version::new(1, 88, 0)),
                /*rust_toolchain*/ "nightly",
                /*rust_version*/
                Some(Version::new(1, 88, 0)),
                /*rust_toolchain_tool*/ "rustup",
                /*espflash_version*/
                Some(ESPFLASH_MIN),
                /*probers_version*/ None,
                /*esp_config_version*/
                Some(ESP_CONFIG_MIN),
                /*probers_suggestion_kind*/ "suggested",
            ),
            "
Checking installed versions
🆗 Rust (nightly): 1.88.0
🆗 espflash: 3.3.0
💡 probe-rs (not found - see https://probe.rs/docs/getting-started/installation/ for how to install (suggested))
🆗 esp-config: 0.5.0
"
            .to_string()
        );
    }

    #[test]
    fn test_ui_nothing_installed() {
        assert_eq!(
            create_check_results(
                /*probe_rs_required*/ true,
                /*msrv*/
                Some(Version::new(1, 88, 0)),
                /*rust_toolchain*/ "stable",
                /*rust_version*/ None,
                /*rust_toolchain_tool*/ "rustup",
                /*espflash_version*/ None,
                /*probers_version*/ None,
                /*esp_config_version*/ None,
                /*probers_suggestion_kind*/ "required",
            ),
            "
Checking installed versions
❌ Rust (stable) (not found - use `rustup` to install)
❌ espflash (not found - see https://crates.io/crates/espflash for installation instructions)
❌ probe-rs (not found - see https://probe.rs/docs/getting-started/installation/ for how to install (required))
💡 esp-config (not found - use `cargo install esp-config --features=tui --locked` to install (installation is optional))

For more details see https://docs.espressif.com/projects/rust/book/
"
            .to_string()
        );
    }
}
