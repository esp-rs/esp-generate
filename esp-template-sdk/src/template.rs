use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::plugin::{Resolved, SCHEME as PLUGIN_SCHEME};

/// Map from selection-group name to the subset of options in that group that
/// are compatible with this item. If any listed group has no current selection
/// — or its selection is outside the allow-list — the item is considered
/// incompatible and is hidden from the TUI (and its selection, if any, is
/// cleared). Absent groups are unconstrained.
///
/// This generalises what `chips: [...]` used to express: a chip restriction
/// is now just a `compatible: { chip: [...] }` entry driven by the `chip`
/// selection group the TUI populates at runtime.
pub type CompatibilityRequirements = IndexMap<String, Vec<String>>;

/// Value carried by a [`GeneratorOption::sets`] entry.
///
/// Using `#[serde(untagged)]` lets template authors write the YAML form
/// that reads most naturally for the datum at hand:
///
/// ```yaml
/// sets:
///   coding_agent_guidance_file: CLAUDE.md       # scalar -> Scalar
///   remove_pins: [spi_flash, spi_psram]         # sequence -> List
/// ```
///
/// Consumers branch on the variant:
///
///   * Scalars are merged into the fact values a template interpolates; list
///     entries are skipped, having no single-string form.
///   * Code-generation paths (e.g. the module pin-reservation block) read
///     the specific list-keys they care about, asserting via `as_list`.
///
/// Keeping both shapes in one container means `sets` stays the sole
/// mechanism for option-scoped data — no parallel fields on
/// [`GeneratorOption`] for every new datum shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SetValue {
    Scalar(String),
    List(Vec<String>),
}

impl SetValue {
    /// Convenience constructor for scalar values, so generator-side code
    /// that synthesises a `sets`-like context (e.g. `project-name`,
    /// `mcu`) doesn't have to spell out the variant every time.
    pub fn scalar(s: impl Into<String>) -> Self {
        Self::Scalar(s.into())
    }

    /// Returns `Some(&str)` if this value is a scalar, `None` for lists.
    /// Value merging uses this to treat list-valued keys as "not applicable"
    /// rather than producing garbage substitutions.
    pub fn as_scalar(&self) -> Option<&str> {
        match self {
            Self::Scalar(s) => Some(s.as_str()),
            Self::List(_) => None,
        }
    }

