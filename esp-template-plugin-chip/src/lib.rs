//! Espressif chip data, as a versioned [`TemplatePlugin`]: the chip list, the
//! `chip` selection-group fragment, and the `chip.…` facts from
//! `esp-metadata-generated`.
//!
//! **This crate's version is the vocabulary**, so bumping the metadata
//! dependency is a version bump here every time, or the number stops meaning
//! anything. A host keeps older templates working by depending on an older
//! version of this crate as well and registering both.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::sync::LazyLock;

use esp_metadata_generated::MemoryRegion;
use esp_template_sdk::contract::{Version, release};
use esp_template_sdk::plugin::{Selection, TemplatePlugin};
use esp_template_sdk::process::{FactValue, Facts, StructFacts};
use indexmap::IndexMap;
use strum::IntoEnumIterator;

/// The name a template pins in `[plugins]`, and the `plugin:` fragment name.
/// Reachable through [`TemplatePlugin::name`].
const NAME: &str = "chip";

/// Mirrors the metadata version — see the module docs. Reachable through
/// [`TemplatePlugin::version`].
const VERSION: Version = release(0, 5, 0);

/// The selection group the chip options belong to.
const GROUP: &str = "chip";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::Display, strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Chip {
    Esp32,
    Esp32c2,
    Esp32c3,
    Esp32c5,
    Esp32c6,
    Esp32c61,
    Esp32h2,
    Esp32s2,
    Esp32s3,
    Esp32s31,
}

impl Chip {
    fn metadata(self) -> esp_metadata_generated::Chip {
        match self {
            Chip::Esp32 => esp_metadata_generated::Chip::Esp32,
            Chip::Esp32c2 => esp_metadata_generated::Chip::Esp32c2,
            Chip::Esp32c3 => esp_metadata_generated::Chip::Esp32c3,
            Chip::Esp32c5 => esp_metadata_generated::Chip::Esp32c5,
            Chip::Esp32c6 => esp_metadata_generated::Chip::Esp32c6,
            Chip::Esp32c61 => esp_metadata_generated::Chip::Esp32c61,
            Chip::Esp32h2 => esp_metadata_generated::Chip::Esp32h2,
            Chip::Esp32s2 => esp_metadata_generated::Chip::Esp32s2,
            Chip::Esp32s3 => esp_metadata_generated::Chip::Esp32s3,
            Chip::Esp32s31 => esp_metadata_generated::Chip::Esp32s31,
        }
    }

    fn dram2_region(self) -> &'static MemoryRegion {
        self.metadata()
            .memory_layout()
            .region("dram2_uninit")
            .expect("All chips should have a dram2_uninit region")
    }

    /// Every GPIO pin, with the limitation tags that apply to it.
    fn pins(self) -> impl Iterator<Item = Pin> + Clone {
        self.metadata().pins().iter().map(|pin| Pin {
            number: pin.pin,
            limitations: pin.limitations,
        })
    }

    /// The Rust target triple, also exposed to templates as `chip.rust_target`.
    pub fn rust_target(self) -> &'static str {
        self.metadata().target()
    }

    /// Whether this chip is Xtensa rather than RISC-V.
    pub fn is_xtensa(self) -> bool {
        self.metadata().is_xtensa()
    }

    /// The chip's name as a person writes it.
    ///
    /// Not derivable from the variant: `Esp32c61` is "ESP32-C61", and the
    /// hyphenation doesn't follow from the kebab-case option name either.
    pub fn display_name(self) -> &'static str {
        match self {
            Chip::Esp32 => "ESP32",
            Chip::Esp32c2 => "ESP32-C2",
            Chip::Esp32c3 => "ESP32-C3",
            Chip::Esp32c5 => "ESP32-C5",
            Chip::Esp32c6 => "ESP32-C6",
            Chip::Esp32c61 => "ESP32-C61",
            Chip::Esp32h2 => "ESP32-H2",
            Chip::Esp32s2 => "ESP32-S2",
            Chip::Esp32s3 => "ESP32-S3",
            Chip::Esp32s31 => "ESP32-S31",
        }
    }

    /// This chip's `chip.…` fields. Cheap: a refcounted share of a map built
    /// once per process.
    fn facts(self) -> Facts {
        Facts {
            structs: IndexMap::from([(NAME.to_string(), CHIP_FACTS[&self].clone())]),
            ..Default::default()
        }
    }
}

