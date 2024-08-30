use std::{env, path::PathBuf, process};

use clap::{Parser, ValueEnum};

mod template_files;
mod tui;

#[derive(Clone, Copy)]
pub struct GeneratorOption {
    name: &'static str,
    display_name: &'static str,
}

#[derive(Clone, Copy)]
pub struct GeneratorOptionCategory {
    name: &'static str,
    display_name: &'static str,
    options: &'static [GeneratorOptionItem],
}

#[derive(Clone, Copy)]
pub enum GeneratorOptionItem {
    Category(GeneratorOptionCategory),
    Option(GeneratorOption),
}
impl GeneratorOptionItem {
    fn title(&self) -> String {
        match self {
            GeneratorOptionItem::Category(category) => category.display_name.to_string(),
            GeneratorOptionItem::Option(option) => option.display_name.to_string(),
        }
    }

    fn name(&self) -> String {
        match self {
            GeneratorOptionItem::Category(category) => category.name.to_string(),
            GeneratorOptionItem::Option(option) => option.name.to_string(),
        }
    }

    fn is_category(&self) -> bool {
        match self {
            GeneratorOptionItem::Category(_) => true,
            GeneratorOptionItem::Option(_) => false,
        }
    }
}

static OPTIONS: &[GeneratorOptionItem] = &[
    GeneratorOptionItem::Option(GeneratorOption {
        name: "alloc",
        display_name: "Alloc",
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "wifi",
        display_name: "Wifi",
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "embassy",
        display_name: "Embassy",
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "probe-rs",
        display_name: "Flash via probe-rs, use defmt",
    }),
    GeneratorOptionItem::Option(GeneratorOption {
        name: "stack_protector",
        display_name: "Enable stack-smash protection (Nightly only)",
    }),
    GeneratorOptionItem::Category(GeneratorOptionCategory {
        name: "optional",
        display_name: "Options",
        options: &[
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wokwi",
                display_name: "Wokwi Support",
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "dev-container",
                display_name: "Dev-Container Support",
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "ci",
                display_name: "Add GitHub CI",
            }),
        ],
    }),
];

