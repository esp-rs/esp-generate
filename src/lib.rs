use std::sync::LazyLock;
use anyhow::Result;
use template::Template;

pub mod cargo;
pub mod manifest;
pub mod source;

pub use esp_template_sdk::{config, contract, plugin, process, sweep, template};
pub use source::TemplateSource;

/// Build-script-generated `TEMPLATE_FILES` array mapping each file under
/// `template/` to its baked-in contents. Kept `pub` so xtask (and any other
/// consumer that needs to resolve `!Include` paths against the bundled
/// template tree) can share the same source-of-truth as the binary.
pub mod template_files;

/// The plugins this binary offers templates.
///
/// A template names what it needs in `[plugins]`; anything not registered here
/// is refused up front rather than surfacing as unknown names mid-render.
pub fn plugins() -> plugin::Plugins {
    let mut plugins = plugin::Plugins::new();
    plugins.register(esp_template_plugin_chip::ChipPlugin);
    plugins
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

static PLUGINS: LazyLock<plugin::Plugins> = LazyLock::new(plugins);

/// A template source, read and validated.
///
/// Everything downstream takes this rather than reading a global, so the source
/// is an ordinary value that a caller chooses.
pub struct Loaded {
    pub source: TemplateSource,
    pub manifest: manifest::Manifest,
    /// Borrows [`PLUGINS`], which outlives every caller.
    pub resolved: plugin::Resolved<'static>,
    pub template: Template,
}

impl Loaded {
    pub fn open(source: TemplateSource) -> Result<Self> {
        let manifest = manifest::Manifest::load(&source)?;
        let resolved = PLUGINS
            .resolve(&manifest.plugins)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let root_yaml = source
            .read("template.yaml")
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let template = {
            let load_include = |path: &str| source.get(path).map(std::borrow::Cow::into_owned);
            Template::load(root_yaml.as_ref(), &resolved, load_include)
                .map_err(|e| anyhow::anyhow!("invalid template: {e}"))?
        };

        template
            .validate_required()
            .map_err(|e| anyhow::anyhow!("invalid `required` list: {e}"))?;
        template
            .validate_capabilities(&resolved)
            .map_err(|e| anyhow::anyhow!("invalid `requires_capabilities`: {e}"))?;

        Ok(Loaded {
            source,
            manifest,
            resolved,
            template,
        })
    }
}
