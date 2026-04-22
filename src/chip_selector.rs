use std::collections::HashSet;

use strum::IntoEnumIterator;

use crate::Chip;
use esp_generate::template::GeneratorOptionItem;

/// Startup-time sanity check for the `chip` category in the parsed template.
///
/// The template is the source of truth for which chips the generator offers
/// and what per-chip template variables they contribute (via `sets`). The
/// [`Chip`] enum is the source of truth for the hardware backends
/// (`esp-metadata-generated`, memory layouts, pins, etc.).
///
/// These two lists must stay in lockstep, so this validator asserts — once,
/// at program start — that:
///
///   1. The template contains exactly one top-level category named `chip`.
///   2. Every entry under it is an `!Option` with `selection_group: chip`.
///   3. Every option's `name` parses to a [`Chip`] variant.
///   4. Every [`Chip`] variant appears in the category.
///
/// Violations are programming errors (someone edited `template.yaml` and
/// forgot the enum, or vice versa), so the return value is phrased as a
/// human-readable `Err(String)` intended to be surfaced via `panic!` /
/// `expect` from the `TEMPLATE` initializer rather than handled dynamically.
pub fn validate_chip_category(options: &[GeneratorOptionItem]) -> Result<(), String> {
    let chip_category = options
        .iter()
        .filter_map(|item| match item {
            GeneratorOptionItem::Category(c) if c.name == "chip" => Some(c),
            _ => None,
        })
        .collect::<Vec<_>>();

    match chip_category.as_slice() {
        [] => return Err("template has no `chip` category".to_string()),
        [_] => {}
        _ => return Err("template has multiple `chip` categories".to_string()),
    }
    let chip_category = chip_category[0];

    let mut seen: HashSet<Chip> = HashSet::new();
    for item in &chip_category.options {
        let option = match item {
            GeneratorOptionItem::Option(o) => o,
            GeneratorOptionItem::Category(c) => {
                return Err(format!(
                    "`chip` category must only contain !Option entries, found nested category `{}`",
                    c.name
                ));
            }
        };

        if option.selection_group != "chip" {
            return Err(format!(
                "chip option `{}` must have `selection_group: chip` (found `{}`)",
                option.name, option.selection_group
            ));
        }

        let chip: Chip = option.name.parse().map_err(|_| {
            format!(
                "chip option `{}` does not match any `Chip` enum variant",
                option.name
            )
        })?;

        if !seen.insert(chip) {
            return Err(format!("chip `{}` is listed more than once", option.name));
        }
    }

    for chip in Chip::iter() {
        if !seen.contains(&chip) {
            return Err(format!(
                "`Chip::{chip:?}` has no entry in the `chip` category of `template.yaml`"
            ));
        }
    }

    Ok(())
}

/// Startup-time sanity check for the `module` category in the parsed
/// template.
///
/// Unlike the `chip` category, there is no enum backing modules — they are
/// authored purely in YAML and identified by name. This validator enforces
/// the minimum structural invariants the generator relies on:
///
///   1. At most one top-level category named `module` exists (zero is
///      tolerated so `template.yaml` can, in principle, opt out entirely).
///   2. Every entry under it is an `!Option` (no nested categories).
///   3. Every option has `selection_group: module`, so the
///      mutual-exclusion logic in [`ActiveConfiguration`] treats them as a
///      single group.
///   4. Every option's `compatible.chip` allow-list contains only chips
///      that actually exist in the [`Chip`] enum — otherwise
///      `prune_incompatible_options` would silently drop the option on
///      every chip, making it unreachable.
///   5. Names are unique (duplicate names would confuse
///      `find_option`-based lookups).
///
/// `sets.remove_pins` entries are not validated here: the `limitations`
/// strings they match live in `esp_metadata_generated` and are chip-dependent,
/// so a typo only ever misfires on the specific chip(s) that own the module.
/// Catching that would need a chip-crossed check that's better expressed in
/// a dedicated test than in a generic validator.
///
/// [`ActiveConfiguration`]: crate::config::ActiveConfiguration
pub fn validate_module_category(options: &[GeneratorOptionItem]) -> Result<(), String> {
    let module_category = options.iter().find_map(|item| match item {
        GeneratorOptionItem::Category(c) if c.name == "module" => Some(c),
        _ => None,
    });
    let Some(module_category) = module_category else {
        return Ok(());
    };

    let valid_chip_names: HashSet<String> = Chip::iter().map(|c| c.to_string()).collect();
    let mut seen_names: HashSet<&str> = HashSet::new();

    for item in &module_category.options {
        let option = match item {
            GeneratorOptionItem::Option(o) => o,
            GeneratorOptionItem::Category(c) => {
                return Err(format!(
                    "`module` category must only contain !Option entries, found nested category `{}`",
                    c.name
                ));
            }
        };

        if option.selection_group != "module" {
            return Err(format!(
                "module option `{}` must have `selection_group: module` (found `{}`)",
                option.name, option.selection_group
            ));
        }

        if !seen_names.insert(option.name.as_str()) {
            return Err(format!(
                "module option name `{}` is listed more than once",
                option.name
            ));
        }

        if let Some(allowed) = option.compatible.get("chip") {
            for chip_name in allowed {
                if !valid_chip_names.contains(chip_name) {
                    return Err(format!(
                        "module option `{}` lists unknown chip `{}` in `compatible.chip`",
                        option.name, chip_name
                    ));
                }
            }
        }

        // `remove_pins` is semantically a list of limitation tags; the
        // pin-reservation code in `main.rs` calls `SetValue::as_list` and
        // silently treats a scalar as empty. Catching the shape mismatch
        // here turns an easy YAML slip (`remove_pins: spi_flash` rather
        // than `remove_pins: [spi_flash]`) into a loud startup error.
        if let Some(value) = option.sets.get("remove_pins") {
            if value.as_list().is_none() {
                return Err(format!(
                    "module option `{}` has non-list `sets.remove_pins`; \
                     use a sequence like `[spi_flash, spi_psram]`",
                    option.name
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::TEMPLATE;

    #[test]
    fn bundled_template_chip_category_is_consistent_with_chip_enum() {
        // Guards against someone adding a Chip variant without updating the
        // template (or vice versa). The library crate can't reach the
        // binary's `TEMPLATE` LazyLock, so we parse the bundled bytes
        // directly — they're the same file that ships.
        validate_chip_category(&TEMPLATE.options).expect("bundled template is invalid");
    }

    #[test]
    fn bundled_template_module_category_passes_validation() {
        validate_module_category(&TEMPLATE.options).expect("bundled template is invalid");
    }

    #[test]
    fn every_chip_has_at_least_one_module() {
        // Not a hard invariant of the schema — the generator is happy with
        // a chip that has zero modules (the `module` category just gets
        // pruned away for that chip). But it *is* a product-level
        // expectation, and keeping the assertion here makes accidental
        // regressions (e.g. forgetting modules for a newly added chip)
        // loud rather than silent.
        let module_category = TEMPLATE
            .options
            .iter()
            .find_map(|item| match item {
                GeneratorOptionItem::Category(c) if c.name == "module" => Some(c),
                _ => None,
            })
            .expect("module category is required by the generator");

        for chip in Chip::iter() {
            let chip_name = chip.to_string();
            let has_module = module_category.options.iter().any(|item| {
                let GeneratorOptionItem::Option(o) = item else {
                    return false;
                };
                o.compatible
                    .get("chip")
                    .is_some_and(|list| list.iter().any(|n| n == &chip_name))
            });
            assert!(has_module, "no modules declared for {chip_name}");
        }
    }
}