/// Every chip's `chip.…` fields, built once per process.
///
/// The symbol union spans every chip, so building one chip's map does most of
/// the work of building all nine. Not a `const`: an `IndexMap` can't be built
/// in const context.
static CHIP_FACTS: LazyLock<HashMap<Chip, StructFacts>> = LazyLock::new(|| {
    // A symbol is either a bare flag (`riscv`) or a `name="value"` pair
    // (`bt_controller="npl"`). Classify by name across all chips so a field's
    // type doesn't change with the selection.
    let mut valued: HashSet<&'static str> = HashSet::new();
    let mut every_symbol: BTreeSet<&'static str> = BTreeSet::new();
    for chip in Chip::iter() {
        for symbol in chip.metadata().all_symbols() {
            match symbol.split_once('=') {
                Some((name, _)) => {
                    valued.insert(name);
                    every_symbol.insert(name);
                }
                None => {
                    every_symbol.insert(symbol);
                }
            }
        }
    }

    Chip::iter()
        .map(|chip| {
            let metadata = chip.metadata();
            let mut mine: HashMap<&str, Option<&str>> = HashMap::new();
            for symbol in metadata.all_symbols() {
                match symbol.split_once('=') {
                    Some((name, value)) => mine.insert(name, Some(value.trim_matches('"'))),
                    None => mine.insert(symbol, None),
                };
            }

            let mut fields: IndexMap<Box<str>, FactValue> = every_symbol
                .iter()
                .map(|symbol| {
                    let value: FactValue = if valued.contains(symbol) {
                        mine.get(symbol).and_then(|v| *v).unwrap_or("").into()
                    } else {
                        mine.contains_key(symbol).into()
                    };
                    ((*symbol).into(), value)
                })
                .collect();

            // Last, so a metadata symbol can never displace one.
            fields.insert("name".into(), chip.to_string().into());
            fields.insert("rust_target".into(), metadata.target().into());
            fields.insert(
                "dram2_uninit_size".into(),
                FactValue::Int(chip.dram2_region().size() as u64),
            );

            (chip, StructFacts::new(fields))
        })
        .collect()
});

/// One GPIO pin and the limitation tags that apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pin {
    number: usize,
    limitations: &'static [&'static str],
}

impl Pin {
    fn has_limitation(&self, tag: &str) -> bool {
        self.limitations.contains(&tag)
    }
}

/// The `reserved_gpio_code` block, and whether it reserves anything.
///
/// `remove_pins` names limitation tags the selected module puts beyond use. The
/// generated source is chip-derived, so it belongs here rather than in a host
/// that would need the pin table to build it.
fn reserved_gpio(chip: Chip, remove_pins: &[String]) -> (String, bool) {
    let mut code = String::new();
    if remove_pins.is_empty() {
        return (code, false);
    }

    let restricted = chip
        .pins()
        .filter(|pin| remove_pins.iter().any(|tag| pin.has_limitation(tag)));
    let strapping: Vec<Pin> = chip
        .pins()
        .filter(|pin| pin.has_limitation("strapping"))
        .collect();

    if !strapping.is_empty() {
        let list = strapping
            .iter()
            .map(|pin| format!("// - GPIO{}", pin.number))
            .collect::<Vec<_>>()
            .join("\n");
        writeln!(
            &mut code,
            r#"// The following pins are used to bootstrap the chip. They are available
                    // for use, but check the datasheet of the module for more information on them.
                    {list}"#
        )
        .unwrap();
    }

    let mut has_reserved = false;
    if restricted.clone().next().is_some() {
        has_reserved = true;
        let plucker = restricted
            .map(|pin| format!("    let _gpio{0} = peripherals.GPIO{0};", pin.number))
            .collect::<Vec<_>>()
            .join("\n");
        writeln!(
            &mut code,
            r#"// These GPIO pins are in use by some feature of the module and should not be used.
                {plucker}"#
        )
        .unwrap();
    }

    (code, has_reserved)
}

