use std::collections::HashMap;

use crate::template::{GeneratorOption, GeneratorOptionItem};

#[derive(Debug)]
pub struct ActiveConfiguration {
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
    /// Rebuild [`Self::flat_options`] from [`Self::options`] and remap
    /// [`Self::selected`] by option name.
    ///
    /// Must be called whenever `options` is mutated out-of-band — e.g. after
    /// the toolchain category is (re)populated in response to a scan result or
    /// a chip switch. `selected` stores indices into `flat_options`, so any
    /// structural change to the tree invalidates them.
    ///
    /// Options whose name is no longer present in the rebuilt flat view are
    /// silently dropped from `selected`. This is the correct behaviour for a
    /// chip switch that removes toolchains (or any other category items): the
    /// cascade logic in `select_idx` / `deselect_idx` runs off indices, not
    /// names, so leaving dangling indices would be a latent panic.
    pub fn rebuild_indices(&mut self) {
        let selected_names: Vec<String> = self
            .selected
            .iter()
            .filter_map(|&idx| self.flat_options.get(idx).map(|o| o.name.clone()))
            .collect();

        self.flat_options = flatten_options(&self.options);

        self.selected = selected_names
            .into_iter()
            .filter_map(|name| self.flat_options.iter().position(|o| o.name == name))
            .collect();
    }

    /// Swap in a new options tree and keep `selected` / `flat_options`
    /// consistent.
    ///
    /// This is the supported entry point for dynamic tree rebuilds (chip
    /// switch, toolchain scan result, …): the caller builds the new options
    /// tree (chip filter + module population + toolchain population) off of
    /// the pristine template and hands it over. The rest is mechanical:
    ///   * [`Self::rebuild_indices`] remaps selection indices by option name,
    ///     silently dropping any name that no longer exists in the new tree;
    ///   * [`Self::drop_unsatisfied`] then cascades out anything whose
    ///     requirements are no longer met against the trimmed set (e.g. an
    ///     option that survived by name but depended on something the chip
    ///     switch eliminated).
    ///
    /// Note: `path` on [`crate::tui::Repository`] is a UI concern and is NOT
    /// touched here.
    pub fn reset_options(&mut self, options: Vec<GeneratorOptionItem>) {
        self.options = options;
        self.rebuild_indices();
        self.drop_unsatisfied();
    }

    /// Return the subset of `required` that currently has no selected
    /// option in the live tree. Mirrors
    /// [`crate::template::Template::missing_required_groups`] but operates
    /// on the selection indices the TUI actually mutates, so it stays in
    /// sync across chip switches and toolchain repopulations without the
    /// caller having to translate back to option names.
    pub fn missing_required_groups(&self, required: &[String]) -> Vec<String> {
        required
            .iter()
            .filter(|group| {
                !self.selected.iter().any(|&idx| {
                    self.flat_options
                        .get(idx)
                        .is_some_and(|o| &o.selection_group == *group)
                })
            })
            .cloned()
            .collect()
    }

