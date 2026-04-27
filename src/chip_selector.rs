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
