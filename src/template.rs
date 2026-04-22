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

#[derive(Clone, Serialize, Deserialize)]
pub struct Template {
    pub options: Vec<GeneratorOptionItem>,
}

impl Template {
    pub fn all_options(&self) -> Vec<&GeneratorOption> {
        all_options_in(&self.options)
    }
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
