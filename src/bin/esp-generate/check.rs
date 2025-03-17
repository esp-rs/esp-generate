use core::str;

use esp_metadata::Chip;

#[derive(Debug)]
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

pub fn check(chip: Chip) {
    let rust_version = get_version(
        "cargo",
        if chip.is_xtensa() {
            &["+esp"]
        } else {
            &["+stable"]
        },
    );

    let espflash_version = get_version("espflash", &[]);
    let probers_version = get_version("probe-rs", &[]);

    println!("\nChecking installed versions");
    print_result("Rust", check_version(rust_version, 1, 84, 0));
    print_result("espflash", check_version(espflash_version, 3, 3, 0));
    print_result("probe-rs", check_version(probers_version, 0, 25, 0));
}

fn print_result(name: &str, check_result: CheckResult) {
    match check_result {
        CheckResult::Ok => println!("🆗 {}", name),
        CheckResult::WrongVersion => println!("🛑 {}", name),
        CheckResult::NotFound => println!("❌ {}", name),
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

    match output {
        Ok(output) => {
            if output.status.success() {
                if let Ok(output) = str::from_utf8(&output.stdout) {
                    let mut parts = output.split_whitespace();
                    let _name = parts.next();
                    let version = parts.next();
                    if let Some(version) = version {
                        let mut version = version.split(&['.', '-', '+']);
                        let major = version.next().unwrap().parse::<u8>().unwrap();
                        let minor = version.next().unwrap().parse::<u8>().unwrap();
                        let patch = version.next().unwrap().parse::<u8>().unwrap();
                        return Some(Version {
                            major,
                            minor,
                            patch,
                        });
                    }
                }
            }

            None
        }
        Err(_) => None,
    }
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
}
