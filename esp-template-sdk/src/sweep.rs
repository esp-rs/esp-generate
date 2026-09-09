//! Enumerating the option combinations a template can be generated with.
//!
//! Shared by `esp-generate check` and `xtask`, and driven entirely by the
//! template's own `required`, `selection_group`, `requires` and `compatible`.

use crate::config::{ActiveConfiguration, find_option, flatten_options};
use crate::plugin::{Resolved, selection};
use crate::template::{GeneratorOption, GeneratorOptionItem, Template};

use indexmap::IndexMap;

/// How much of the option space to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Coverage {
    /// Every option once, with whatever its `requires` drags in.
    #[default]
    Individual,
    /// Every valid union of those individual selections.
    Combinations,
}

/// What to enumerate, and how far.
#[derive(Debug, Clone, Default)]
pub struct SweepOptions {
    pub coverage: Coverage,
    /// Options forced into every combination. A pick for a required group
    /// narrows that group to it instead of sweeping the group's members.
    pub pinned: Vec<String>,
    /// Selection groups kept out of the pool entirely.
    pub excluded_groups: Vec<String>,
    /// Categories kept out of the pool, with everything nested beneath them.
    pub excluded_categories: Vec<String>,
    /// Groups crossed with the pool instead of joining it, so every option is
    /// enumerated once per member.
    pub crossed_groups: Vec<String>,
}

/// Every combination worth generating, each a full option list, in a stable
/// order.
pub fn enumerate(
    template: &Template,
    resolved: &Resolved<'_>,
    opts: &SweepOptions,
) -> Result<Vec<Vec<String>>, String> {
    let flat = flatten_options(&template.options);
    let pool = pool(template, opts, &flat);
    let crosses = crosses(opts, &flat);
    let mut out = Vec::new();

    for base in required_picks(template, &flat, &opts.pinned)? {
        out.extend(enumerate_for(
            template, resolved, opts, &flat, &pool, &crosses, &base,
        )?);
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// The always-on option sets: one per way of satisfying every `required` group,
/// narrowed by whatever the caller pinned.
fn required_picks(
    template: &Template,
    flat: &[GeneratorOption],
    pinned: &[String],
) -> Result<Vec<Vec<String>>, String> {
    for name in pinned {
        if find_option(name, flat).is_none() {
            return Err(format!("template has no option `{name}`"));
        }
    }

    let mut picks: Vec<Vec<String>> = vec![
        pinned
            .iter()
            .filter(|name| {
                find_option(name, flat)
                    .is_some_and(|(_, o)| !template.required.contains(&o.selection_group))
            })
            .cloned()
            .collect(),
    ];

    for group in &template.required {
        let pinned_here: Vec<&String> = pinned
            .iter()
            .filter(|name| {
                find_option(name, flat).is_some_and(|(_, o)| &o.selection_group == group)
            })
            .collect();

        let members: Vec<String> = if pinned_here.is_empty() {
            flat.iter()
                .filter(|o| &o.selection_group == group)
                .map(|o| o.name.clone())
                .collect()
        } else {
            pinned_here.into_iter().cloned().collect()
        };

        if members.is_empty() {
            return Err(format!("required group `{group}` has no options"));
        }

        picks = picks
            .into_iter()
            .flat_map(|base| {
                members.iter().map(move |member| {
                    let mut next = base.clone();
                    next.push(member.clone());
                    next
                })
            })
            .collect();
    }

    Ok(picks)
}

/// The options that get a dimension of their own: everything not already
/// decided by a required group, excluded, or crossed.
fn pool<'a>(template: &Template, opts: &SweepOptions, flat: &'a [GeneratorOption]) -> Vec<&'a str> {
    let reserved = |group: &str| {
        !group.is_empty()
            && (template.required.iter().any(|g| g == group)
                || opts.excluded_groups.iter().any(|g| g == group)
                || opts.crossed_groups.iter().any(|g| g == group))
    };

    let mut excluded = Vec::new();
    collect_categories(
        &template.options,
        &opts.excluded_categories,
        false,
        &mut excluded,
    );

    flat.iter()
        .filter(|o| {
            !o.name.is_empty() && !reserved(&o.selection_group) && !excluded.contains(&o.name)
        })
        .map(|o| o.name.as_str())
        .collect()
}