    /// Returns `Some(&[String])` if this value is a list, `None` for
    /// scalars. Used by code-generation paths that expect a list-shaped
    /// entry under a specific well-known key.
    pub fn as_list(&self) -> Option<&[String]> {
        match self {
            Self::List(xs) => Some(xs.as_slice()),
            Self::Scalar(_) => None,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeneratorOption {
    pub name: String,
    pub display_name: String,
    /// Selecting one option in the group deselect other options of the same group.
    #[serde(default)]
    pub selection_group: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Per-selection-group compatibility allow-lists. See
    /// [`CompatibilityRequirements`].
    #[serde(default)]
    pub compatible: CompatibilityRequirements,
    /// Option-scoped template data, merged in per selected option. Scalars
    /// feed interpolation; lists feed code-generation paths. On a collision
    /// with a generator-provided fact, the generator wins.
    #[serde(default)]
    pub sets: IndexMap<String, SetValue>,
    /// Plugin fields that must be true for this option to be compatible (AND).
    /// Each entry is `<namespace>.<field>`, e.g. `chip.soc_has_wifi`; empty =
    /// no gate.
    #[serde(default)]
    pub requires_capabilities: Vec<String>,
    /// Host tools to pre-flight when this option is selected (e.g.
    /// `[probe-rs]`). The option only declares the need; the host owns the
    /// check. An open list — an unrecognised name is ignored.
    #[serde(default)]
    pub requires_tools: Vec<String>,
    /// When set, only nightly toolchains are offered while this option is
    /// selected. The host reads it off the selected options.
    #[serde(default)]
    pub requires_nightly: bool,
}

impl GeneratorOption {
    pub fn options(&self) -> Vec<String> {
        vec![self.name.to_string()]
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeneratorOptionCategory {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub options: Vec<GeneratorOptionItem>,
}

impl GeneratorOptionCategory {
    pub fn options(&self) -> Vec<String> {
        let mut res = Vec::new();
        for option in self.options.iter() {
            res.extend(option.options());
        }
        res
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum GeneratorOptionItem {
    Category(GeneratorOptionCategory),
    Option(GeneratorOption),
}

impl GeneratorOptionItem {
    pub fn title(&self) -> &str {
        match self {
            GeneratorOptionItem::Category(category) => category.display_name.as_str(),
            GeneratorOptionItem::Option(option) => option.display_name.as_str(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            GeneratorOptionItem::Category(category) => category.name.as_str(),
            GeneratorOptionItem::Option(option) => option.name.as_str(),
        }
    }

    pub fn options(&self) -> Vec<String> {
        match self {
            GeneratorOptionItem::Category(category) => category.options(),
            GeneratorOptionItem::Option(option) => option.options(),
        }
    }

    pub fn is_category(&self) -> bool {
        matches!(self, GeneratorOptionItem::Category(_))
    }

    /// Per-group compatibility allow-list for this item. Categories don't
    /// currently carry their own compatibility constraints — they derive
    /// from their children via
    /// [`ActiveConfiguration::is_active`](crate::config::ActiveConfiguration::is_active).
    pub fn compatible(&self) -> Option<&CompatibilityRequirements> {
        match self {
            GeneratorOptionItem::Category(_) => None,
            GeneratorOptionItem::Option(option) => Some(&option.compatible),
        }
    }

    pub fn requires(&self) -> &[String] {
        match self {
            GeneratorOptionItem::Category(category) => category.requires.as_slice(),
            GeneratorOptionItem::Option(option) => option.requires.as_slice(),
        }
    }

    pub fn help(&self) -> &str {
        match self {
            GeneratorOptionItem::Category(category) => &category.help,
            GeneratorOptionItem::Option(option) => &option.help,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Template {
    pub options: Vec<GeneratorOptionItem>,
    /// Names of selection groups that must have at least one option selected
    /// before generation can proceed. A missing required group is a hard
    /// error in headless mode and blocks the Save action in the TUI — the
    /// TUI also surfaces which groups still need a pick so the user knows
    /// what's gating the "save and generate" key.
    #[serde(default)]
    pub required: Vec<String>,
}

impl Template {
    pub fn all_options(&self) -> Vec<&GeneratorOption> {
        all_options_in(&self.options)
    }

    /// Validate that every entry in [`Self::required`] is non-empty and
    /// names a selection group that actually carries at least one option
    /// in the template.
    pub fn validate_required(&self) -> Result<(), String> {
        let all = self.all_options();
        for group in &self.required {
            if group.is_empty() {
                return Err("required selection group names must be non-empty".into());
            }
            let has_member = all.iter().any(|o| o.selection_group == *group);
            if !has_member {
                return Err(format!(
                    "required selection group `{group}` has no options in the template"
                ));
            }
        }
        Ok(())
    }

    /// Validate that every `requires_capabilities` entry is `<namespace>.<field>`
    /// with a namespace one of `plugins` contributes.
    ///
    /// The gate reads a malformed or unprovided entry as false for every
    /// selection, so the option would silently never be offered.
    pub fn validate_capabilities(&self, plugins: &Resolved<'_>) -> Result<(), String> {
        let namespaces = plugins.namespaces();
        let available = || match namespaces.as_slice() {
            [] => "this template declares no plugins".to_string(),
            names => format!("available: {}", names.join(", ")),
        };

        for option in self.all_options() {
            for capability in &option.requires_capabilities {
                let bad = |why: &str| {
                    Err(format!(
                        "option `{}` requires capability `{capability}`, {why} ({})",
                        option.name,
                        available()
                    ))
                };

                let Some((namespace, field)) = capability.split_once('.') else {
                    return bad("which is not of the form `<namespace>.<field>`");
                };
                if field.is_empty() {
                    return bad("which names no field");
                }
                if !namespaces.iter().any(|n| n == namespace) {
                    return bad(&format!(
                        "but no declared plugin provides `{namespace}` facts"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Return the required groups that are not satisfied by the given set
    /// of selected option names.
    pub fn missing_required_groups(&self, selected_names: &[String]) -> Vec<String> {
        let all = self.all_options();
        self.required
            .iter()
            .filter(|group| {
                !selected_names.iter().any(|name| {
                    all.iter()
                        .any(|o| &o.selection_group == *group && &o.name == name)
                })
            })
            .cloned()
            .collect()
    }

    /// Parses a bundled [`Template`] from its YAML root document,
    /// expanding every `!Include <path>` tagged node by looking up
    /// `<path>` via `loader` and substituting the parsed contents in
    /// place.
    ///
    /// The include mechanism works at the untyped `serde_yaml::Value`
    /// layer, so it's agnostic to position — an `!Include` can appear
    /// wherever a full node is otherwise allowed. The typical use is
    /// swapping in a whole `!Category` block:
    ///
    /// ```yaml
    /// options:
    ///   - !Include chip.yaml
    /// ```
    ///
    /// The included file is parsed as a standalone YAML document and
    /// its root node replaces the `!Include` node, so that file's root
    /// must itself be a complete `!Category` (or whatever shape the
    /// call-site expects).
    ///
    /// A `plugin:` path is resolved from `plugins` before `loader` is
    /// consulted, so a fragment always comes from something the template
    /// pinned. See [`crate::plugin`].
    ///
    /// Every other path passed to `loader` is the raw string from the tag —
    /// interpreted by the loader however it sees fit. The bundled-
    /// template loader treats them as keys into `TEMPLATE_FILES` (i.e.
    /// paths relative to the `template/` root); a future per-file
    /// loader could resolve them relative to the including file.
    ///
    /// Cycles are detected and rejected rather than blowing the stack.
    pub fn load<F>(main_yaml: &str, plugins: &Resolved<'_>, loader: F) -> Result<Self, String>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut value: serde_yaml::Value =
            serde_yaml::from_str(main_yaml).map_err(|e| format!("invalid template YAML: {e}"))?;
        expand_includes(&mut value, plugins, &loader, &mut Vec::new())?;
        serde_yaml::from_value(value)
            .map_err(|e| format!("template does not conform to schema: {e}"))
    }
}

/// Recursively expand `!Include` tagged nodes in `value` using `loader`.
/// `stack` carries the include paths currently being expanded, so a file
/// that (directly or transitively) includes itself is reported rather than
/// recursed into indefinitely.
fn expand_includes<F>(
    value: &mut serde_yaml::Value,
    plugins: &Resolved<'_>,
    loader: &F,
    stack: &mut Vec<String>,
) -> Result<(), String>
where
    F: Fn(&str) -> Option<String>,
{
    use serde_yaml::Value;

    match value {
        Value::Tagged(tagged) if tagged.tag == "!Include" => {
            let path = match &tagged.value {
                Value::String(s) => s.clone(),
                other => {
                    return Err(format!("!Include expects a string path, got {:?}", other));
                }
            };

            if stack.iter().any(|p| p == &path) {
                let mut chain = stack.clone();
                chain.push(path);
                return Err(format!("template-include cycle: {}", chain.join(" -> ")));
            }

            // A plugin fragment is resolved before the loader, so a template
            // file cannot shadow one by sitting at the same path.
            let contents = match path.strip_prefix(PLUGIN_SCHEME) {
                Some(spec) => plugins.fragment(spec)?,
                None => {
                    loader(&path).ok_or_else(|| format!("template include `{path}` not found"))?
                }
            };
            let mut inner: Value = serde_yaml::from_str(&contents)
                .map_err(|e| format!("failed to parse include `{path}`: {e}"))?;

            stack.push(path);
            expand_includes(&mut inner, plugins, loader, stack)?;
            stack.pop();

            *value = inner;
        }
        // Keep walking into other tagged nodes (e.g. `!Category`, `!Option`)
        // so `!Include` can appear nested inside them, not only at the top.
        Value::Tagged(tagged) => expand_includes(&mut tagged.value, plugins, loader, stack)?,
        Value::Sequence(seq) => {
            for v in seq {
                expand_includes(v, plugins, loader, stack)?;
            }
        }
        Value::Mapping(map) => {
            for (_, v) in map.iter_mut() {
                expand_includes(v, plugins, loader, stack)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn all_options_in(options: &[GeneratorOptionItem]) -> Vec<&GeneratorOption> {
    options
        .iter()
        .flat_map(|o| match o {
            GeneratorOptionItem::Option(option) => vec![option],
            GeneratorOptionItem::Category(category) => all_options_in(&category.options),
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    /// A minimal two-file template: main references a !Include, the
    /// included file carries the actual category. After `Template::load`,
    /// the final tree looks exactly like an inlined template would.
    #[test]
    fn include_is_substituted_in_place() {
        let main = r#"
options:
  - !Include category.yaml
"#;
        let category = r#"
!Category
name: demo
display_name: Demo
options:
  - !Option
    name: demo-a
    display_name: Demo A
"#;

        let template = Template::load(main, &Resolved::default(), |path| {
            (path == "category.yaml").then(|| category.to_string())
        })
        .expect("include should resolve");

        assert_eq!(template.options.len(), 1);
        match &template.options[0] {
            GeneratorOptionItem::Category(c) => {
                assert_eq!(c.name, "demo");
                assert_eq!(c.options.len(), 1);
            }
            other => panic!("expected Category, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_key_is_refused_rather_than_ignored() {
        let on_category = r#"
options:
  - !Category
    name: peripherals
    display_name: Peripherals
    compatible:
      chip: [esp32c6]
    options: []
"#;
        let err = Template::load(on_category, &Resolved::default(), |_| None)
            .expect_err("a gate on a category must not be silently dropped");
        assert!(err.contains("compatible"), "{err}");

        let stale_chips = r#"
options:
  - !Option
    name: wifi
    display_name: Wi-Fi
    chips: [esp32c6]
"#;
        let err = Template::load(stale_chips, &Resolved::default(), |_| None)
            .expect_err("`chips:` was replaced by `compatible`");
        assert!(err.contains("chips"), "{err}");
    }

    #[test]
    fn missing_include_is_reported() {
        let main = r#"
options:
  - !Include missing.yaml
"#;
        let err = Template::load(main, &Resolved::default(), |_| None).expect_err("should error");
        assert!(err.contains("missing.yaml"), "{err}");
    }

    #[test]
    fn an_unqualified_capability_is_rejected_at_load() {
        let t = Template {
            options: vec![GeneratorOptionItem::Option(GeneratorOption {
                name: "wifi".to_string(),
                requires_capabilities: vec!["soc_has_wifi".to_string()],
                ..Default::default()
            })],
            ..Template::default()
        };

        let err = t
            .validate_capabilities(&Resolved::default())
            .expect_err("a name with no namespace must be refused");
        assert!(err.contains("soc_has_wifi"), "{err}");
        assert!(err.contains("wifi"), "{err}");
        assert!(err.contains("<namespace>.<field>"), "{err}");
    }

    #[test]
    fn a_capability_from_an_undeclared_plugin_is_rejected() {
        let t = Template {
            options: vec![GeneratorOptionItem::Option(GeneratorOption {
                name: "wifi".to_string(),
                requires_capabilities: vec!["chip.soc_has_wifi".to_string()],
                ..Default::default()
            })],
            ..Template::default()
        };

        let err = t
            .validate_capabilities(&Resolved::default())
            .expect_err("a template declaring no plugins has no `chip` facts");
        assert!(err.contains("chip"), "{err}");
    }

    #[test]
    fn validate_required_rejects_unknown_or_empty_groups() {
        // A `required` entry that doesn't name a real selection group is a
        // template bug — satisfying it is impossible, so no user-driven
        // selection could ever unblock generation. Catching it at load
        // keeps the failure close to its cause.
        let t = Template {
            required: vec!["nonexistent".to_string()],
            ..Template::default()
        };
        let err = t
            .validate_required()
            .expect_err("unknown group should fail");
        assert!(err.contains("nonexistent"), "{err}");

        let t = Template {
            required: vec![String::new()],
            ..Template::default()
        };
        let err = t
            .validate_required()
            .expect_err("empty group name should fail");
        assert!(err.contains("non-empty"), "{err}");
    }

    #[test]
    fn missing_required_groups_reports_unsatisfied_groups_only() {
        // A template that declares `chip` as required. When no chip-like
        // name is selected, `missing_required_groups` reports `chip`;
        // once a matching name is selected the group drops out.
        let main = r#"
required:
  - chip
options:
  - !Category
    name: chip
    display_name: Chip
    options:
      - !Option
        name: esp32
        display_name: ESP32
        selection_group: chip
      - !Option
        name: esp32c6
        display_name: ESP32-C6
        selection_group: chip
"#;
        let template = Template::load(main, &Resolved::default(), |_| None).expect("should parse");
        template.validate_required().expect("chip group exists");

        assert_eq!(
            template.missing_required_groups(&[]),
            vec!["chip".to_string()]
        );
        assert_eq!(
            template.missing_required_groups(&["unrelated".to_string()]),
            vec!["chip".to_string()]
        );
        assert!(
            template
                .missing_required_groups(&["esp32".to_string()])
                .is_empty()
        );
    }

    #[test]
    fn cyclic_include_is_rejected() {
        let main = r#"
options:
  - !Include a.yaml
"#;
        let a = r#"
!Category
name: a
display_name: A
options:
  - !Include b.yaml
"#;
        let b = r#"
!Category
name: b
display_name: B
options:
  - !Include a.yaml
"#;
        let err = Template::load(main, &Resolved::default(), |path| match path {
            "a.yaml" => Some(a.to_string()),
            "b.yaml" => Some(b.to_string()),
            _ => None,
        })
        .expect_err("cycle should be rejected");
        assert!(err.contains("cycle"), "{err}");
    }
}
