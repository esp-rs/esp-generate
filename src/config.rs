use esp_metadata::Chip;

use crate::template::{GeneratorOption, GeneratorOptionItem};

pub struct ActiveConfiguration<'c> {
    /// The chip that is configured for
    pub chip: Chip,
    /// The names of the selected options
    pub selected: Vec<String>,
    /// All available options
    pub options: &'c [GeneratorOptionItem],
}

impl ActiveConfiguration<'_> {
    pub fn is_group_selected(&self, group: &str) -> bool {
        self.selected.iter().any(|s| {
            let option = find_option(s, self.options).unwrap();
            option.selection_group == group
        })
    }

    pub fn is_selected(&self, option: &str) -> bool {
        self.selected_index(option).is_some()
    }

    pub fn selected_index(&self, option: &str) -> Option<usize> {
        self.selected.iter().position(|s| s == option)
    }

    /// Tries to deselect all options in a selection gropu. Returns false if it's prevented by some
    /// requirement.
    fn deselect_group(
        selected: &mut Vec<String>,
        options: &[GeneratorOptionItem],
        group: &str,
    ) -> bool {
        // No group, nothing to deselect
        if group.is_empty() {
            return true;
        }

        // Avoid deselecting some options then failing.
        if !selected
            .iter()
            .all(|s| Self::can_be_disabled_impl(selected, options, s))
        {
            return false;
        }

        selected.retain(|s| {
            let option = find_option(s, options).unwrap();
            option.selection_group != group
        });

        true
    }

    pub fn select(&mut self, option: String) {
        let o = find_option(&option, self.options).unwrap();
        if !self.requirements_met(o) {
            return;
        }
        if !Self::deselect_group(&mut self.selected, self.options, &o.selection_group) {
            return;
        }
        self.selected.push(option);
    }

    pub fn is_active(&self, item: &GeneratorOptionItem) -> bool {
        match item {
            GeneratorOptionItem::Category(options) => {
                for sub in options.options.iter() {
                    if self.is_active(sub) {
                        return true;
                    }
                }
                false
            }
            GeneratorOptionItem::Option(option) => self.requirements_met(option),
        }
    }

    pub fn requirements_met(&self, option: &GeneratorOption) -> bool {
        if !option.chips.is_empty() && !option.chips.contains(&self.chip) {
            return false;
        }

        // Are this option's requirements met?
        for requirement in option.requires.iter() {
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
            if self.is_group_selected(key) == expected {
                continue;
            }

            return false;
        }

        // Does any of the enabled options have a requirement against this one?
        for selected in self.selected.iter() {
            let Some(selected_option) = find_option(selected, self.options) else {
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
        Self::can_be_disabled_impl(&self.selected, self.options, option)
    }

    fn can_be_disabled_impl(
        selected: &[String],
        options: &[GeneratorOptionItem],
        option: &str,
    ) -> bool {
        for selected in selected.iter() {
            let selected_option = find_option(selected, options).unwrap();
            if selected_option.requires.iter().any(|o| o == option) {
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
            let opt = find_option(opt.as_str(), self.options).unwrap();
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
}

pub struct Relationships<'a> {
    pub requires: Vec<&'a str>,
    pub required_by: Vec<&'a str>,
    pub disabled_by: Vec<&'a str>,
}

pub fn find_option<'c>(
    option: &str,
    options: &'c [GeneratorOptionItem],
) -> Option<&'c GeneratorOption> {
    for item in options {
        match item {
            GeneratorOptionItem::Category(category) => {
                let found_option = find_option(option, &category.options);
                if found_option.is_some() {
                    return found_option;
                }
            }
            GeneratorOptionItem::Option(item) => {
                if item.name == option {
                    return Some(item);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod test {
    use esp_metadata::Chip;

    use crate::{
        config::ActiveConfiguration,
        template::{GeneratorOption, GeneratorOptionItem},
    };

    #[test]
    fn required_by_and_requires_pick_the_right_options() {
        let options = &[
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
            selected: vec!["option1".to_string()],
            options,
        };

        let rels = active.collect_relationships(&options[0]);
        assert_eq!(rels.requires, &["option2"]);
        assert_eq!(rels.required_by, <&[&str]>::default());

        let rels = active.collect_relationships(&options[1]);
        assert_eq!(rels.requires, <&[&str]>::default());
        assert_eq!(rels.required_by, &["option1"]);
    }

    #[test]
    fn selecting_one_in_group_deselects_other() {
        let options = &[
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
            options,
        };

        active.select("option1".to_string());
        assert_eq!(active.selected, &["option1"]);

        active.select("option2".to_string());
        assert_eq!(active.selected, &["option2"]);

        // Enable option3, which prevents deselecting option2, which disallows selecting option1
        active.select("option3".to_string());
        assert_eq!(active.selected, &["option2", "option3"]);

        active.select("option1".to_string());
        assert_eq!(active.selected, &["option2", "option3"]);
    }

    #[test]
    fn depending_on_group_allows_changing_group_option() {
        let options = &[
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
                display_name: "Requires any in group to be selected".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                chips: vec![Chip::Esp32],
                requires: vec!["group".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            chip: Chip::Esp32,
            selected: vec![],
            options,
        };

        // Nothing is selected in group, so option3 can't be selected
        active.select("option3".to_string());
        assert_eq!(active.selected, empty());

        active.select("option1".to_string());
        assert_eq!(active.selected, &["option1"]);

        active.select("option3".to_string());
        assert_eq!(active.selected, &["option1", "option3"]);

        active.select("option2".to_string());
        assert_eq!(active.selected, &["option3", "option2"]);
    }

    fn empty() -> &'static [&'static str] {
        &[]
    }
}
