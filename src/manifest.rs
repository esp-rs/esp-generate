//! The template manifest — `metadata.toml`.
//!
//! Read by the binary before the SDK runs. The split is the parse boundary:
//! this file decides *which* files a project gets and what they are called,
//! `template.yaml` decides what is *in* them.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::{TemplateSource, contract};
use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Template machinery — option-tree fragments and `include` partials — which is
/// never emitted, wherever a template puts it. Structural rather than a
/// per-file `when = false`, which would look like a selection could turn it on.
pub const RESERVED_DIR: &str = ".template";

/// The manifest itself, and the option tree, are the binary's and the SDK's
/// inputs — never output.
pub const MANIFEST_PATH: &str = "metadata.toml";
const OPTION_TREE_PATH: &str = "template.yaml";

/// Whether `path` is template machinery rather than a candidate output file.
///
/// Both separators, as [`crate::process::is_safe_relative_path`] accepts both.
fn is_reserved(path: &str) -> bool {
    path == MANIFEST_PATH
        || path == OPTION_TREE_PATH
        || path
            .split_once(['/', '\\'])
            .is_some_and(|(head, _)| head == RESERVED_DIR)
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// The `esp-template-sdk` version this template is written against.
    pub sdk_version: contract::Version,
    #[serde(default)]
    #[allow(dead_code)]
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub description: String,
    /// Plugins this template depends on, in declaration order — which decides
    /// which one wins if two contribute the same name, so a sorted map would
    /// silently make that alphabetical.
    #[serde(default)]
    pub plugins: IndexMap<String, contract::Version>,
    /// Per-file emission rules. Absent means "emitted, under its own path".
    #[serde(default)]
    emit: Vec<EmitRule>,
}

#[derive(Debug, Deserialize)]
struct EmitRule {
    /// Source path, template-root-relative.
    path: String,
    /// A somni condition; the file is emitted only when it holds.
    when: Option<String>,
    /// The output path, if it differs from `path`. Template text, so
    /// `{{ name }}` interpolates the same way a file body does.
    #[serde(rename = "as")]
    output: Option<String>,
}

/// What the binary should do with one template file.
#[derive(Debug, PartialEq, Eq)]
pub enum Emit<'a> {
    /// Not an output file at all: machinery, or one of the two manifests.
    Never,
    /// Emitted when `condition` holds (always, if `None`), at `output` — the
    /// source path unless the manifest renamed it.
    When {
        condition: Option<&'a str>,
        output: Option<&'a str>,
    },
}

impl Manifest {
    /// Read, parse and validate the manifest of `source`. Every check lives
    /// here, so a caller cannot get a half-checked one.
    pub fn load(source: &TemplateSource) -> Result<Self> {
        let raw = source
            .get(MANIFEST_PATH)
            .with_context(|| format!("template is missing `{MANIFEST_PATH}`"))?;

        let manifest = Self::parse(&raw)?;
        manifest.validate_paths(|path| source.get(path).is_some())?;
        Ok(manifest)
    }

    /// Parse a manifest and check the template is one this binary can run,
    /// before any template file is touched.
    pub fn parse(source: &str) -> Result<Self> {
        let manifest: Manifest = toml_edit::de::from_str(source)
            .context("`metadata.toml` is not a valid template manifest")?;

        let sdk = &*contract::SDK_VERSION;
        if !contract::is_compatible(sdk, &manifest.sdk_version) {
            bail!(
                "this template needs esp-template-sdk {}, but this esp-generate has {sdk}",
                manifest.sdk_version
            );
        }

        let mut seen = HashSet::new();
        for rule in &manifest.emit {
            if !seen.insert(rule.path.as_str()) {
                bail!(
                    "`metadata.toml` has more than one `emit` rule for `{}`",
                    rule.path
                );
            }

            if is_reserved(&rule.path) {
                bail!(
                    "`metadata.toml` has an `emit` rule for `{}`, which is never emitted \
                     (`{MANIFEST_PATH}`, `{OPTION_TREE_PATH}` and anything under `{RESERVED_DIR}` \
                     are template machinery)",
                    rule.path
                );
            }
        }

        Ok(manifest)
    }

    /// What to do with the template file at `path`.
    pub fn emit(&self, path: &str) -> Emit<'_> {
        if is_reserved(path) {
            return Emit::Never;
        }

