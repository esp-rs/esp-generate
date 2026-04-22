use std::collections::HashSet;

use strum::IntoEnumIterator;

use crate::Chip;
use crate::template::GeneratorOptionItem;

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
    use crate::template::Template;

    #[test]
    fn bundled_template_chip_category_is_consistent_with_chip_enum() {
        // Guards against someone adding a Chip variant without updating the
        // template (or vice versa). The library crate can't reach the
        // binary's `TEMPLATE` LazyLock, so we parse the bundled bytes
        // directly — they're the same file that ships.
        let template_yaml = include_str!("../template/template.yaml");
        let template: Template =
            serde_yaml::from_str(template_yaml).expect("bundled template.yaml must parse");
        validate_chip_category(&template.options).expect("bundled template is invalid");
    }
}