/// One entry per way of picking at most one member from each crossed group,
/// crossed with each other so two crossed groups are seen together.
fn crosses<'a>(opts: &SweepOptions, flat: &'a [GeneratorOption]) -> Vec<Vec<&'a str>> {
    let mut crosses = vec![Vec::new()];
    for group in &opts.crossed_groups {
        let members: Vec<&str> = flat
            .iter()
            .filter(|o| &o.selection_group == group)
            .map(|o| o.name.as_str())
            .collect();

        crosses = crosses
            .into_iter()
            .flat_map(|picked| {
                std::iter::once(picked.clone()).chain(members.iter().map(move |member| {
                    let mut next = picked.clone();
                    next.push(*member);
                    next
                }))
            })
            .collect();
    }
    crosses
}

/// The combinations reachable from one set of always-on options.
fn enumerate_for(
    template: &Template,
    resolved: &Resolved<'_>,
    opts: &SweepOptions,
    flat: &[GeneratorOption],
    pool: &[&str],
    crosses: &[Vec<&str>],
    base: &[String],
) -> Result<Vec<Vec<String>>, String> {
    let facts = resolved.facts(&selection(base.to_vec(), flat))?;

    let base_idx: Vec<usize> = base
        .iter()
        .filter_map(|name| find_option(name, flat).map(|(idx, _)| idx))
        .collect();

    // Reused rather than rebuilt: the enumeration below runs once per
    // combination, and this holds a copy of the whole option tree.
    let mut trial = ActiveConfiguration {
        selected: base_idx.clone(),
        flat_options: flat.to_vec(),
        options: template.options.clone(),
        facts: Some(facts.clone()),
    };

    // Each option paired with everything its `requires` drags in, keyed by what
    // it is an alternative to: members of one selection group exclude each
    // other, and so do the cross-variants of a single option.
    let mut singles: IndexMap<String, Vec<Vec<usize>>> = IndexMap::new();
    for cross in crosses {
        for option in pool {
            trial.selected.clear();
            trial.selected.extend_from_slice(&base_idx);
            for member in cross {
                select_with_dependencies(&mut trial, member)?;
            }
            select_with_dependencies(&mut trial, option)?;

            let Some(selected) = valid_selection(&trial, &base_idx) else {
                continue;
            };
            let (_, found) = find_option(option, flat).expect("pool comes from `flat`");
            let key = if found.selection_group.is_empty() {
                found.name.clone()
            } else {
                found.selection_group.clone()
            };
            let alternatives = singles.entry(key).or_default();
            if !alternatives.contains(&selected) {
                alternatives.push(selected);
            }
        }
    }

    let names = |selected: Vec<usize>| -> Vec<String> {
        base.iter()
            .cloned()
            .chain(selected.into_iter().map(|idx| flat[idx].name.clone()))
            .collect()
    };

    if opts.coverage == Coverage::Individual {
        let mut flattened: Vec<Vec<usize>> = vec![Vec::new()];
        flattened.extend(singles.into_values().flatten());
        flattened.sort();
        flattened.dedup();
        return Ok(flattened.into_iter().map(names).collect());
    }

    // One dimension per key, each offering "none" plus its alternatives, so
    // only combinations a host could actually reach are built at all.
    let dimensions: Vec<&Vec<Vec<usize>>> = singles.values().collect();
    let Some(total) = dimensions
        .iter()
        .try_fold(1u64, |total, d| total.checked_mul(d.len() as u64 + 1))
    else {
        return Err(format!(
            "{} dimensions is more combinations than can be counted",
            dimensions.len()
        ));
    };

    let mut result: Vec<Vec<usize>> = Vec::new();
    for n in 0..total {
        // Mixed-radix: each dimension contributes "none" or one alternative.
        let mut n = n;
        trial.selected.clear();
        trial.selected.extend_from_slice(&base_idx);
        for dimension in &dimensions {
            let radix = dimension.len() as u64 + 1;
            let choice = (n % radix) as usize;
            n /= radix;
            if choice > 0 {
                trial.selected.extend_from_slice(&dimension[choice - 1]);
            }
        }
        trial.selected.sort();
        trial.selected.dedup();

        if let Some(selected) = valid_selection(&trial, &base_idx) {
            result.push(selected);
        }
    }
    result.sort();
    result.dedup();

    Ok(result.into_iter().map(names).collect())
}

