use core::str;

use esp_metadata::Chip;

#[derive(Debug)]
struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

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
        Some(version) => {
            if version.major < major {
                return CheckResult::WrongVersion;
            }

            if version.major == major && version.minor < minor {
                return CheckResult::WrongVersion;
            }

            if version.major == major && version.minor == minor && version.patch < patch {
                return CheckResult::WrongVersion;
            }

            CheckResult::Ok
        }
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
