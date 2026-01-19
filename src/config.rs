use esp_metadata::Chip;

use crate::template::{GeneratorOption, GeneratorOptionItem};

#[derive(Debug)]
pub struct ActiveConfiguration {
    /// The chip that is configured for
    pub chip: Chip,
    /// The names of the selected options
    pub selected: Vec<usize>,
    /// The tree of all available options
    pub options: Vec<GeneratorOptionItem>,
    /// All available option items (categories are not included), flattened to avoid the need for recursion.
    pub flat_options: Vec<GeneratorOption>,
}

pub fn flatten_options(options: &[GeneratorOptionItem]) -> Vec<GeneratorOption> {
    options
        .iter()
        .flat_map(|item| match item {
            GeneratorOptionItem::Category(category) => flatten_options(&category.options),
            GeneratorOptionItem::Option(option) => vec![option.clone()],
        })
        .collect()
}

impl ActiveConfiguration {
    pub fn is_group_selected(&self, group: &str) -> bool {
        self.selected
            .iter()
            .any(|s| self.flat_options[*s].selection_group == group)
    }

    pub fn is_selected(&self, option: &str) -> bool {
        self.selected_index(option).is_some()
    }

    pub fn selected_index(&self, option: &str) -> Option<usize> {
        self.selected
            .iter()
            .position(|s| self.flat_options[*s].name == option)
    }

    /// Tries to deselect all options in a selection group. Returns false if it's prevented by some
    /// requirement.
    fn deselect_group(selected: &mut Vec<usize>, options: &[GeneratorOption], group: &str) -> bool {
        // No group, nothing to deselect
        if group.is_empty() {
            return true;
        }

        // Avoid deselecting some options then failing.
        if !selected.iter().copied().all(|s| {
            let o = &options[s];
            if o.selection_group == group {
                // We allow deselecting group options because we are changing the options in the
                // group, so after this operation the group have a selected item still.
                Self::can_be_disabled_impl(selected, options, s, true)
            } else {
                true
            }
        }) {
            return false;
        }

        selected.retain(|s| options[*s].selection_group != group);

        true
    }

    pub fn select(&mut self, option: &str) {
        let (index, _o) = find_option(option, &self.flat_options, self.chip).unwrap();
        self.select_idx(index);
    }

    pub fn select_idx(&mut self, idx: usize) {
        let o = &self.flat_options[idx];
        if !self.is_option_active(o) {
            return;
        }
        if !Self::deselect_group(&mut self.selected, &self.flat_options, &o.selection_group) {
            return;
        }
        self.selected.push(idx);
    }

    /// Returns whether an item is active (can be selected).
    ///
    /// This function is different from `is_option_active` in that it handles categories as well.
    pub fn is_active(&self, item: &GeneratorOptionItem) -> bool {
        match item {
            GeneratorOptionItem::Category(category) => {
                if !self.requirements_met(&category.requires) {
                    return false;
                }
                for sub in category.options.iter() {
                    if self.is_active(sub) {
                        return true;
                    }
                }
                false
            }
            GeneratorOptionItem::Option(option) => self.is_option_active(option),
        }
    }

    /// Returns whether all requirements are met.
    ///
    /// A requirement may be:
    /// - an `option`
    /// - the absence of an `!option`
    /// - a `selection_group`, which means one option in that selection group must be selected
    /// - the absence of a `!selection_group`, which means no option in that selection group must
    ///   be selected
    ///
    /// A selection group must not have the same name as an option.
    fn requirements_met(&self, requires: &[String]) -> bool {
        for requirement in requires {
            let (key, expected) = if let Some(requirement) = requirement.strip_prefix('!') {
                (requirement, false)
            } else {
                (requirement.as_str(), true)
            };

            // Requirement is an option that must be selected?
            if self.is_selected(key) == expected {
                continue;
            }

            // Requirement is a group that must have a selected option?
            let is_group = Self::group_exists(key, &self.flat_options);
            if is_group && self.is_group_selected(key) == expected {
                continue;
            }

            return false;
        }

        true
    }

    /// Returns whether an option is active (can be selected).
    ///
    /// This involves checking if the option is available for the current chip, if it's not
    /// disabled by any other selected option, and if all its requirements are met.
    pub fn is_option_active(&self, option: &GeneratorOption) -> bool {
        if !option.chips.is_empty() && !option.chips.contains(&self.chip) {
            return false;
        }

        // Are this option's requirements met?
        if !self.requirements_met(&option.requires) {
            return false;
        }

        // Does any of the enabled options have a requirement against this one?
        for selected in self.selected.iter().copied() {
            let Some(selected_option) = self.flat_options.get(selected) else {
                ratatui::restore();
                panic!("selected option not found: {selected}");
            };

            for requirement in selected_option.requires.iter() {
                if let Some(requirement) = requirement.strip_prefix('!') {
                    if requirement == option.name {
                        return false;
                    }
                }
            }
        }

        true
    }

    // An option can only be disabled if it's not required by any other selected option.
    pub fn can_be_disabled(&self, option: &str) -> bool {
        let (option, _) = find_option(option, &self.flat_options, self.chip).unwrap();
        Self::can_be_disabled_impl(&self.selected, &self.flat_options, option, false)
    }

    fn can_be_disabled_impl(
        selected: &[usize],
        options: &[GeneratorOption],
        option: usize,
        allow_deselecting_group: bool,
    ) -> bool {
        let op = &options[option];
        for selected in selected.iter().copied() {
            let selected_option = &options[selected];
            if selected_option
                .requires
                .iter()
                .any(|o| o == &op.name || (o == &op.selection_group && !allow_deselecting_group))
            {
                return false;
            }
        }
        true
    }