        match self.emit.iter().find(|rule| rule.path == path) {
            Some(rule) => Emit::When {
                condition: rule.when.as_deref(),
                output: rule.output.as_deref(),
            },
            None => Emit::When {
                condition: None,
                output: None,
            },
        }
    }

    /// Check every `emit` rule points at a file that exists.
    ///
    /// A stale rule is silently inert: the file it was meant to guard gets
    /// emitted unconditionally, or not at all.
    fn validate_paths(&self, exists: impl Fn(&str) -> bool) -> Result<()> {
        let stale: Vec<&str> = self
            .emit
            .iter()
            .map(|rule| rule.path.as_str())
            .filter(|path| !exists(path))
            .collect();

        if !stale.is_empty() {
            bail!(
                "`metadata.toml` has `emit` rules for files that do not exist: {}",
                stale.join(", ")
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const MINIMAL: &str = r#"
sdk_version = "0.1.0"
"#;

    #[test]
    fn an_unruled_file_is_emitted_under_its_own_path() {
        let manifest = Manifest::parse(MINIMAL).unwrap();
        assert_eq!(
            manifest.emit("src/bin/main.rs"),
            Emit::When {
                condition: None,
                output: None
            }
        );
    }

    #[test]
    fn machinery_is_never_emitted() {
        let manifest = Manifest::parse(MINIMAL).unwrap();

        for path in [
            "metadata.toml",
            "template.yaml",
            ".template/chip.yaml",
            ".template/partials/main_async.rs",
        ] {
            assert_eq!(manifest.emit(path), Emit::Never, "{path}");
        }

        assert_ne!(manifest.emit("src/dot.template/x.rs"), Emit::Never);
        assert_ne!(manifest.emit(".templates/x.rs"), Emit::Never);

        assert_eq!(manifest.emit(".template\\chip.yaml"), Emit::Never);
    }

    #[test]
    fn a_rule_supplies_the_condition_and_the_output_path() {
        let manifest = Manifest::parse(
            r#"
sdk_version = "0.1.0"

emit = [
  { path = "wokwi.toml",  when = 'option("wokwi")' },
  { path = "GUIDANCE.md", when = 'group_selected("agents")', as = "{{ agent_file }}" },
]
"#,
        )
        .unwrap();

        assert_eq!(
            manifest.emit("wokwi.toml"),
            Emit::When {
                condition: Some("option(\"wokwi\")"),
                output: None
            }
        );
        assert_eq!(
            manifest.emit("GUIDANCE.md"),
            Emit::When {
                condition: Some("group_selected(\"agents\")"),
                output: Some("{{ agent_file }}")
            }
        );
    }

    #[test]
    fn an_incompatible_template_is_rejected_up_front() {
        let err = Manifest::parse("sdk_version = \"99.0.0\"\n")
            .expect_err("a future major must be refused");
        let msg = err.to_string();
        assert!(msg.contains("99.0.0"), "{msg}");
        assert!(msg.contains("esp-template-sdk"), "{msg}");
    }

    #[test]
    fn the_running_sdk_version_is_accepted() {
        let current = contract::SDK_VERSION.to_string();
        Manifest::parse(&format!("sdk_version = \"{current}\"\n"))
            .expect("the SDK must satisfy its own version");
    }

    #[test]
    fn a_manifest_without_a_version_is_not_a_template() {
        let err = Manifest::parse("name = \"nope\"\n").expect_err("missing version must fail");
        assert!(err.to_string().contains("metadata.toml"), "{err}");
    }

    #[test]
    fn a_rule_for_a_missing_file_is_rejected() {
        let manifest = Manifest::parse(
            r#"
sdk_version = "0.1.0"

[[emit]]
path = "was/renamed.rs"
when = 'option("x")'
"#,
        )
        .unwrap();

        let err = manifest
            .validate_paths(|_| false)
            .expect_err("a stale rule is inert, so it must be loud");
        assert!(err.to_string().contains("was/renamed.rs"), "{err}");

        manifest
            .validate_paths(|_| true)
            .expect("a rule whose file exists is fine");
    }

    /// Both are ordinary TOML for the same value, so the formatting is not
    /// load-bearing.
    #[test]
    fn inline_and_block_emit_rules_are_the_same_manifest() {
        let inline = Manifest::parse(
            r#"
sdk_version = "0.1.0"
emit = [{ path = "wokwi.toml", when = 'option("wokwi")' }]
"#,
        )
        .unwrap();

        let block = Manifest::parse(
            r#"
sdk_version = "0.1.0"

[[emit]]
path = "wokwi.toml"
when = 'option("wokwi")'
"#,
        )
        .unwrap();

        assert_eq!(inline.emit("wokwi.toml"), block.emit("wokwi.toml"));
    }

    /// A sorted map would make this alphabetical — `board` would beat `chip`.
    #[test]
    fn plugins_keep_their_declaration_order() {
        let manifest = Manifest::parse(
            r#"
sdk_version = "0.1.0"
plugins = { chip = "0.4.0", board = "1.0.0" }
"#,
        )
        .unwrap();

        let order: Vec<&str> = manifest.plugins.keys().map(String::as_str).collect();
        assert_eq!(order, ["chip", "board"], "declaration order was not kept");
    }

    #[test]
    fn a_rule_for_template_machinery_is_rejected() {
        for path in [
            ".template/partials/main.rs",
            "metadata.toml",
            "template.yaml",
        ] {
            let err = Manifest::parse(&format!(
                "sdk_version = \"0.1.0\"\nemit = [{{ path = \"{path}\", when = 'false' }}]\n"
            ))
            .expect_err("a rule for machinery is inert, so it must be loud");
            assert!(err.to_string().contains(path), "{err}");
        }

        Manifest::parse(
            "sdk_version = \"0.1.0\"\nemit = [{ path = \"src/dot.template/x.rs\", when = 'false' }]\n",
        )
        .expect("a path that only looks reserved is a normal file");
    }

    #[test]
    fn duplicate_rules_are_rejected() {
        let err = Manifest::parse(
            r#"
sdk_version = "0.1.0"

[[emit]]
path = "wokwi.toml"
when = 'option("wokwi")'

[[emit]]
path = "wokwi.toml"
when = 'false'
"#,
        )
        .expect_err("two rules for one path is ambiguous");
        assert!(err.to_string().contains("wokwi.toml"), "{err}");
    }
}
