use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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
///   wokwi-board: board-esp32-c6-devkitc-1       # scalar -> Scalar
///   remove_pins: [spi_flash, spi_psram]         # sequence -> List
/// ```
///
/// Consumers branch on the variant:
///
///   * `#REPLACE` directives look for scalars only and silently skip list
///     entries — list-valued data is meaningful to code-generation paths
///     (pin reservations, etc.) but has no obvious single-string form for
///     textual substitution.
///   * Code-generation paths (e.g. the module pin-reservation block) read
///     the specific list-keys they care about, asserting via `as_list`.
///
/// Keeping both shapes in one container means `sets` stays the sole
/// mechanism for option-scoped data — no parallel fields on
/// [`GeneratorOption`] for every new datum shape.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Templates' `#REPLACE` machinery uses this to treat list-valued keys
    /// as "not applicable" rather than producing garbage substitutions.
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
    /// Option-scoped template data. Each selected option's `sets` entries
    /// are merged into the generation context, keyed by variable name. Values
    /// can be scalars or lists ([`SetValue`]) so the same mechanism carries
    /// heterogeneous data:
    ///
    ///   * scalars (`wokwi-board: board-...`) feed `#REPLACE` substitutions
    ///     in template files — chips that have no Wokwi model simply don't
    ///     contribute a `wokwi-board` entry;
    ///   * lists (`remove_pins: [spi_flash, spi_psram]`) feed code-generation
    ///     paths — the module pin-reservation block intersects `remove_pins`
    ///     with the chip's pin metadata to emit `let _ = peripherals.GPIOn;`
    ///     stanzas.
    ///
    /// Keys must not collide with the fixed set of generator-provided
    /// variables (`project-name`, `mcu`, `rust_target`, etc.); on collision
    /// the generator-provided value wins to preserve existing behaviour.
    #[serde(default)]
    pub sets: IndexMap<String, SetValue>,
}

impl GeneratorOption {
    pub fn options(&self) -> Vec<String> {
        vec![self.name.to_string()]
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
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
    /// from their children via [`ActiveConfiguration::is_active`].
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub options: Vec<GeneratorOptionItem>,
}

impl Template {
    pub fn all_options(&self) -> Vec<&GeneratorOption> {
        all_options_in(&self.options)
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
    /// Paths passed to `loader` are the raw strings from the tag —
    /// interpreted by the loader however it sees fit. The bundled-
    /// template loader treats them as keys into `TEMPLATE_FILES` (i.e.
    /// paths relative to the `template/` root); a future per-file
    /// loader could resolve them relative to the including file.
    ///
    /// Cycles are detected and rejected rather than blowing the stack.
    pub fn load<F>(main_yaml: &str, loader: F) -> Result<Self, String>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut value: serde_yaml::Value =
            serde_yaml::from_str(main_yaml).map_err(|e| format!("invalid template YAML: {e}"))?;
        expand_includes(&mut value, &loader, &mut Vec::new())?;
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
                    return Err(format!(
                        "!Include expects a string path, got {:?}",
                        other
                    ));
                }
            };

            if stack.iter().any(|p| p == &path) {
                let mut chain = stack.clone();
                chain.push(path);
                return Err(format!(
                    "template-include cycle: {}",
                    chain.join(" -> ")
                ));
            }

            let contents = loader(&path)
                .ok_or_else(|| format!("template include `{path}` not found"))?;
            let mut inner: Value = serde_yaml::from_str(&contents)
                .map_err(|e| format!("failed to parse include `{path}`: {e}"))?;

            stack.push(path);
            expand_includes(&mut inner, loader, stack)?;
            stack.pop();

            *value = inner;
        }
        // Keep walking into other tagged nodes (e.g. `!Category`, `!Option`)
        // so `!Include` can appear nested inside them, not only at the top.
        Value::Tagged(tagged) => expand_includes(&mut tagged.value, loader, stack)?,
        Value::Sequence(seq) => {
            for v in seq {
                expand_includes(v, loader, stack)?;
            }
        }
        Value::Mapping(map) => {
            for (_, v) in map.iter_mut() {
                expand_includes(v, loader, stack)?;
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

        let template = Template::load(main, |path| {
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
    fn missing_include_is_reported() {
        let main = r#"
options:
  - !Include missing.yaml
"#;
        let err = Template::load(main, |_| None).expect_err("should error");
        assert!(err.contains("missing.yaml"), "{err}");
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
        let err = Template::load(main, |path| match path {
            "a.yaml" => Some(a.to_string()),
            "b.yaml" => Some(b.to_string()),
            _ => None,
        })
        .expect_err("cycle should be rejected");
        assert!(err.contains("cycle"), "{err}");
    }
}