/// The chip plugin: the `chip` selection group and the `chip.…` facts.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChipPlugin;

impl TemplatePlugin for ChipPlugin {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> Version {
        VERSION
    }

    /// The `chip` selection group, generated from [`Chip`] so the enum and the
    /// option list cannot drift. The only fragment this plugin offers.
    fn fragment(&self, name: &str) -> Option<String> {
        if name != NAME {
            return None;
        }

        let mut yaml = String::from(
            "!Category\n\
             name: chip\n\
             display_name: Chip selection\n\
             help: Target Espressif chip. Switching chips deselects and hides incompatible options.\n\
             options:\n",
        );

        for chip in Chip::iter() {
            yaml.push_str(&format!(
                "  - !Option\n    \
                   name: {chip}\n    \
                   display_name: {}\n    \
                   selection_group: {GROUP}\n",
                chip.display_name()
            ));
        }

        Some(yaml)
    }

    /// The selected chip's facts, or none while no chip is picked — which makes
    /// any `chip.…` reference an error rather than a silent false.
    fn facts(&self, selection: &Selection) -> Facts {
        let Some(chip) = selection.group(GROUP).and_then(|n| n.parse::<Chip>().ok()) else {
            return Facts::default();
        };

        let (code, has_reserved) = reserved_gpio(chip, selection.set_list("remove_pins"));

        let mut facts = chip.facts();
        facts.set_value("reserved_gpio_code", code);
        facts.set_value("has_reserved_pins", has_reserved);
        facts
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_version_tracks_the_metadata_it_wraps() {
        assert_eq!(VERSION.to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn every_chip_is_offered_in_the_chip_group() {
        let fragment = ChipPlugin.fragment(NAME).expect("the chip plugin has one");
        for chip in Chip::iter() {
            assert!(
                fragment.contains(&format!("name: {chip}\n")),
                "{chip} is missing from the fragment"
            );
            assert!(
                fragment.contains(chip.display_name()),
                "{chip} display name"
            );
        }
        assert_eq!(
            fragment.matches("!Option").count(),
            Chip::iter().count(),
            "one option per chip, no more"
        );

        assert_eq!(ChipPlugin.fragment("anything_else"), None);
    }

    fn picked(chip: Option<&str>) -> Selection {
        Selection {
            options: chip
                .into_iter()
                .map(str::to_string)
                .chain(["alloc".to_string()])
                .collect(),
            groups: chip
                .map(|c| (GROUP.to_string(), c.to_string()))
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn facts_follow_the_selected_chip() {
        let facts = ChipPlugin.facts(&picked(Some("esp32c6")));
        let chip = facts
            .structs
            .get(NAME)
            .expect("a selected chip yields chip facts");
        assert_eq!(
            chip.fields().get("name"),
            Some(&FactValue::Str("esp32c6".into()))
        );
    }

    #[test]
    fn no_chip_selected_means_no_chip_facts() {
        assert!(ChipPlugin.facts(&picked(None)).structs.is_empty());
    }

    #[test]
    fn a_chip_name_this_plugin_does_not_know_yields_no_facts() {
        let unknown = Selection {
            groups: [(GROUP.to_string(), "esp32z9".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        assert!(ChipPlugin.facts(&unknown).structs.is_empty());
    }

    /// An option named after a chip, outside the `chip` group, is not the pick.
    #[test]
    fn only_the_chip_group_names_the_chip() {
        let impostor = Selection {
            options: vec!["esp32c6".to_string()],
            ..Default::default()
        };
        assert!(ChipPlugin.facts(&impostor).structs.is_empty());
    }

    /// A capability a chip merely lacks must be present and falsy, or a
    /// portable `chip.soc_has_wifi` would be an unknown field.
    #[test]
    fn every_chip_carries_every_chips_symbols() {
        let all: BTreeSet<Box<str>> = Chip::iter()
            .flat_map(|c| CHIP_FACTS[&c].fields().keys().cloned())
            .collect();

        for chip in Chip::iter() {
            let mine = CHIP_FACTS[&chip].fields();
            for symbol in &all {
                assert!(mine.contains_key(symbol), "{chip} is missing `{symbol}`");
            }
            assert!(
                !mine.contains_key("soc_has_wfi"),
                "a misspelling must stay absent, so somni can report it"
            );
        }
    }

    #[test]
    fn the_named_scalars_are_populated_on_every_chip() {
        for chip in Chip::iter() {
            let f = CHIP_FACTS[&chip].fields();
            let str_field = |k: &str| match f.get(k) {
                Some(FactValue::Str(v)) => v.as_str(),
                other => panic!("{chip}.{k} must be a string, got {other:?}"),
            };

            assert_eq!(str_field("name"), chip.to_string());
            assert_eq!(str_field("rust_target"), chip.metadata().target());
            assert_eq!(
                f.get("dram2_uninit_size"),
                Some(&FactValue::Int(chip.dram2_region().size() as u64))
            );
        }
    }

    #[test]
    fn pins_carry_numbers_and_limitation_tags() {
        let strapping: Vec<usize> = Chip::Esp32s3
            .pins()
            .filter(|pin| pin.has_limitation("strapping"))
            .map(|pin| pin.number)
            .collect();
        assert!(
            !strapping.is_empty(),
            "a chip with no strapping pins would silently drop that comment"
        );

        assert_eq!(
            Chip::Esp32s3
                .pins()
                .filter(|p| p.has_limitation("nope"))
                .count(),
            0
        );

        for chip in Chip::iter() {
            let pins = chip.pins();
            assert!(pins.clone().count() > 0, "{chip} exposes no pins");
            assert_eq!(pins.clone().count(), pins.count());
        }
    }

    #[test]
    fn chip_symbols_are_chip_specific() {
        let c6 = CHIP_FACTS[&Chip::Esp32c6].fields().clone();
        let h2 = CHIP_FACTS[&Chip::Esp32h2].fields().clone();

        assert_eq!(c6.get("soc_has_wifi"), Some(&FactValue::Bool(true)));
        assert_eq!(
            h2.get("soc_has_wifi"),
            Some(&FactValue::Bool(false)),
            "ESP32-H2 has no Wi-Fi"
        );
    }

    #[test]
    fn chip_symbols_cover_the_isa() {
        for chip in Chip::iter() {
            let f = CHIP_FACTS[&chip].fields();
            let xtensa = f.get("xtensa") == Some(&FactValue::Bool(true));
            let riscv = f.get("riscv") == Some(&FactValue::Bool(true));
            assert_ne!(xtensa, riscv, "{chip} is exactly one of Xtensa or RISC-V");
            assert_eq!(xtensa, chip.metadata().is_xtensa());
        }
    }

    /// Some symbols are `name="value"` pairs. Their type must not change with
    /// the selected chip, or a comparison that works on one chip becomes a type
    /// error on another.
    #[test]
    fn valued_symbols_stay_strings_on_every_chip() {
        for chip in Chip::iter() {
            let f = CHIP_FACTS[&chip].fields();
            assert!(
                matches!(f.get("bt_controller"), Some(FactValue::Str(_))),
                "{chip} must expose `bt_controller` as a string"
            );
            if let Some(FactValue::Str(v)) = f.get("bt_controller") {
                assert!(!v.contains('"'), "{chip} left quotes in `{v}`");
            }
        }
    }
}