/// Collect every option name nested under one of `excluded`, at any depth.
fn collect_categories(
    items: &[GeneratorOptionItem],
    excluded: &[String],
    inside: bool,
    out: &mut Vec<String>,
) {
    for item in items {
        match item {
            GeneratorOptionItem::Option(option) if inside => out.push(option.name.clone()),
            GeneratorOptionItem::Option(_) => {}
            GeneratorOptionItem::Category(category) => collect_categories(
                &category.options,
                excluded,
                inside || excluded.iter().any(|name| name == &category.name),
                out,
            ),
        }
    }
}

/// Select `option` and, recursively, everything its `requires` names. One the
/// selection cannot support is skipped rather than forced.
fn select_with_dependencies(config: &mut ActiveConfiguration, option: &str) -> Result<(), String> {
    let (idx, found) = find_option(option, &config.flat_options)
        .ok_or_else(|| format!("template has no option `{option}`"))?;

    if config.selected.contains(&idx) {
        return Ok(());
    }

    // Cloned so the borrow from `find_option` ends before the recursion.
    for dependency in found.requires.clone() {
        if dependency.starts_with('!') {
            continue;
        }
        select_with_dependencies(config, &dependency)?;
    }

    if config.is_option_active(&config.flat_options[idx]) {
        config.select_idx(idx);
    }

    Ok(())
}