    pub fn collect_relationships<'a>(
        &'a self,
        option: &'a GeneratorOptionItem,
    ) -> Relationships<'a> {
        let mut requires = Vec::new();
        let mut required_by = Vec::new();
        let mut disabled_by = Vec::new();

        self.selected.iter().for_each(|opt| {
            let opt = &self.flat_options[*opt];
            for o in opt.requires.iter() {
                if let Some(disables) = o.strip_prefix("!") {
                    if disables == option.name() {
                        disabled_by.push(opt.name.as_str());
                    }
                } else if o == option.name() {
                    required_by.push(opt.name.as_str());
                }
            }
        });
        for req in option.requires() {
            if let Some(disables) = req.strip_prefix("!") {
                if self.is_selected(disables) {
                    disabled_by.push(disables);
                }
            } else {
                requires.push(req.as_str());
            }
        }

        Relationships {
            requires,
            required_by,
            disabled_by,
        }
    }

    fn group_exists(key: &str, options: &[GeneratorOption]) -> bool {
        options.iter().any(|o| o.selection_group == key)
    }
}

pub struct Relationships<'a> {
    pub requires: Vec<&'a str>,
    pub required_by: Vec<&'a str>,
    pub disabled_by: Vec<&'a str>,
}

pub fn find_option<'c>(
    option: &str,
    options: &'c [GeneratorOption],
    chip: Chip,
) -> Option<(usize, &'c GeneratorOption)> {
    options
        .iter()
        .enumerate()
        .find(|(_, opt)| opt.name == option && (opt.chips.is_empty() || opt.chips.contains(&chip)))
}

#[cfg(test)]
mod test {
    use esp_metadata::Chip;

    use crate::{
        config::*,
        template::{GeneratorOption, GeneratorOptionCategory, GeneratorOptionItem},
    };

    #[test]
    fn required_by_and_requires_pick_the_right_options() {
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option1".to_string(),
                display_name: "Foobar".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["option2".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec![],
            }),
        ];
        let active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![0],
            flat_options: flatten_options(&options),
            options,
        };

        let rels = active.collect_relationships(&active.options[0]);
        assert_eq!(rels.requires, &["option2"]);
        assert_eq!(rels.required_by, <&[&str]>::default());

        let rels = active.collect_relationships(&active.options[1]);
        assert_eq!(rels.requires, <&[&str]>::default());
        assert_eq!(rels.required_by, &["option1"]);
    }

    #[test]
    fn selecting_one_in_group_deselects_other() {
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option1".to_string(),
                display_name: "Foobar".to_string(),
                selection_group: "group".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "group".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option3".to_string(),
                display_name: "Prevents deselecting option2".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["option2".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        active.select("option1");
        assert_eq!(active.selected, &[0]);

        active.select("option2");
        assert_eq!(active.selected, &[1]);

        // Enable option3, which prevents deselecting option2, which disallows selecting option1
        active.select("option3");
        assert_eq!(active.selected, &[1, 2]);

        active.select("option1");
        assert_eq!(active.selected, &[1, 2]);
    }

    #[test]
    fn depending_on_group_allows_changing_group_option() {
        let options = vec![
            GeneratorOptionItem::Category(GeneratorOptionCategory {
                name: "group-options".to_string(),
                display_name: "Group options".to_string(),
                help: "".to_string(),
                requires: vec![],
                options: vec![
                    GeneratorOptionItem::Option(GeneratorOption {
                        name: "option1".to_string(),
                        display_name: "Foobar".to_string(),
                        selection_group: "group".to_string(),
                        help: "".to_string(),
                        chips: vec![Chip::Esp32],
                        requires: vec![],
                    }),
                    GeneratorOptionItem::Option(GeneratorOption {
                        name: "option2".to_string(),
                        display_name: "Barfoo".to_string(),
                        selection_group: "group".to_string(),
                        help: "".to_string(),
                        chips: vec![Chip::Esp32],
                        requires: vec![],
                    }),
                ],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option3".to_string(),
                display_name: "Requires any in group to be selected".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["group".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option4".to_string(),
                display_name: "Extra option that depends on something".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["option3".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        // Nothing is selected in group, so option3 can't be selected
        active.select("option3");
        assert_eq!(active.selected, empty());

        active.select("option1");
        assert_eq!(active.selected, &[0]);

        active.select("option3");
        assert_eq!(active.selected, &[0, 2]);

        // The rejection algorithm must not trigger on unrelated options. This option is
        // meant to test the group filtering. It prevents disabling "option3" but it does not
        // belong to `group`, so it should not prevent selecting between "option1" or "option2".
        active.select("option4");
        assert_eq!(active.selected, &[0, 2, 3]);

        active.select("option2");
        assert_eq!(active.selected, &[2, 3, 1]);
    }

    #[test]
    fn depending_on_group_prevents_deselecting() {
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option1".to_string(),
                display_name: "Foobar".to_string(),
                selection_group: "group".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["group".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        active.select("option1");
        active.select("option2");

        // Option1 can't be deselected because option2 requires that a `group` option is selected
        assert!(!active.can_be_disabled("option1"));
    }

    #[test]
    fn requiring_not_option_only_rejects_existing_group() {
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option1".to_string(),
                display_name: "Foobar".to_string(),
                selection_group: "group".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["!option1".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        active.select("option1");
        let (_, opt2) = find_option("option2", &active.flat_options, Chip::Esp32).unwrap();
        assert!(!active.is_option_active(opt2));
    }

    fn empty() -> &'static [usize] {
        &[]
    }
}
