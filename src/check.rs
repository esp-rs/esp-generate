use core::str;

use esp_metadata::Chip;

#[derive(Debug, PartialEq)]
struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

#[derive(Debug, PartialEq, Eq)]
enum CheckResult {
    Ok,
    WrongVersion,
    NotFound,
}

pub fn check(chip: Chip, probe_rs_required: bool) {
    // TODO: check +nightly if needed
    let rust_version = get_version(
        "rustc",
        if chip.is_xtensa() {
            &["+esp"]
        } else {
            &["+stable"]
        },
    );

    let rust_toolchain = if chip.is_xtensa() { "esp" } else { "stable" };
    let rust_toolchain_tool = if chip.is_xtensa() { "espup" } else { "rustup" };

    let espflash_version = get_version("espflash", &[]);

    let probers_version = get_version("probe-rs", &[]);
    let probers_suggestion_kind = if probe_rs_required {
        "required"
    } else {
        "suggested"
    };

    println!("\nChecking installed versions");

    let mut requirements_unsatisfied = false;
    requirements_unsatisfied |= print_result(
        &format!("Rust ({rust_toolchain})"),
        check_version(rust_version, 1, 86, 0),
        format!("minimum required version is 1.86 - use `{rust_toolchain_tool}` to upgrade"),
        format!("not found - use `{rust_toolchain_tool}` to install"),
        true,
    );
    requirements_unsatisfied |= print_result(
        "espflash",
        check_version(espflash_version, 3, 3, 0),
        "minimum required version is 3.3.0 - see https://crates.io/crates/espflash",
        "not found - see https://crates.io/crates/espflash for installation instructions",
        true,
    );
    requirements_unsatisfied |= print_result(
        "probe-rs",
        check_version(probers_version, 0, 25, 0),
        format!("minimum {probers_suggestion_kind} version is 0.25.0 - see https://probe.rs/docs/getting-started/installation/ for how to upgrade"),
        "not found - see https://probe.rs/docs/getting-started/installation/ for how to install",
        probe_rs_required,
    );

    if requirements_unsatisfied {
        println!("\nFor more details see https://docs.espressif.com/projects/rust/book/")
    }
}

fn print_result(
    name: &str,
    check_result: CheckResult,
    wrong_version_help: impl AsRef<str>,
    not_found_help: impl AsRef<str>,
    required: bool,
) -> bool {
    let wrong_version_help = wrong_version_help.as_ref();
    let not_found_help = not_found_help.as_ref();

    match check_result {
        CheckResult::Ok => {
            println!("🆗 {name}");
            false
        }
        CheckResult::WrongVersion => {
            println!("🛑 {name} ({wrong_version_help})");
            required
        }
        CheckResult::NotFound => {
            println!("❌ {name} ({not_found_help})");
            required
        }
    }
}

fn check_version(version: Option<Version>, major: u8, minor: u8, patch: u8) -> CheckResult {
    match version {
        Some(v) if (v.major, v.minor, v.patch) < (major, minor, patch) => CheckResult::WrongVersion,
        Some(_) => CheckResult::Ok,
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
        assert_eq!(check_version(version, 1, 84, 0), CheckResult::Ok);
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
}
