use strum::IntoEnumIterator;

use crate::Chip;
use crate::template::GeneratorOptionItem;

/// Populates the `chip` category in the template options with one entry per
/// supported [`Chip`], all sharing the `chip` selection group so only one can
/// be active at a time.
///
/// This mirrors [`esp_generate::modules::populate_module_category`] and
/// [`crate::toolchain::ToolchainCategory::populate`]: a single `PLACEHOLDER`
/// `!Option` in the YAML is replaced with the real set on every build.
/// `build_options_for_chip` re-runs this on every chip switch, so the populate
/// step must stay idempotent — meaning it operates on a fresh template clone
/// and doesn't care about any previous state of the category.
///
/// The entries' `name` is `chip.to_string()` (e.g. `esp32c6`), which matches
/// what `#IF option("esp32c6")` in template files expects and what
/// `compatible: { chip: [...] }` allow-lists compare against.
pub fn populate_chip_category(options: &mut [GeneratorOptionItem]) {
    for item in options.iter_mut() {
        let GeneratorOptionItem::Category(category) = item else {
            continue;
        };
        if category.name != "chip" {
            continue;
        }

        let template_opt = match category.options.first() {
            Some(GeneratorOptionItem::Option(opt)) => opt.clone(),
            _ => panic!("chip category must contain a placeholder !Option"),
        };

        category.options.clear();

        for chip in Chip::iter() {
            let mut opt = template_opt.clone();
            opt.name = chip.to_string();
            opt.display_name = chip.to_string();
            opt.selection_group = "chip".to_string();
            category.options.push(GeneratorOptionItem::Option(opt));
        }

        break;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::template::{GeneratorOption, GeneratorOptionCategory};
    use indexmap::IndexMap;

    fn placeholder_tree() -> Vec<GeneratorOptionItem> {
        vec![GeneratorOptionItem::Category(GeneratorOptionCategory {
            name: "chip".to_string(),
            display_name: "Chip".to_string(),
            help: String::new(),
            requires: Vec::new(),
            options: vec![GeneratorOptionItem::Option(GeneratorOption {
                name: "PLACEHOLDER".to_string(),
                display_name: "<dynamic>".to_string(),
                selection_group: "chip".to_string(),
                help: String::new(),
                requires: Vec::new(),
                compatible: IndexMap::new(),
            })],
        })]
    }

    #[test]
    fn populate_expands_placeholder_into_one_option_per_chip() {
        let mut tree = placeholder_tree();
        populate_chip_category(&mut tree);

        let GeneratorOptionItem::Category(category) = &tree[0] else {
            panic!("expected category");
        };

        let expected: Vec<String> = Chip::iter().map(|c| c.to_string()).collect();
        let actual: Vec<String> = category
            .options
            .iter()
            .map(|item| match item {
                GeneratorOptionItem::Option(o) => o.name.clone(),
                _ => panic!("chip category must only hold options"),
            })
            .collect();
        assert_eq!(actual, expected);

        for item in &category.options {
            let GeneratorOptionItem::Option(o) = item else {
                unreachable!();
            };
            assert_eq!(o.selection_group, "chip");
            assert!(
                !o.compatible.contains_key("chip"),
                "chip-group options must not restrict themselves by chip (they drive the restriction)"
            );
        }
    }

    #[test]
    fn populate_is_idempotent() {
        // Matters because `build_options_for_chip` runs this on every chip
        // switch against a fresh clone — but tests (and any future caller
        // that reuses a tree) must not see the list grow across repeated
        // populations. Here we emulate that by priming with the placeholder
        // and invoking populate twice.
        let mut tree = placeholder_tree();
        populate_chip_category(&mut tree);
        let first_len = match &tree[0] {
            GeneratorOptionItem::Category(c) => c.options.len(),
            _ => unreachable!(),
        };

        // Second population needs a tree that still starts with a valid
        // first option — `populate` uses `options.first()` as its template.
        // After the first call, the first option is a real chip option with
        // `selection_group = "chip"`, which is fine to clone from.
        populate_chip_category(&mut tree);
        let second_len = match &tree[0] {
            GeneratorOptionItem::Category(c) => c.options.len(),
            _ => unreachable!(),
        };

        assert_eq!(first_len, second_len);
    }
}