    /// For each group in `groups`, the currently-selected option name (or
    /// empty string if none). `groups` must be collected from the pristine
    /// template so that selections for groups whose dependants have been
    /// pruned still show up.
    pub fn compatibility_signature(&self, groups: &[String]) -> HashMap<String, String> {
        groups
            .iter()
            .map(|group| {
                let selected = self
                    .selected
                    .iter()
                    .find_map(|&idx| {
                        let o = &self.flat_options[idx];
                        (&o.selection_group == group).then(|| o.name.clone())
                    })
                    .unwrap_or_default();
                (group.clone(), selected)
            })
            .collect()
    }

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
        let (index, _o) = find_option(option, &self.flat_options).unwrap();
        self.select_idx(index);
    }

    /// Selects the option at `idx`.
    ///
    /// Negative requirements (`!X`) on this option force-deselect `X` if it's currently
    /// selected. The deselection cascades: anything whose requirements are no longer
    /// met is dropped as well (there is no save/restore mechanism; swapped-out options
    /// simply disappear until the user re-enables them).
    ///
    /// Positive requirements remain hard gates — if any of them is unmet the call is
    /// a no-op.
    pub fn select_idx(&mut self, idx: usize) {
        let o = self.flat_options[idx].clone();

        // Positive requirements can't be materialised, so they still gate selection.
        for req in &o.requires {
            if req.starts_with('!') {
                continue;
            }
            if self.is_selected(req) {
                continue;
            }
            if Self::group_exists(req, &self.flat_options) && self.is_group_selected(req) {
                continue;
            }
            return;
        }

        // If the option is already selected, leave state as-is.
        if self.selected.contains(&idx) {
            return;
        }

        // Swap out same-group siblings (the existing "radio-button" behaviour).
        if !Self::deselect_group(&mut self.selected, &self.flat_options, &o.selection_group) {
            return;
        }

        // Force-deselect anything directly forbidden by a `!X` requirement.
        for req in &o.requires {
            let Some(disables) = req.strip_prefix('!') else {
                continue;
            };
            if let Some(pos) = self.selected_index(disables) {
                self.selected.swap_remove(pos);
            }
        }

        self.selected.push(idx);

        // Cascade: drop anything whose requirements broke as a side effect.
        self.drop_unsatisfied();
    }

    /// Deselects the option at `idx`, then cascades: any remaining selection whose
    /// requirements were propped up by the one we just removed is dropped too. This is
    /// the counterpart to [`Self::select_idx`]'s cascade, and gives the user a
    /// predictable "clear, don't save" experience when toggling off an option that
    /// others depend on (e.g. toggling probe-rs off clears panic-rtt-target).
    pub fn deselect_idx(&mut self, idx: usize) {
        let Some(pos) = self.selected.iter().position(|&s| s == idx) else {
            return;
        };
        self.selected.swap_remove(pos);
        self.drop_unsatisfied();
    }

    /// Fixpoint loop that evicts selected options whose requirements are no longer
    /// met, or whose `compatible` constraint is no longer satisfied. Used after
    /// cascading deselection and after any chip / selection-group change that
    /// might have invalidated compatibility.
    fn drop_unsatisfied(&mut self) {
        loop {
            let victim = self.selected.iter().position(|&idx| {
                let opt = &self.flat_options[idx];
                !self.requirements_met(&opt.requires)
                    || !Self::is_option_compatible_against(
                        opt,
                        &self.selected,
                        &self.flat_options,
                    )
            });
            match victim {
                Some(pos) => {
                    self.selected.swap_remove(pos);
                }
                None => return,
            }
        }
    }

    /// Returns the names of currently-selected options that would be force-deselected
    /// (directly or via cascade) if the user toggled the given option. Empty when
    /// toggling would be non-destructive.
    ///
    /// Symmetric: works whether the option is currently selected (simulates deselect,
    /// reports cascade) or not (simulates select, reports same-group siblings,
    /// negative-requirement targets, and cascade). The option being toggled is never
    /// itself reported — only collateral damage.
    ///
    /// Defensive short-circuit: if the option is currently unselected and cannot
    /// actually be toggled on (chip-mismatch or an unmet *positive* requirement),
    /// the toggle would be a no-op, so we return an empty list. We gate on
    /// [`Self::is_option_toggleable`] rather than the strict [`Self::is_option_active`]
    /// on purpose — a row whose only conflict is a negative requirement *is*
    /// toggleable (selecting it cascades the conflict out), and its cascade is
    /// exactly what callers want reported.
    pub fn would_force_deselect(&self, option: &GeneratorOption) -> Vec<String> {
        let option_idx = find_option(&option.name, &self.flat_options).map(|(i, _)| i);

        let currently_selected = option_idx
            .map(|idx| self.selected.contains(&idx))
            .unwrap_or(false);

        if !currently_selected && !self.is_option_toggleable(option) {
            return Vec::new();
        }

        let mut simulated = self.selected.clone();
        if currently_selected {
            // Simulated deselect: remove the option, then let cascade evict dependents.
            if let Some(idx) = option_idx {
                if let Some(pos) = simulated.iter().position(|&i| i == idx) {
                    simulated.swap_remove(pos);
                }
            }
        } else {
            // Simulated select: kick same-group siblings…
            if !option.selection_group.is_empty() {
                simulated.retain(|idx| {
                    let o = &self.flat_options[*idx];
                    o.selection_group != option.selection_group
                });
            }

            // …and anything directly named by a `!X` requirement.
            for req in &option.requires {
                let Some(disables) = req.strip_prefix('!') else {
                    continue;
                };
                if let Some(pos) = simulated
                    .iter()
                    .position(|&i| self.flat_options[i].name == disables)
                {
                    simulated.swap_remove(pos);
                }
            }

            // Put the option into the simulated set so cascade evaluates it in context.
            if let Some(idx) = option_idx {
                if !simulated.contains(&idx) {
                    simulated.push(idx);
                }
            }
        }

        // Shared cascade: drop anything whose requirements are no longer met
        // or whose `compatible` constraint is no longer satisfied. The latter
        // is how chip-switch previews show every option that would be pruned
        // by the new chip — no chip-specific code path needed, since the chip
        // is just another entry in the `chip` selection group and any option
        // with `compatible: {chip: [...]}` simply drops out of the simulated
        // set when the new chip isn't in its allow-list.
        loop {
            let victim = simulated.iter().position(|&idx| {
                let opt = &self.flat_options[idx];
                !Self::requirements_met_against(opt, &simulated, &self.flat_options)
                    || !Self::is_option_compatible_against(opt, &simulated, &self.flat_options)
            });
            match victim {
                Some(pos) => {
                    simulated.swap_remove(pos);
                }
                None => break,
            }
        }

        // Collateral = things that were selected but aren't in the simulated set
        // (excluding the option itself, which is the user's direct action).
        self.selected
            .iter()
            .copied()
            .filter(|idx| !simulated.contains(idx) && Some(*idx) != option_idx)
            .map(|idx| self.flat_options[idx].name.clone())
            .collect()
    }

    /// Static helper: evaluate `option.requires` against an arbitrary selected set.
    fn requirements_met_against(
        option: &GeneratorOption,
        selected: &[usize],
        flat_options: &[GeneratorOption],
    ) -> bool {
        for requirement in &option.requires {
            let (key, expected) = if let Some(rest) = requirement.strip_prefix('!') {
                (rest, false)
            } else {
                (requirement.as_str(), true)
            };

            let is_selected = selected.iter().any(|s| flat_options[*s].name == key);
            if is_selected == expected {
                continue;
            }

            let is_group = flat_options.iter().any(|o| o.selection_group == key);
            if is_group {
                let group_selected = selected
                    .iter()
                    .any(|s| flat_options[*s].selection_group == key);
                if group_selected == expected {
                    continue;
                }
            }

            return false;
        }
        true
    }

    /// Returns whether an item is toggleable in the UI.
    ///
    /// For a category, the category's own requirements must be met (strict) *and* at
    /// least one descendant must itself be toggleable; for a leaf option this uses
    /// [`Self::is_option_toggleable`], i.e. an option with an unmet `!X` is still
    /// considered reachable because selecting it would cascade `X` out.
    ///
    /// This is the TUI-facing predicate. For strict validation (e.g. the CLI
    /// checking whether a user's selection set is self-consistent), use
    /// [`Self::is_option_active`] directly.
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
            GeneratorOptionItem::Option(option) => self.is_option_toggleable(option),
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

    /// Returns whether every `compatible: { group: [...] }` entry on `option`
    /// is currently satisfied. Absent groups (and the empty map) are trivially
    /// compatible. For each listed group there must be a selected option whose
    /// `selection_group` matches the key AND whose `name` is in the allow-list.
    ///
    /// This is the generalised replacement for the old `chips:` filter —
    /// `compatible: { chip: [...] }` is now how a template option expresses
    /// "I only apply to these chips", driven by the current selection in the
    /// `chip` selection group.
    pub fn is_option_compatible(&self, option: &GeneratorOption) -> bool {
        Self::is_option_compatible_against(option, &self.selected, &self.flat_options)
    }

    /// Static variant of [`Self::is_option_compatible`] that evaluates against
    /// an arbitrary selection set. Used by [`Self::would_force_deselect`] to
    /// simulate the effect of a toggle without mutating `self`.
    fn is_option_compatible_against(
        option: &GeneratorOption,
        selected: &[usize],
        flat_options: &[GeneratorOption],
    ) -> bool {
        for (group, allowed) in &option.compatible {
            let group_ok = selected.iter().any(|&idx| {
                let o = &flat_options[idx];
                o.selection_group == *group && allowed.iter().any(|n| n == &o.name)
            });
            if !group_ok {
                return false;
            }
        }
        true
    }

    /// Strict "is this option consistent with the current selection?" predicate.
    ///
    /// Returns `true` only when:
    ///   * every `compatible` group constraint is satisfied (see
    ///     [`Self::is_option_compatible`]),
    ///   * every one of its requirements is satisfied (including negative ones), and
    ///   * no other currently-selected option has `!{option.name}` in its `requires`.
    ///
    /// This is the right check for validation: the CLI (`esp-generate --headless -o …`)
    /// and the xtask regression harness rely on it to reject inconsistent selection
    /// sets. It is *not* the right check for "can the user click this row in the TUI"
    /// — that's [`Self::is_option_toggleable`], which permits negative-requirement
    /// conflicts on the understanding that selecting the row will cascade them out
    /// via [`Self::select_idx`].
    pub fn is_option_active(&self, option: &GeneratorOption) -> bool {
        if !self.is_option_compatible(option) {
            return false;
        }

        if !self.requirements_met(&option.requires) {
            return false;
        }

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

    /// Permissive "can the user toggle this row in the TUI?" predicate.
    ///
    /// Returns `true` when the option is available for the current chip and every
    /// *positive* requirement is met. Negative requirements (`!X`) are intentionally
    /// ignored here: the TUI treats them as force-deselect hints, not gates.
    /// Selecting a row in this state routes through [`Self::select_idx`], which
    /// cascades the negative-requirement targets (and their dependents) out; use
    /// [`Self::would_force_deselect`] to preview that cascade.
    ///
    /// Callers that care about full self-consistency (CLI validation, xtask
    /// coverage) must use [`Self::is_option_active`] instead.
    pub fn is_option_toggleable(&self, option: &GeneratorOption) -> bool {
        if !self.is_option_compatible(option) {
            return false;
        }

        for req in &option.requires {
            if req.starts_with('!') {
                continue;
            }
            if self.is_selected(req) {
                continue;
            }
            if Self::group_exists(req, &self.flat_options) && self.is_group_selected(req) {
                continue;
            }
            return false;
        }

        true
    }

    // An option can only be disabled if it's not required by any other selected option.
    pub fn can_be_disabled(&self, option: &str) -> bool {
        let (option, _) = find_option(option, &self.flat_options).unwrap();
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

/// Find an option by name.
///
/// The template may carry multiple entries that share a name but differ by
/// their `compatible` constraints. `options` is expected to have already been
/// pruned for the active selections (see `build_options` in `main`), so at
/// most one entry per name should survive and a simple name match is enough.
pub fn find_option<'c>(
    option: &str,
    options: &'c [GeneratorOption],
) -> Option<(usize, &'c GeneratorOption)> {
    options
        .iter()
        .enumerate()
        .find(|(_, opt)| opt.name == option)
}

#[cfg(test)]
mod test {
    use indexmap::IndexMap;

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
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["option2".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
        ];
        let active = ActiveConfiguration {
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
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "group".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option3".to_string(),
                display_name: "Prevents deselecting option2".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["option2".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
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
                        compatible: IndexMap::new(),
                        sets: IndexMap::new(),
                        requires: vec![],
                    }),
                    GeneratorOptionItem::Option(GeneratorOption {
                        name: "option2".to_string(),
                        display_name: "Barfoo".to_string(),
                        selection_group: "group".to_string(),
                        help: "".to_string(),
                        compatible: IndexMap::new(),
                        sets: IndexMap::new(),
                        requires: vec![],
                    }),
                ],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option3".to_string(),
                display_name: "Requires any in group to be selected".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["group".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option4".to_string(),
                display_name: "Extra option that depends on something".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["option3".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
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
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["group".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
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
    fn negative_requirement_force_deselects_instead_of_blocking() {
        // option2 requires `!option1`. option1 is selected. option2 must be *active*
        // (negative requirements no longer block selection) and selecting it must
        // clear option1 rather than save it somewhere for later.
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option1".to_string(),
                display_name: "Foobar".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "option2".to_string(),
                display_name: "Barfoo".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["!option1".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        active.select("option1");
        let opt2 = active.flat_options[1].clone();
        // Strict validation (CLI/xtask): selection set is inconsistent → false.
        assert!(!active.is_option_active(&opt2));
        // TUI predicate: row is toggleable, selecting it will cascade `option1` out.
        assert!(active.is_option_toggleable(&opt2));
        assert_eq!(
            active.would_force_deselect(&opt2),
            vec!["option1".to_string()]
        );

        active.select("option2");
        assert!(active.is_selected("option2"));
        assert!(!active.is_selected("option1"));
    }

    #[test]
    fn select_cascade_evicts_dependents_of_deselected_option() {
        // Real-world analogue: selecting `probe-rs` must clear `log` (which has
        // `!probe-rs`) and `embedded-test` (which requires `log`). Nothing is
        // remembered — if the user toggles probe-rs back off they start from
        // scratch. Symmetrically, once probe-rs *is* selected, deselecting it must
        // cascade out anything requiring it (here, `panic-rtt-target`) and the
        // warning predicate must list those dependents.
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "probe-rs".to_string(),
                display_name: "probe-rs".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log".to_string(),
                display_name: "log".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["!probe-rs".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "embedded-test".to_string(),
                display_name: "embedded-test".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["log".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "panic-rtt-target".to_string(),
                display_name: "panic-rtt-target".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["probe-rs".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wifi".to_string(),
                display_name: "wifi".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
        ];
        let mut active = ActiveConfiguration {
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        active.select("log");
        active.select("embedded-test");
        active.select("wifi");

        // Select side: hovering `probe-rs` while `log`/`embedded-test` are on
        // reports both as going away; actually selecting it does the cascade.
        let probe_rs = active.flat_options[0].clone();
        let mut evicted = active.would_force_deselect(&probe_rs);
        evicted.sort();
        assert_eq!(
            evicted,
            vec!["embedded-test".to_string(), "log".to_string()]
        );

        active.select("probe-rs");
        assert!(active.is_selected("probe-rs"));
        assert!(!active.is_selected("log"));
        assert!(!active.is_selected("embedded-test"));
        assert!(active.is_selected("wifi"));

        // Deselect side: with probe-rs on, add `panic-rtt-target` (requires
        // probe-rs). Hovering probe-rs must now list panic-rtt-target in the
        // warning; `deselect_idx` does the same cascade.
        active.select("panic-rtt-target");
        let probe_rs = active.flat_options[0].clone();
        let mut evicted = active.would_force_deselect(&probe_rs);
        evicted.sort();
        assert_eq!(evicted, vec!["panic-rtt-target".to_string()]);
        assert!(!evicted.contains(&"probe-rs".to_string())); // never itself
        assert!(!evicted.contains(&"wifi".to_string())); // unrelated stays

        let (probe_rs_flat_idx, _) =
            find_option("probe-rs", &active.flat_options).unwrap();
        active.deselect_idx(probe_rs_flat_idx);
        assert!(!active.is_selected("probe-rs"));
        assert!(!active.is_selected("panic-rtt-target"));
        // Previously-cleared `!probe-rs` options are NOT resurrected.
        assert!(!active.is_selected("log"));
        assert!(!active.is_selected("embedded-test"));
        assert!(active.is_selected("wifi"));

        // Short-circuit: an unselected option that isn't actually toggleable must
        // report an empty cascade — toggling it is a no-op, so advertising
        // collateral damage would be a lie.
        //
        // Fresh config with three would-be-destructive rows whose toggle
        // is *not* actionable:
        //   * compatibility-mismatch (`compatible: { chip: [Esp32c6] }` with
        //     an `esp32` selection in the `chip` group),
        //   * unmet positive requirement (`requires: ["missing"]`),
        //   * both of the above.
        // Each of them also carries `!victim`, which — without the guard —
        // `would_force_deselect` would still happily report.
        let mut wrong_chip_compat = IndexMap::new();
        wrong_chip_compat.insert("chip".to_string(), vec!["esp32c6".to_string()]);
        let options = vec![
            // Stand-in for the chip selector the TUI populates at runtime.
            // `ActiveConfiguration::select` picks this up and now drives every
            // `compatible: { chip: [...] }` check.
            GeneratorOptionItem::Option(GeneratorOption {
                name: "esp32".to_string(),
                display_name: "esp32".to_string(),
                selection_group: "chip".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "victim".to_string(),
                display_name: "victim".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wrong-chip".to_string(),
                display_name: "wrong-chip".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: wrong_chip_compat,
                sets: IndexMap::new(),
                requires: vec!["!victim".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "unmet-pos".to_string(),
                display_name: "unmet-pos".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["missing".to_string(), "!victim".to_string()],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "neg-conflict".to_string(),
                display_name: "neg-conflict".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec!["!victim".to_string()],
            }),
        ];
        let mut active = ActiveConfiguration {
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };
        active.select("esp32");
        active.select("victim");

        let wrong_chip = active
            .flat_options
            .iter()
            .find(|o| o.name == "wrong-chip")
            .cloned()
            .unwrap();
        let unmet_pos = active
            .flat_options
            .iter()
            .find(|o| o.name == "unmet-pos")
            .cloned()
            .unwrap();
        let neg_conflict = active
            .flat_options
            .iter()
            .find(|o| o.name == "neg-conflict")
            .cloned()
            .unwrap();

        assert!(active.would_force_deselect(&wrong_chip).is_empty());
        assert!(active.would_force_deselect(&unmet_pos).is_empty());
        // Negative-only conflict is still actionable — the cascade must be reported.
        assert_eq!(
            active.would_force_deselect(&neg_conflict),
            vec!["victim".to_string()]
        );
    }

    fn empty() -> &'static [usize] {
        &[]
    }

    #[test]
    fn compatible_against_non_chip_group_hides_and_cascades() {
        // Exercise the generalised `compatible` constraint on a group other
        // than `chip`. A `pretty-logs` option is only compatible when the
        // active `log-frontend` is `defmt` (not `log`); switching the group
        // selection must cascade `pretty-logs` out of the selected set, and
        // `is_option_compatible` must reflect the change.
        let mut pretty_logs_compat = IndexMap::new();
        pretty_logs_compat.insert("log-frontend".to_string(), vec!["defmt".to_string()]);
        let options = vec![
            GeneratorOptionItem::Option(GeneratorOption {
                name: "defmt".to_string(),
                display_name: "defmt".to_string(),
                selection_group: "log-frontend".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "log".to_string(),
                display_name: "log".to_string(),
                selection_group: "log-frontend".to_string(),
                help: "".to_string(),
                compatible: IndexMap::new(),
                sets: IndexMap::new(),
                requires: vec![],
            }),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "pretty-logs".to_string(),
                display_name: "pretty-logs".to_string(),
                selection_group: "".to_string(),
                help: "".to_string(),
                compatible: pretty_logs_compat,
                sets: IndexMap::new(),
                requires: vec![],
            }),
        ];
        let mut active = ActiveConfiguration {
            selected: vec![],
            flat_options: flatten_options(&options),
            options,
        };

        // Baseline: nothing picked in log-frontend, so `pretty-logs` can't be
        // compatible and therefore can't be toggled on.
        let pretty = active
            .flat_options
            .iter()
            .find(|o| o.name == "pretty-logs")
            .cloned()
            .unwrap();
        assert!(!active.is_option_compatible(&pretty));
        assert!(!active.is_option_toggleable(&pretty));

        // Picking `defmt` satisfies `compatible: { log-frontend: [defmt] }`.
        active.select("defmt");
        assert!(active.is_option_compatible(&pretty));
        assert!(active.is_option_toggleable(&pretty));
        active.select("pretty-logs");
        assert!(active.is_selected("pretty-logs"));

        // Switching to `log` (same group) swaps the selection. The cascade
        // in `drop_unsatisfied` notices `pretty-logs` is no longer compatible
        // and clears it — exactly the "selected options should be cleared"
        // half of the `compatible` contract.
        active.select("log");
        assert!(active.is_selected("log"));
        assert!(!active.is_selected("defmt"));
        assert!(
            !active.is_selected("pretty-logs"),
            "pretty-logs must be cleared when log-frontend moves off defmt"
        );
        assert!(!active.is_option_compatible(&pretty));
    }
}
