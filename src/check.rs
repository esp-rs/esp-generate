use core::str;
use std::{fmt::Display, str::FromStr};

use esp_metadata::Chip;

#[derive(Debug, PartialEq, Eq)]
pub struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(&['.', '-', '+']);
        let major = parts
            .next()
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or("Invalid major version")?;
        let minor = parts
            .next()
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or("Invalid minor version")?;
        let patch = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CheckResult {
    Ok(Version),
    WrongVersion,
    NotFound,
}

pub fn check(chip: Chip, probe_rs_required: bool, msrv: Version, requires_nightly: bool) {
    let rust_toolchain = if chip.is_xtensa() {
        "esp"
    } else if requires_nightly {
        "nightly"
    } else {
        "stable"
    };

    let rust_version = get_version("rustc", &[format!("+{rust_toolchain}").as_str()]);

    let rust_toolchain_tool = if chip.is_xtensa() { "espup" } else { "rustup" };

    let espflash_version = get_version("espflash", &[]);

    let probers_version = get_version("probe-rs", &[]);

    let esp_config_version = get_version("esp-config", &[]);

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
            rust_toolchain,
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
    msrv: Version,
    rust_toolchain: &'static str,
    rust_version: Option<Version>,
    rust_toolchain_tool: &'static str,
    espflash_version: Option<Version>,
    probers_version: Option<Version>,
    esp_config_version: Option<Version>,
    probers_suggestion_kind: &'static str,
) -> String {
    let mut result = String::new();

    result.push_str("\nChecking installed versions\n");

    let mut requirements_unsatisfied = false;
    requirements_unsatisfied |= format_result(
        &format!("Rust ({rust_toolchain})"),
        check_version(rust_version, msrv.major, msrv.minor, msrv.patch),
        format!("minimum required version is 1.86 - use `{rust_toolchain_tool}` to upgrade"),
        format!("not found - use `{rust_toolchain_tool}` to install"),
        true,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        "espflash",
        check_version(espflash_version, 3, 3, 0),
        "minimum required version is 3.3.0 - see https://crates.io/crates/espflash",
        "not found - see https://crates.io/crates/espflash for installation instructions",
        true,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        "probe-rs",
        check_version(probers_version, 0, 25, 0),
        format!("minimum {probers_suggestion_kind} version is 0.25.0 - see https://probe.rs/docs/getting-started/installation/ for how to upgrade"),
        format!("not found - see https://probe.rs/docs/getting-started/installation/ for how to install ({probers_suggestion_kind})"),
        probe_rs_required,
        &mut result,
    );
    requirements_unsatisfied |= format_result(
        "esp-config",
        check_version(esp_config_version, 0, 5, 0),
        "minimum suggested version is 0.5.0",
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
    name: &str,
    check_result: CheckResult,
    wrong_version_help: impl AsRef<str>,
    not_found_help: impl AsRef<str>,
    required: bool,
    message: &mut String,
) -> bool {
    let wrong_version_help = wrong_version_help.as_ref();
    let not_found_help = not_found_help.as_ref();

    match check_result {
        CheckResult::Ok(found) => {
            message.push_str(&format!("🆗 {name}: {found}\n"));
            false
        }
        CheckResult::WrongVersion => {
            message.push_str(&format!("🛑 {name} ({wrong_version_help})\n"));
            required
        }
        CheckResult::NotFound => {
            message.push_str(&format!("❌ {name} ({not_found_help})\n"));
            required
        }
    }
}

fn check_version(version: Option<Version>, major: u8, minor: u8, patch: u8) -> CheckResult {
    match version {
        Some(v) if (v.major, v.minor, v.patch) < (major, minor, patch) => CheckResult::WrongVersion,
        Some(v) => CheckResult::Ok(v),
        None => CheckResult::NotFound,
    }
}

fn get_version(cmd: &str, args: &[&str]) -> Option<Version> {
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

    let mut version = version.split(&['.', '-', '+']);
    let major = version.next()?.parse::<u8>().ok()?;
    let minor = version.next()?.parse::<u8>().ok()?;
    let patch = version.next()?.parse::<u8>().ok()?;
    Some(Version {
        major,
        minor,
        patch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_version() {
        // Ok
        let version = Some(Version {
            major: 1,
            minor: 84,
            patch: 0,
        });
        assert_eq!(
            check_version(version, 1, 84, 0),
            CheckResult::Ok(Version {
                major: 1,
                minor: 84,
                patch: 0,
            })
        );
        // Wrong major
        let version = Some(Version {
            major: 0,
            minor: 85,
            patch: 0,
        });
        assert_eq!(check_version(version, 1, 84, 0), CheckResult::WrongVersion);
        // Wrong minor
        let version = Some(Version {
            major: 1,
            minor: 83,
            patch: 0,
        });
        assert_eq!(check_version(version, 1, 84, 0), CheckResult::WrongVersion);
        // Wrong patch
        let version = Some(Version {
            major: 1,
            minor: 84,
            patch: 0,
        });
        assert_eq!(check_version(version, 1, 84, 1), CheckResult::WrongVersion);
        // Not found
        assert_eq!(check_version(None, 1, 84, 0), CheckResult::NotFound);
    }

    #[test]
    fn test_extract_version() {
        let input = r#"New version of espflash is available: v3.3.0

espflash 1.7.0"#;

        let output = extract_version("espflash", input);
        assert_eq!(
            output,
            Some(Version {
                major: 1,
                minor: 7,
                patch: 0
            })
        );
    }

    #[test]
    fn test_ui_all_good() {
        assert_eq!(
            create_check_results(
                /*probe_rs_required*/ true,
                /*msrv*/
                Version {
                    major: 1,
                    minor: 85,
                    patch: 0
                },
                /*rust_toolchain*/ "nightly",
                /*rust_version*/
                Some(Version {
                    major: 1,
                    minor: 85,
                    patch: 0
                }),
                /*rust_toolchain_tool*/ "rustup",
                /*espflash_version*/
                Some(Version {
                    major: 3,
                    minor: 3,
                    patch: 0
                }),
                /*probers_version*/
                Some(Version {
                    major: 0,
                    minor: 25,
                    patch: 0
                }),
                /*esp_config_version*/
                Some(Version {
                    major: 0,
                    minor: 5,
                    patch: 0
                }),
                /*probers_suggestion_kind*/ "required",
            ),
            "
Checking installed versions
🆗 Rust (nightly): 1.85.0
🆗 espflash: 3.3.0
🆗 probe-rs: 0.25.0
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
                Version {
                    major: 1,
                    minor: 85,
                    patch: 0
                },
                /*rust_toolchain*/ "nightly",
                /*rust_version*/
                Some(Version {
                    major: 1,
                    minor: 85,
                    patch: 0
                }),
                /*rust_toolchain_tool*/ "rustup",
                /*espflash_version*/
                Some(Version {
                    major: 3,
                    minor: 3,
                    patch: 0
                }),
                /*probers_version*/ None,
                /*esp_config_version*/
                Some(Version {
                    major: 0,
                    minor: 5,
                    patch: 0
                }),
                /*probers_suggestion_kind*/ "suggested",
            ),
            "
Checking installed versions
🆗 Rust (nightly): 1.85.0
🆗 espflash: 3.3.0
❌ probe-rs (not found - see https://probe.rs/docs/getting-started/installation/ for how to install (suggested))
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
                Version {
                    major: 1,
                    minor: 85,
                    patch: 0
                },
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
❌ esp-config (not found - use `cargo install esp-config --features=tui --locked` to install (installation is optional))

For more details see https://docs.espressif.com/projects/rust/book/
"
            .to_string()
        );
    }
}