static CHIP_VARS: &[(Chip, &[(&'static str, &'static str)])] = &[
    (
        Chip::Esp32,
        &[
            ("xtensa", "xtensa"),
            ("rust_target", "xtensa-esp32-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0",
            ),
        ],
    ),
    (
        Chip::Esp32S2,
        &[
            ("xtensa", "xtensa"),
            ("rust_target", "xtensa-esp32s2-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0",
            ),
        ],
    ),
    (
        Chip::Esp32S3,
        &[
            ("xtensa", "xtensa"),
            ("rust_target", "xtensa-esp32s3-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0",
            ),
        ],
    ),
    (
        Chip::Esp32C2,
        &[
            ("riscv", "riscv"),
            ("rust_target", "riscv32imc-unknown-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0",
            ),
        ],
    ),
    (
        Chip::Esp32C3,
        &[
            ("riscv", "riscv"),
            ("rust_target", "riscv32imc-unknown-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0",
            ),
        ],
    ),
    (
        Chip::Esp32C6,
        &[
            ("riscv", "riscv"),
            ("rust_target", "riscv32imac-unknown-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0",
            ),
        ],
    ),
    (
        Chip::Esp32H2,
        &[
            ("riscv", "riscv"),
            ("rust_target", "riscv32imac-unknown-none-elf"),
            (
                "esp_wifi_timer",
                "esp_hal::timer::systimer::SystemTimer::new(peripherals.SYSTIMER).alarm0",
            ),
        ],
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
#[value(rename_all = "LOWER_CASE")]
pub enum Chip {
    Esp32,
    Esp32S2,
    Esp32S3,
    Esp32C2,
    Esp32C3,
    Esp32C6,
    Esp32H2,
}

impl Chip {
    pub fn to_string(&self) -> String {
        match self {
            Chip::Esp32 => "esp32",
            Chip::Esp32S2 => "esp32s2",
            Chip::Esp32S3 => "esp32s3",
            Chip::Esp32C2 => "esp32c2",
            Chip::Esp32C3 => "esp32c3",
            Chip::Esp32C6 => "esp32c6",
            Chip::Esp32H2 => "esp32h2",
        }
        .to_string()
    }

    pub fn architecture_name(&self) -> String {
        match self {
            Chip::Esp32 | Chip::Esp32S2 | Chip::Esp32S3 => "xtensa",
            _ => "riscv",
        }
        .to_string()
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    name: String,

    #[arg(short, long)]
    chip: Chip,

    #[arg(long)]
    headless: bool,

    #[arg(short, long)]
    option: Vec<String>,
}

fn main() {
    let args = Args::parse();

    if env::current_dir().unwrap().join(&args.name).exists() {
        eprintln!("Directory already exists");
        process::exit(-1);
    }

    let mut selected = if !args.headless {
        let repository = tui::Repository::new(OPTIONS, &args.option);
        // TUI stuff ahead
        let terminal = tui::init_terminal().unwrap();

        // create app and run it
        let selected = tui::App::new(repository).run(terminal).unwrap();

        tui::restore_terminal().unwrap();
        // done with the TUI

        if let Some(selected) = selected {
            selected
        } else {
            process::exit(-1);
        }
    } else {
        args.option.clone()
    };

    selected.push(args.chip.architecture_name());

    let mut variables = vec![
        ("project-name".to_string(), args.name.clone()),
        ("mcu".to_string(), args.chip.to_string()),
    ];

    for (chip, vars) in CHIP_VARS {
        if chip == &args.chip {
            for (key, value) in vars.iter() {
                variables.push((key.to_string(), value.to_string()))
            }
        }
    }

    let dir = PathBuf::from(&args.name);
    std::fs::create_dir(&dir).unwrap();

    for &(file_path, contents) in template_files::TEMPLATE_FILES.iter() {
        let path = dir.join(file_path);

        let processed = process_file(file_path, contents, &selected, &variables);
        if let Some(processed) = processed {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, processed).unwrap();
        }
    }

    process::Command::new("cargo")
        .arg("fmt")
        .arg("--")
        .arg("--config")
        .arg("group_imports=StdExternalCrate")
        .arg("--config")
        .arg("imports_granularity=Module")
        .current_dir(&args.name)
        .output()
        .unwrap();

    process::Command::new("git")
        .arg("init")
        .current_dir(&args.name)
        .output()
        .unwrap();
}

fn process_file(
    path: &str,
    contents: &str,
    options: &Vec<String>,
    variables: &Vec<(String, String)>,
) -> Option<String> {
    if path.ends_with("Cargo.lock") {
        return None;
    }

    let mut res = String::new();

    let mut replace = None;
    let mut replacement = None;
    let mut include = vec![true];
    let mut first_line = true;

    for line in contents.lines() {
        if first_line {
            let trimmed = line.trim();
            let cond = if trimmed.starts_with("//INCLUDEFILE ") {
                Some(trimmed.strip_prefix("//INCLUDEFILE ").unwrap())
            } else if trimmed.starts_with("#INCLUDEFILE ") {
                Some(trimmed.strip_prefix("#INCLUDEFILE ").unwrap())
            } else {
                None
            };

            if let Some(cond) = cond {
                let include_file = if cond.starts_with("!") {
                    !options.contains(&cond[1..].to_string())
                } else {
                    options.contains(&cond.to_string())
                };

                if !include_file {
                    return None;
                } else {
                    continue;
                }
            }
        }
        first_line = false;

        // that's a bad workaround
        if line.trim() == "#[rustfmt::skip]" {
            continue;
        }

        if line.trim().starts_with("#REPLACE ") {
            let what = line.trim().strip_prefix("#REPLACE ").unwrap();
            let mut split = what.split_terminator(" ");
            replace = Some(split.next().unwrap().to_string());
            let var = split.next().unwrap().to_string();
            for (key, value) in variables {
                if key == &var {
                    replacement = Some(value);
                    break;
                }
            }
        } else if line.trim().starts_with("//REPLACE ") {
            let what = line.trim().strip_prefix("//REPLACE ").unwrap();
            let mut split = what.split_terminator(" ");
            replace = Some(split.next().unwrap().to_string());
            let var = split.next().unwrap().to_string();
            for (key, value) in variables {
                if key == &var {
                    replacement = Some(value);
                    break;
                }
            }
        } else if line.trim().starts_with("#IF ") {
            let cond = line.trim().strip_prefix("#IF ").unwrap();
            if cond.starts_with("!") {
                include.push(!options.contains(&cond[1..].to_string()) && *include.last().unwrap());
            } else {
                include.push(options.contains(&cond.to_string()) && *include.last().unwrap());
            }
        } else if line.trim().starts_with("#ENDIF") {
            include.pop();
        } else if line.trim().starts_with("//IF ") {
            let cond = line.trim().strip_prefix("//IF ").unwrap();
            if cond.starts_with("!") {
                include.push(!options.contains(&cond[1..].to_string()) && *include.last().unwrap());
            } else {
                include.push(options.contains(&cond.to_string()) && *include.last().unwrap());
            }
        } else if line.trim().starts_with("//ENDIF") {
            include.pop();
        } else if *include.last().unwrap() {
            let mut line = line.to_string();

            if line.trim().starts_with("#+") {
                line = line.replace("#+", "").to_string();
            }

            if line.trim().starts_with("//+") {
                line = line.replace("//+", "").to_string();
            }

            if let (Some(replace), Some(replacement)) = (replace, replacement) {
                line = line.replace(&replace, &replacement);
            }

            res.push_str(&line);
            res.push('\n');

            replace = None;
            replacement = None;
        }
    }

    Some(res)
}
