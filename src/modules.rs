use esp_metadata::Chip;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: &'static str,
    pub display_name: &'static str,
    pub chip: Chip,
    pub reserved_gpios: &'static [u8],
    pub octal_psram: bool,
}

pub const MODULES: &[Module] = &[
    // ESP32-C6 modules (in-package flash, SPI flash pins 24-26,28-30 not exposed in HAL)
    Module {
        name: "esp32c6-wroom-1",
        display_name: "ESP32-C6-WROOM-1 (4MB flash)",
        chip: Chip::Esp32c6,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32c6-wroom-1u",
        display_name: "ESP32-C6-WROOM-1U (4MB flash, U.FL)",
        chip: Chip::Esp32c6,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32c6-mini-1",
        display_name: "ESP32-C6-MINI-1 (4/8MB flash)",
        chip: Chip::Esp32c6,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32-S3 modules (SPI flash pins 26-32 not exposed in HAL)
    // Quad flash/PSRAM modules: no additional reserved pins
    // Octal flash/PSRAM modules: pins 33-37 connected to octal flash/PSRAM
    Module {
        name: "esp32s3-wroom-1",
        display_name: "ESP32-S3-WROOM-1 (quad flash/PSRAM)",
        chip: Chip::Esp32s3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32s3-wroom-1u",
        display_name: "ESP32-S3-WROOM-1U (quad flash/PSRAM, U.FL)",
        chip: Chip::Esp32s3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32s3-wroom-2",
        display_name: "ESP32-S3-WROOM-2 (octal flash/PSRAM)",
        chip: Chip::Esp32s3,
        reserved_gpios: &[33, 34, 35, 36, 37],
        octal_psram: true,
    },
    Module {
        name: "esp32s3-mini-1",
        display_name: "ESP32-S3-MINI-1 (quad flash/PSRAM)",
        chip: Chip::Esp32s3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32s3-mini-1u",
        display_name: "ESP32-S3-MINI-1U (quad flash/PSRAM, U.FL)",
        chip: Chip::Esp32s3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32-C3 modules (in-package flash, SPI flash pins 11-17 not exposed in HAL)
    Module {
        name: "esp32c3-wroom-02",
        display_name: "ESP32-C3-WROOM-02 (4MB flash)",
        chip: Chip::Esp32c3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32c3-wroom-02u",
        display_name: "ESP32-C3-WROOM-02U (4MB flash, U.FL)",
        chip: Chip::Esp32c3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32c3-mini-1",
        display_name: "ESP32-C3-MINI-1 (4MB flash)",
        chip: Chip::Esp32c3,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32 modules (SPI flash pins 6-11 not exposed in HAL)
    // WROVER-E has PSRAM on pins 16, 17 which need to be reserved
    Module {
        name: "esp32-wroom-32e",
        display_name: "ESP32-WROOM-32E (4/8/16MB flash)",
        chip: Chip::Esp32,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32-wroom-32ue",
        display_name: "ESP32-WROOM-32UE (4/8/16MB flash, U.FL)",
        chip: Chip::Esp32,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32-wrover-e",
        display_name: "ESP32-WROVER-E (8MB PSRAM)",
        chip: Chip::Esp32,
        reserved_gpios: &[16, 17],
        octal_psram: false,
    },
    Module {
        name: "esp32-mini-1",
        display_name: "ESP32-MINI-1 (4MB flash)",
        chip: Chip::Esp32,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32-S2 modules (SPI flash pins 26-32 not exposed in HAL)
    Module {
        name: "esp32s2-wroom",
        display_name: "ESP32-S2-WROOM (4MB flash)",
        chip: Chip::Esp32s2,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32s2-wrover",
        display_name: "ESP32-S2-WROVER (2MB PSRAM)",
        chip: Chip::Esp32s2,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32s2-mini-1",
        display_name: "ESP32-S2-MINI-1 (4MB flash)",
        chip: Chip::Esp32s2,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32-C2 modules (SPI flash pins 11-17 not exposed in HAL)
    Module {
        name: "esp32c2-mini-1",
        display_name: "ESP32-C2-MINI-1 (2/4MB flash)",
        chip: Chip::Esp32c2,
        reserved_gpios: &[],
        octal_psram: false,
    },
    // ESP32-H2 modules (in-package flash, no reserved GPIOs)
    Module {
        name: "esp32h2-wroom-02",
        display_name: "ESP32-H2-WROOM-02 (4MB flash)",
        chip: Chip::Esp32h2,
        reserved_gpios: &[],
        octal_psram: false,
    },
    Module {
        name: "esp32h2-mini-1",
        display_name: "ESP32-H2-MINI-1 (4MB flash)",
        chip: Chip::Esp32h2,
        reserved_gpios: &[],
        octal_psram: false,
    },
];

pub fn modules_for_chip(chip: Chip) -> Vec<&'static Module> {
    MODULES.iter().filter(|m| m.chip == chip).collect()
}

pub fn find_module(name: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.name == name)
}

use crate::template::GeneratorOptionItem;

/// Populates the module category in the template options with chip-specific modules.
pub fn populate_module_category(chip: Chip, options: &mut [GeneratorOptionItem]) {
    let modules = modules_for_chip(chip);

    for item in options.iter_mut() {
        let GeneratorOptionItem::Category(category) = item else {
            continue;
        };
        if category.name != "module" {
            continue;
        }

        let template_opt = match category.options.first() {
            Some(GeneratorOptionItem::Option(opt)) => opt.clone(),
            _ => {
                panic!("module category must contain a placeholder !Option");
            }
        };

        category.options.clear();

        let mut opt = template_opt.clone();
        opt.name = "generic".to_string();
        opt.display_name = "Generic/unknown (no GPIO reservations)".to_string();
        opt.selection_group = "module".to_string();
        category.options.push(GeneratorOptionItem::Option(opt));

        for module in modules {
            let mut opt = template_opt.clone();
            opt.name = module.name.to_string();
            opt.display_name = module.display_name.to_string();
            opt.selection_group = "module".to_string();
            category.options.push(GeneratorOptionItem::Option(opt));
        }

        break;
    }
}
