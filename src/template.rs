use esp_metadata::Chip;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub chips: Vec<Chip>,
}

impl GeneratorOption {
    pub fn options(&self) -> Vec<String> {
        vec![self.name.to_string()]
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GeneratorOptionCategory {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub help: String,
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

#[derive(Clone, Serialize, Deserialize)]
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

    pub fn chips(&self) -> &[Chip] {
        match self {
            GeneratorOptionItem::Category(_) => &[],
            GeneratorOptionItem::Option(option) => option.chips.as_slice(),
        }
    }

    pub fn requires(&self) -> &[String] {
        match self {
            GeneratorOptionItem::Category(_) => &[],
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
