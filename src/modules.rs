use crate::Chip;

#[derive(Clone, Debug)]
pub struct Module {
    pub name: &'static str,
    pub display_name: &'static str,
    pub chip: Chip,
    pub remove_pins: &'static [&'static str],
}

pub const ESP32_MODULES: &[Module] = &[
    Module {
        name: "esp32-wroom-32e",
        display_name: "ESP32-WROOM-32E/32UE (4/8/16MB flash)",
        chip: Chip::Esp32,
        remove_pins: &["spi_flash", "esp32_pico_v3"],
    },
    Module {
        name: "esp32-wrover-e",
        display_name: "ESP32-WROVER-E/IE (8MB PSRAM)",
        chip: Chip::Esp32,
        remove_pins: &["spi_flash", "spi_psram", "esp32_pico_v3"],
    },
    Module {
        name: "esp32-mini-1",
        display_name: "ESP32-MINI-1 (4MB flash)",
        chip: Chip::Esp32,
        remove_pins: &["spi_flash", "esp32_pico_v3"],
    },
];

pub const ESP32C2_MODULES: &[Module] = &[Module {
    name: "esp32c2-mini-1",
    display_name: "ESP32-C2-MINI-1 (2/4MB flash)",
    chip: Chip::Esp32c2,
    remove_pins: &["spi_flash"],
}];

pub const ESP32C3_MODULES: &[Module] = &[
    Module {
        name: "esp32c3-wroom-02",
        display_name: "ESP32-C3-WROOM-02/02U (4MB flash)",
        chip: Chip::Esp32c3,
        remove_pins: &["spi_flash"],
    },
    Module {
        name: "esp32c3-mini-1",
        display_name: "ESP32-C3-MINI-1 (4MB flash)",
        chip: Chip::Esp32c3,
        remove_pins: &["spi_flash"],
    },
];

pub const ESP32C6_MODULES: &[Module] = &[
    Module {
        name: "esp32c6-wroom-1",
        display_name: "ESP32-C6-WROOM-1/1U (4MB flash)",
        chip: Chip::Esp32c6,
        remove_pins: &["spi_flash"],
    },
    Module {
        name: "esp32c6-mini-1",
        display_name: "ESP32-C6-MINI-1 (4/8MB flash)",
        chip: Chip::Esp32c6,
        remove_pins: &["spi_flash"],
    },
];

pub const ESP32H2_MODULES: &[Module] = &[
    Module {
        name: "esp32h2-wroom-02",
        display_name: "ESP32-H2-WROOM-02 (4MB flash)",
        chip: Chip::Esp32h2,
        remove_pins: &["spi_flash"],
    },
    Module {
        name: "esp32h2-mini-1",
        display_name: "ESP32-H2-MINI-1 (4MB flash)",
        chip: Chip::Esp32h2,
        remove_pins: &["spi_flash"],
    },
];

pub const ESP32S2_MODULES: &[Module] = &[
    Module {
        name: "esp32s2-wroom",
        display_name: "ESP32-S2-WROOM (4MB flash)",
        chip: Chip::Esp32s2,
        remove_pins: &["spi_flash"],
    },
    Module {
        name: "esp32s2-wrover",
        display_name: "ESP32-S2-WROVER (2MB PSRAM)",
        chip: Chip::Esp32s2,
        remove_pins: &["spi_flash", "spi_psram"],
    },
    Module {
        name: "esp32s2-mini-1",
        display_name: "ESP32-S2-MINI-1 (4MB flash)",
        chip: Chip::Esp32s2,
        remove_pins: &["spi_flash"],
    },
];

pub const ESP32S3_MODULES: &[Module] = &[
    Module {
        name: "esp32s3-wroom-1",
        display_name: "ESP32-S3-WROOM-1/1U (quad flash/PSRAM)",
        chip: Chip::Esp32s3,
        remove_pins: &["spi_flash"],
    },
    Module {
        name: "esp32s3-wroom-2",
        display_name: "ESP32-S3-WROOM-2 (octal flash/PSRAM)",
        chip: Chip::Esp32s3,
        remove_pins: &[],
    },
    Module {
        name: "esp32s3-mini-1",
        display_name: "ESP32-S3-MINI-1/1U (quad flash/PSRAM)",
        chip: Chip::Esp32s3,
        remove_pins: &[],
    },
];

use crate::template::GeneratorOptionItem;

/// Populates the module category in the template options with chip-specific modules.
pub fn populate_module_category(chip: Chip, options: &mut [GeneratorOptionItem]) {
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

        for module in chip.modules() {
            let mut opt = template_opt.clone();
            opt.name = module.name.to_string();
            opt.display_name = module.display_name.to_string();
            opt.selection_group = "module".to_string();
            category.options.push(GeneratorOptionItem::Option(opt));
        }

        break;
    }
}
