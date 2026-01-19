use esp_metadata_generated::MemoryRegion;
use serde::{Deserialize, Serialize};

pub mod cargo;
pub mod config;
pub mod modules;
pub mod template;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    clap::ValueEnum,
    strum::EnumIter,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Chip {
    Esp32,
    Esp32c2,
    Esp32c3,
    Esp32c6,
    Esp32h2,
    Esp32s2,
    Esp32s3,
}
impl Chip {
    pub fn metadata(self) -> esp_metadata_generated::Chip {
        match self {
            Chip::Esp32 => esp_metadata_generated::Chip::Esp32,
            Chip::Esp32c2 => esp_metadata_generated::Chip::Esp32c2,
            Chip::Esp32c3 => esp_metadata_generated::Chip::Esp32c3,
            Chip::Esp32c6 => esp_metadata_generated::Chip::Esp32c6,
            Chip::Esp32h2 => esp_metadata_generated::Chip::Esp32h2,
            Chip::Esp32s2 => esp_metadata_generated::Chip::Esp32s2,
            Chip::Esp32s3 => esp_metadata_generated::Chip::Esp32s3,
        }
    }

    pub fn wokwi(self) -> &'static str {
        match self {
            Chip::Esp32 => "board-esp32-devkit-c-v4",
            Chip::Esp32c2 => "",
            Chip::Esp32c3 => "board-esp32-c3-devkitm-1",
            Chip::Esp32c6 => "board-esp32-c6-devkitc-1",
            Chip::Esp32h2 => "board-esp32-h2-devkitm-1",
            Chip::Esp32s2 => "board-esp32-s2-devkitm-1",
            Chip::Esp32s3 => "board-esp32-s3-devkitc-1",
        }
    }

    pub fn dram2_region(self) -> &'static MemoryRegion {
        self.metadata()
            .memory_layout()
            .region("dram2_uninit")
            .expect("All chips should have a dram2_uninit region")
    }
}

/// This turns a list of strings into a sentence, and appends it to the base string.
///
/// # Example
///
/// ```rust
/// # use esp_generate::append_list_as_sentence;
/// let list = &["foo", "bar", "baz"];
/// let sentence = append_list_as_sentence("Here is a sentence.", "My elements are", list);
/// assert_eq!(sentence, "Here is a sentence. My elements are `foo`, `bar` and `baz`.");
///
/// let list = &["foo", "bar", "baz"];
/// let sentence = append_list_as_sentence("The following list is problematic:", "", list);
/// assert_eq!(sentence, "The following list is problematic: `foo`, `bar` and `baz`.");
/// ```
pub fn append_list_as_sentence<S: AsRef<str>>(base: &str, word: &str, els: &[S]) -> String {
    if !els.is_empty() {
        let mut requires = String::new();

        if !base.is_empty() {
            requires.push_str(base);
            requires.push(' ');
        }

        for (i, r) in els.iter().enumerate() {
            if i == 0 {
                if !word.is_empty() {
                    requires.push_str(word);
                    requires.push(' ');
                }
            } else if i == els.len() - 1 {
                requires.push_str(" and ");
            } else {
                requires.push_str(", ");
            };

            requires.push('`');
            requires.push_str(r.as_ref());
            requires.push('`');
        }
        requires.push('.');

        requires
    } else {
        base.to_string()
    }
}
