//! Turning a selection into the files a project would contain. Generation
//! writes the result; `check` discards it and keeps the errors.

use anyhow::{Result, bail};

use esp_generate::config::find_option;
use esp_generate::manifest;
use esp_generate::plugin::selection;
use esp_generate::process;
use esp_generate::template::GeneratorOption;

use esp_generate::Loaded;

/// The values the host supplies to every render.
pub struct HostValues {
    pub project_name: String,
    /// The `-o …` line that would reproduce this project.
    pub generate_parameters: String,
    pub esp_hal_version_full: String,
    /// `None` when the template declares no toolchain-bearing target.
    pub rust_toolchain: Option<String>,
}

/// The template's `esp-hal` version, padded to the `x.y.z` docs.rs links need.
pub fn esp_hal_version_full(version: &str) -> String {
    let Some(stripped) = version.strip_prefix('~') else {
        return version.to_string();
    };
    let mut padded = stripped.to_string();
    while padded.chars().filter(|c| *c == '.').count() < 2 {
        padded.push_str(".0");
    }
    padded
}

/// Merge scalar `sets` from the selected options into `facts`.
fn merge_template_sets(facts: &mut process::Facts, selected: &[String], flat: &[GeneratorOption]) {
    for name in selected {
        let Some((_, opt)) = find_option(name, flat) else {
            continue;
        };
        for (key, value) in &opt.sets {
            if let Some(scalar) = value.as_scalar() {
                facts.set_value(key.clone(), scalar);
            }
        }
    }
}

/// The facts a render evaluates against, and the target they name.
///
/// Written in the order that makes first-writer-wins come out right: host
/// values, then the per-ISA toolchain default, then template `sets`.
pub fn facts(
    loaded: &Loaded,
    selected: &[String],
    flat_options: &[GeneratorOption],
    host: &HostValues,
) -> Result<(process::Facts, Option<crate::toolchain::ChipTarget>)> {
    let mut facts = loaded
        .resolved
        .facts(&selection(selected.to_vec(), flat_options))
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Every name that could be true for *some* selection, so a misspelling is
    // distinguishable from a name that is merely false right now.
    facts.vocabulary.options = Some(
        loaded
            .template
            .all_options()
            .iter()
            .map(|o| o.name.clone())
            .chain(flat_options.iter().map(|o| o.name.clone()))
            .collect(),
    );
    facts.vocabulary.groups = Some(
        loaded
            .template
            .all_options()
            .iter()
            .map(|o| o.selection_group.clone())
            .chain(flat_options.iter().map(|o| o.selection_group.clone()))
            .filter(|g| !g.is_empty())
            .collect(),
    );

    facts.set_value("generate_version", env!("CARGO_PKG_VERSION"));
    facts.set_value("project_name", host.project_name.clone());
    facts.set_value("generate_parameters", host.generate_parameters.clone());
    facts.set_value("esp_hal_version_full", host.esp_hal_version_full.clone());
    if let Some(toolchain) = &host.rust_toolchain {
        facts.set_value("rust_toolchain", toolchain.clone());
    }

    // Interpolation has no fallback for an unset name, so the per-ISA default
    // is written even when nothing picked one.
    let target = crate::toolchain::ChipTarget::from_facts(&facts);
    if let Some(target) = &target {
        facts.set_value(
            "rust_toolchain",
            if target.is_xtensa { "esp" } else { "stable" },
        );
    }

    merge_template_sets(&mut facts, selected, flat_options);

    Ok((facts, target))
}

/// The groups with a pick, backing `group_selected(...)`. A disjoint namespace
/// from the option names.
fn selected_groups(selected: &[String], flat_options: &[GeneratorOption]) -> Result<Vec<String>> {
    let mut groups: Vec<String> = Vec::new();
    for name in selected {
        let Some((_, option)) = find_option(name, flat_options) else {
            bail!("selected option `{name}` is not in the template");
        };
        if !option.selection_group.is_empty() && !groups.contains(&option.selection_group) {
            groups.push(option.selection_group.clone());
        }
    }
    Ok(groups)
}

/// What a selection would generate.
pub struct Planned {
    /// Each file as `(output path, contents)`.
    pub files: Vec<(String, String)>,
    /// The SDK predicates the render evaluated.
    pub predicates_used: Vec<&'static str>,
}

/// Everything this selection generates. Nothing is written.
pub fn plan(
    loaded: &Loaded,
    selected: &[String],
    flat_options: &[GeneratorOption],
    facts: &process::Facts,
) -> Result<Planned> {
    let groups = selected_groups(selected, flat_options)?;
    let renderer = process::Renderer::new(selected, &groups, facts);
    let mut load_partial = |path: &str| loaded.source.read(path).map(std::borrow::Cow::into_owned);

    let mut planned = Vec::new();
    for (source_path, contents) in loaded.source.files().map_err(|e| anyhow::anyhow!("{e}"))? {
        let source_path = source_path.as_str();
        let manifest::Emit::When { condition, output } = loaded.manifest.emit(source_path) else {
            continue;
        };
        if let Some(condition) = condition {
            let what = format!("`emit.when` condition for `{source_path}`");
            if !renderer.evaluate(condition, &what)? {
                continue;
            }
        }

        let processed = renderer
            .render(&contents, &mut load_partial)
            .map_err(|e| anyhow::anyhow!("{source_path}:{e}"))?;

        let out_path = match output {
            Some(path) => {
                let what = format!("`emit.as` path for `{source_path}`");
                renderer.output_path(path, &what)?
            }
            None => source_path.to_string(),
        };

        // A template-authored rename must not walk out of the project.
        if !process::is_safe_relative_path(&out_path) {
            bail!("template file `{source_path}` resolved to unsafe output path `{out_path}`");
        }

        planned.push((out_path, processed));
    }

    Ok(Planned {
        files: planned,
        predicates_used: renderer.predicates_used(),
    })
}

#[cfg(test)]
mod test {
    use esp_generate::template::SetValue;

    use super::*;

    /// A template `sets` key must not displace a host value of the same name.
    /// Host values are written first, and this merge never overwrites.
    #[test]
    fn a_template_set_cannot_displace_a_host_value() {
        let mut opt = GeneratorOption {
            name: "sneaky".to_string(),
            ..Default::default()
        };
        opt.sets.insert(
            "has_reserved_pins".to_string(),
            SetValue::scalar("template-wins"),
        );
        opt.sets
            .insert("its_own_key".to_string(), SetValue::scalar("kept"));

        let mut facts = process::Facts::default();
        facts.set_value("has_reserved_pins", true);

        merge_template_sets(&mut facts, &["sneaky".to_string()], &[opt]);

        assert_eq!(
            facts.values.get("has_reserved_pins"),
            Some(&process::FactValue::Bool(true)),
            "a template `sets` key displaced a host value"
        );
        assert_eq!(
            facts.values.get("its_own_key"),
            Some(&process::FactValue::Str("kept".into())),
            "a key the host never set must still come through"
        );
    }
}