/// The selection minus the always-on base, or `None` if it is not a
/// combination a host could have reached.
fn valid_selection(config: &ActiveConfiguration, base_idx: &[usize]) -> Option<Vec<usize>> {
    let mut groups = Vec::new();

    for &idx in &config.selected {
        let option = &config.flat_options[idx];

        if !config.is_option_active(option) {
            return None;
        }

        // One pick per selection group.
        if !option.selection_group.is_empty() {
            if groups.contains(&&option.selection_group) {
                return None;
            }
            groups.push(&option.selection_group);
        }
    }

    let mut selected: Vec<usize> = config
        .selected
        .iter()
        .copied()
        .filter(|idx| !base_idx.contains(idx))
        .collect();
    selected.sort();
    Some(selected)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A template with a required `chip` group, a mutually exclusive
    /// `log-frontend` group, a dependency, and one plain option.
    const YAML: &str = r#"
required: [chip]
options:
  - !Category
    name: chip
    display_name: Chip
    options:
      - !Option
        name: chip-a
        display_name: A
        selection_group: chip
      - !Option
        name: chip-b
        display_name: B
        selection_group: chip
  - !Option
    name: alloc
    display_name: Alloc
  - !Option
    name: wifi
    display_name: Wifi
    requires: [alloc]
  - !Option
    name: defmt
    display_name: defmt
    selection_group: log-frontend
  - !Option
    name: log
    display_name: log
    selection_group: log-frontend
  - !Option
    name: ble
    display_name: BLE
    selection_group: ble-lib
  - !Category
    name: editor
    display_name: Editor
    options:
      - !Option
        name: vscode
        display_name: VS Code
"#;

    fn template() -> Template {
        Template::load(YAML, &Resolved::default(), |_| None).expect("test template must load")
    }

    fn options_of(opts: SweepOptions) -> Vec<Vec<String>> {
        enumerate(&template(), &Resolved::default(), &opts).expect("sweep must succeed")
    }

    /// Nothing pinned means every way of satisfying the required group.
    #[test]
    fn a_required_group_is_swept_one_member_at_a_time() {
        let all = options_of(SweepOptions::default());
        assert!(
            all.iter()
                .all(|o| o.contains(&"chip-a".to_string()) != o.contains(&"chip-b".to_string()),)
        );
        assert!(all.iter().any(|o| o[0] == "chip-a"));
        assert!(all.iter().any(|o| o[0] == "chip-b"));
    }

    #[test]
    fn pinning_a_member_narrows_the_sweep_to_it() {
        let pinned = options_of(SweepOptions {
            pinned: vec!["chip-b".to_string()],
            ..Default::default()
        });
        assert!(pinned.iter().all(|o| o.contains(&"chip-b".to_string())));
        assert!(!pinned.iter().any(|o| o.contains(&"chip-a".to_string())));
    }

    #[test]
    fn an_option_brings_its_requirements_with_it() {
        let with_wifi = options_of(SweepOptions {
            pinned: vec!["chip-a".to_string()],
            ..Default::default()
        });
        let wifi = with_wifi
            .iter()
            .find(|o| o.contains(&"wifi".to_string()))
            .expect("wifi must be swept");
        assert!(wifi.contains(&"alloc".to_string()), "{wifi:?}");
    }

    /// Two members of one selection group can never be selected together, so a
    /// combination holding both is not one a host could reach.
    #[test]
    fn a_selection_group_never_yields_two_picks_at_once() {
        let all = options_of(SweepOptions {
            coverage: Coverage::Combinations,
            pinned: vec!["chip-a".to_string()],
            ..Default::default()
        });
        for combination in &all {
            assert!(
                !(combination.contains(&"defmt".to_string())
                    && combination.contains(&"log".to_string())),
                "{combination:?}"
            );
        }
        // But each on its own is reached.
        assert!(all.iter().any(|o| o.contains(&"defmt".to_string())));
        assert!(all.iter().any(|o| o.contains(&"log".to_string())));
    }

    #[test]
    fn combinations_cover_more_than_individual_options() {
        let individual = options_of(SweepOptions {
            pinned: vec!["chip-a".to_string()],
            ..Default::default()
        });
        let combinations = options_of(SweepOptions {
            coverage: Coverage::Combinations,
            pinned: vec!["chip-a".to_string()],
            ..Default::default()
        });
        assert!(combinations.len() > individual.len());
        assert!(
            combinations
                .iter()
                .any(|o| o.contains(&"alloc".to_string()) && o.contains(&"defmt".to_string())),
            "a combination should pair options that the individual sweep only sees alone"
        );
    }

    #[test]
    fn an_excluded_category_takes_its_options_with_it() {
        let all = options_of(SweepOptions {
            excluded_categories: vec!["editor".to_string()],
            ..Default::default()
        });
        assert!(!all.iter().any(|o| o.contains(&"vscode".to_string())));
    }

    #[test]
    fn an_excluded_group_is_left_out() {
        let all = options_of(SweepOptions {
            excluded_groups: vec!["log-frontend".to_string()],
            ..Default::default()
        });
        assert!(!all.iter().any(|o| o.contains(&"defmt".to_string())));
        assert!(!all.iter().any(|o| o.contains(&"log".to_string())));
    }

    /// A crossed group multiplies the sweep rather than joining it, so every
    /// other option is seen once per member and once without any.
    #[test]
    fn a_crossed_group_pairs_with_every_option() {
        let all = options_of(SweepOptions {
            pinned: vec!["chip-a".to_string()],
            crossed_groups: vec!["log-frontend".to_string()],
            ..Default::default()
        });
        for member in ["defmt", "log"] {
            assert!(
                all.iter()
                    .any(|o| o.contains(&member.to_string()) && o.contains(&"alloc".to_string())),
                "`alloc` should be swept alongside `{member}`"
            );
        }
        assert!(
            all.iter()
                .any(|o| o == &["chip-a".to_string(), "alloc".to_string()]),
            "and once with no member at all"
        );
    }

    /// Two crossed groups are dimensions in their own right, so an option is
    /// seen with a member of each at once — not with one or the other.
    #[test]
    fn crossed_groups_cross_with_each_other() {
        let all = options_of(SweepOptions {
            pinned: vec!["chip-a".to_string()],
            crossed_groups: vec!["log-frontend".to_string(), "ble-lib".to_string()],
            ..Default::default()
        });

        assert!(
            all.iter().any(|o| o.contains(&"alloc".to_string())
                && o.contains(&"defmt".to_string())
                && o.contains(&"ble".to_string())),
            "an option should be swept with a member of both crossed groups: {all:?}"
        );
    }

    #[test]
    fn pinning_a_name_the_template_does_not_have_is_an_error() {
        let err = enumerate(
            &template(),
            &Resolved::default(),
            &SweepOptions {
                pinned: vec!["nonsense".to_string()],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("nonsense"), "{err}");
    }

    /// Members of one selection group exclude each other, so a large group
    /// costs one combination per member rather than one per subset.
    #[test]
    fn a_large_selection_group_does_not_explode() {
        let mut yaml = String::from("options:\n");
        for i in 0..30 {
            yaml.push_str(&format!(
                "  - !Option\n    name: opt{i}\n    display_name: O\n    selection_group: big\n"
            ));
        }
        let big = Template::load(&yaml, &Resolved::default(), |_| None).unwrap();

        let all = enumerate(
            &big,
            &Resolved::default(),
            &SweepOptions {
                coverage: Coverage::Combinations,
                ..Default::default()
            },
        )
        .unwrap();

        // One per member, plus the empty selection — not 2^30.
        assert_eq!(all.len(), 31);
    }
}
