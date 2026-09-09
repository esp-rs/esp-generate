//! Template plugins — versioned bundles of domain data.
//!
//! A plugin supplies what templates reference but the SDK knows nothing about:
//! the chip list, its capability symbols, the fragment that offers them. A
//! template pins the versions it needs in `metadata.toml`:
//!
//! ```toml
//! plugins = { chip = "0.4.0" }
//! ```
//!
//! A plugin's version tracks the data it wraps, so it names a vocabulary. A
//! host may register several versions of one plugin; the SDK links one version
//! of itself but any number of plugins.

use indexmap::IndexMap;
use semver::Version;

use crate::process::Facts;
use crate::template::{GeneratorOption, SetValue};

/// The current selection, as plugins see it.
#[derive(Debug, Default, Clone)]
pub struct Selection {
    /// Every selected option name.
    pub options: Vec<String>,
    /// The pick for each selection group that has one, keyed by group name. A
    /// plugin that owns a group reads its pick here rather than guessing from
    /// [`Self::options`], where an unrelated option could share the name.
    pub groups: IndexMap<String, String>,
    /// The `sets` of every selected option, merged, first writer winning. A
    /// plugin whose facts depend on option-scoped data reads it here.
    pub sets: IndexMap<String, SetValue>,
}

impl Selection {
    /// The option picked for `group`, if any.
    pub fn group(&self, group: &str) -> Option<&str> {
        self.groups.get(group).map(String::as_str)
    }

    /// The list-valued `sets` entry `key`, or empty if absent or scalar.
    pub fn set_list(&self, key: &str) -> &[String] {
        self.sets
            .get(key)
            .and_then(SetValue::as_list)
            .unwrap_or(&[])
    }
}

/// Marks an include as plugin-provided: `plugin:<name>`, or
/// `plugin:<name>:<fragment>` for a plugin offering several.
pub(crate) const SCHEME: &str = "plugin:";

/// A versioned bundle of domain data a template can depend on.
pub trait TemplatePlugin: std::fmt::Debug + Send + Sync {
    /// The name a template pins in `[plugins]`, e.g. `chip`.
    fn name(&self) -> &str;

    /// The version a template pins. Tracks the data this plugin wraps, so it
    /// names a vocabulary rather than a release.
    fn version(&self) -> Version;

    /// The option-tree fragment called `name`, if this plugin has one.
    fn fragment(&self, _name: &str) -> Option<String> {
        None
    }

    /// Facts this plugin contributes for the current selection. Called on every
    /// selection change, not only at generation — option gating reads these.
    fn facts(&self, _selection: &Selection) -> Facts {
        Facts::default()
    }

    /// The fact namespaces this plugin contributes, whatever the selection.
    fn namespaces(&self) -> Vec<String> {
        vec![self.name().to_string()]
    }
}

/// The [`Selection`] a set of option names amounts to.
pub fn selection(options: Vec<String>, flat: &[GeneratorOption]) -> Selection {
    let mut groups = IndexMap::new();
    let mut sets: IndexMap<String, SetValue> = IndexMap::new();
    for name in &options {
        let Some((_, opt)) = crate::config::find_option(name, flat) else {
            continue;
        };
        if !opt.selection_group.is_empty() {
            groups
                .entry(opt.selection_group.clone())
                .or_insert_with(|| name.clone());
        }
        for (key, value) in &opt.sets {
            sets.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Selection {
        options,
        groups,
        sets,
    }
}

/// The plugins a host offers, and the lookup a template resolves against.
#[derive(Default)]
pub struct Plugins {
    registered: Vec<Box<dyn TemplatePlugin>>,
}

impl Plugins {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer a plugin. A host may register several versions of the same name.
    pub fn register(&mut self, plugin: impl TemplatePlugin + 'static) -> &mut Self {
        self.registered.push(Box::new(plugin));
        self
    }

    /// Pick the plugins satisfying `required`, or say what is missing.
    ///
    /// Declaration order is preserved, so it decides a clash. Version matching
    /// is [`crate::contract::is_compatible`], as for the SDK version gate.
    pub fn resolve(&self, required: &IndexMap<String, Version>) -> Result<Resolved<'_>, String> {
        let mut chosen: Vec<&dyn TemplatePlugin> = Vec::new();

        for (name, want) in required {
            let candidates: Vec<&dyn TemplatePlugin> = self
                .registered
                .iter()
                .map(|p| p.as_ref())
                .filter(|p| p.name() == name)
                .collect();

            if candidates.is_empty() {
                return Err(format!(
                    "this template needs the `{name}` plugin, which this esp-generate does not provide{}",
                    self.offered()
                ));
            }

            // Newest satisfying version, not whichever was registered first.
            let best = candidates
                .iter()
                .filter(|p| crate::contract::is_compatible(&p.version(), want))
                .max_by_key(|p| p.version());

            match best {
                Some(p) => chosen.push(*p),
                None => {
                    let have: Vec<String> =
                        candidates.iter().map(|p| p.version().to_string()).collect();
                    return Err(format!(
                        "this template needs `{name}` plugin {want}, but this esp-generate has {}",
                        have.join(", ")
                    ));
                }
            }
        }

        Ok(Resolved { chosen })
    }

    fn offered(&self) -> String {
        if self.registered.is_empty() {
            return String::new();
        }
        let names: Vec<String> = self
            .registered
            .iter()
            .map(|p| format!("`{}` {}", p.name(), p.version()))
            .collect();
        format!(" (it provides: {})", names.join(", "))
    }
}

/// A supplied vocabulary widens the merged one; an absent one leaves it alone,
/// so one plugin declining to supply names cannot turn the check off.
fn merge_vocabulary(
    into: &mut Option<std::collections::HashSet<String>>,
    from: Option<std::collections::HashSet<String>>,
) {
    if let Some(from) = from {
        into.get_or_insert_with(Default::default).extend(from);
    }
}

fn clash(kind: &str, name: &str, first: &str, second: &str) -> String {
    format!(
        "plugins `{first}` and `{second}` both contribute the {kind} `{name}`; \
         a template cannot say which it means"
    )
}

/// The plugins a template resolved to, in the order it asked for them.
#[derive(Debug, Default)]
pub struct Resolved<'a> {
    chosen: Vec<&'a dyn TemplatePlugin>,
}

impl Resolved<'_> {
    /// Resolve the part of an include path after the `plugin:` scheme.
    ///
    /// `chip` means the `chip` plugin's fragment of the same name; `chip:boards`
    /// names one directly. An undeclared plugin and a missing fragment are
    /// different mistakes and report separately.
    pub fn fragment(&self, spec: &str) -> Result<String, String> {
        let (plugin_name, fragment_name) = match spec.split_once(':') {
            Some((plugin, fragment)) => (plugin, fragment),
            None => (spec, spec),
        };

        let plugin = self
            .chosen
            .iter()
            .find(|p| p.name() == plugin_name)
            .ok_or_else(|| {
                format!(
                    "`{SCHEME}{spec}` needs the `{plugin_name}` plugin, which this template does \
                     not declare in `[plugins]`"
                )
            })?;

        plugin
            .fragment(fragment_name)
            .ok_or_else(|| format!("plugin `{plugin_name}` has no fragment `{fragment_name}`"))
    }

    /// Every fact namespace the resolved plugins contribute, in declaration
    /// order.
    pub fn namespaces(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for plugin in &self.chosen {
            for namespace in plugin.namespaces() {
                if !all.contains(&namespace) {
                    all.push(namespace);
                }
            }
        }
        all
    }

    /// Every resolved plugin's facts for `selection`, merged.
    ///
    /// Two plugins contributing the same name is an error, not a silent win:
    /// which one a template got would otherwise depend on `[plugins]` order.
    pub fn facts(&self, selection: &Selection) -> Result<Facts, String> {
        let mut merged = Facts::default();
        let mut namespace_owner: IndexMap<String, &str> = IndexMap::new();
        let mut value_owner: IndexMap<String, &str> = IndexMap::new();

        for plugin in &self.chosen {
            let facts = plugin.facts(selection);

            for (name, fields) in facts.structs {
                if let Some(first) = namespace_owner.insert(name.clone(), plugin.name()) {
                    return Err(clash("namespace", &name, first, plugin.name()));
                }
                merged.structs.insert(name, fields);
            }
            let mut values: Vec<_> = facts.values.into_iter().collect();
            values.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in values {
                if let Some(first) = value_owner.insert(key.clone(), plugin.name()) {
                    return Err(clash("value", &key, first, plugin.name()));
                }
                merged.set_value(key, value);
            }
            merge_vocabulary(&mut merged.vocabulary.options, facts.vocabulary.options);
            merge_vocabulary(&mut merged.vocabulary.groups, facts.vocabulary.groups);
        }
        Ok(merged)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contract::release;

    #[derive(Debug)]
    struct Stub {
        name: &'static str,
        version: Version,
    }

    impl TemplatePlugin for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn version(&self) -> Version {
            self.version.clone()
        }
        fn fragment(&self, name: &str) -> Option<String> {
            match name {
                n if n == self.name => Some(format!("fragment for {} {}", self.name, self.version)),
                "extra" => Some(format!("extra fragment for {}", self.name)),
                _ => None,
            }
        }
        fn facts(&self, selection: &Selection) -> Facts {
            let mut facts = Facts::default();
            facts.set_value("vocabulary", format!("{} {}", self.name, self.version));
            facts.set_value(
                "remove_pins_seen",
                selection.set_list("remove_pins").len() as u64,
            );
            facts
        }
    }

    fn host(plugins: &[(&'static str, Version)]) -> Plugins {
        let mut registry = Plugins::new();
        for (name, version) in plugins {
            registry.register(Stub {
                name,
                version: version.clone(),
            });
        }
        registry
    }

    fn need(pairs: &[(&str, Version)]) -> IndexMap<String, Version> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn a_matching_version_resolves() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let resolved = host.resolve(&need(&[("chip", release(0, 4, 0))])).unwrap();
        assert_eq!(
            resolved.fragment("chip").as_deref(),
            Ok("fragment for chip 0.4.0")
        );
    }

    #[test]
    fn a_missing_plugin_says_what_the_host_offers() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let err = host
            .resolve(&need(&[("board", release(1, 0, 0))]))
            .expect_err("an unprovided plugin must be refused");
        assert!(err.contains("`board`"), "{err}");
        assert!(
            err.contains("chip"),
            "should list what it does provide: {err}"
        );
    }

    #[test]
    fn an_unsatisfiable_version_names_what_is_available() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let err = host
            .resolve(&need(&[("chip", release(0, 5, 0))]))
            .expect_err("a newer vocabulary must be refused");
        assert!(err.contains("0.5.0"), "{err}");
        assert!(err.contains("0.4.0"), "{err}");
    }

    /// A host may offer several vocabularies at once, so a template pinned to
    /// an older one keeps working.
    #[test]
    fn a_host_can_offer_several_versions_of_one_plugin() {
        let host = host(&[("chip", release(0, 4, 0)), ("chip", release(0, 5, 0))]);

        for want in [release(0, 4, 0), release(0, 5, 0)] {
            let resolved = host.resolve(&need(&[("chip", want.clone())])).unwrap();
            assert_eq!(
                resolved.fragment("chip").as_deref(),
                Ok(format!("fragment for chip {want}").as_str()),
                "asked for {want}"
            );
        }
    }

    /// The point of the whole registry: two templates pinned to different
    /// versions of one plugin each see their own vocabulary, in one binary.
    #[test]
    fn each_template_gets_the_facts_of_the_version_it_pinned() {
        let host = host(&[("chip", release(0, 4, 0)), ("chip", release(0, 5, 0))]);
        let selection = Selection::default();

        for want in [release(0, 4, 0), release(0, 5, 0)] {
            let resolved = host.resolve(&need(&[("chip", want.clone())])).unwrap();
            assert_eq!(
                resolved.facts(&selection).unwrap().values.get("vocabulary"),
                Some(&crate::process::FactValue::Str(format!("chip {want}"))),
                "asked for {want}"
            );
        }
    }

    /// A plugin whose facts depend on option-scoped data reads it off the
    /// selection rather than the host computing it and handing over a value.
    #[test]
    fn a_plugin_reads_option_scoped_sets() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let resolved = host.resolve(&need(&[("chip", release(0, 4, 0))])).unwrap();

        let selection = Selection {
            sets: [(
                "remove_pins".to_string(),
                SetValue::List(vec!["spi_flash".into(), "spi_psram".into()]),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(
            resolved
                .facts(&selection)
                .unwrap()
                .values
                .get("remove_pins_seen"),
            Some(&crate::process::FactValue::Int(2))
        );
    }

    #[test]
    fn two_plugins_contributing_one_name_is_an_error() {
        // Both stubs write `vocabulary`, so neither can be said to own it.
        let host = host(&[("chip", release(0, 4, 0)), ("board", release(1, 0, 0))]);
        let resolved = host
            .resolve(&need(&[
                ("chip", release(0, 4, 0)),
                ("board", release(1, 0, 0)),
            ]))
            .unwrap();

        let err = resolved
            .facts(&Selection::default())
            .expect_err("a silent winner would depend on `[plugins]` order");
        assert!(err.contains("chip") && err.contains("board"), "{err}");

        // Both stubs write both names, so the one reported must not vary.
        for _ in 0..8 {
            assert_eq!(resolved.facts(&Selection::default()).unwrap_err(), err);
        }
    }

    #[test]
    fn declaration_order_decides_a_clash() {
        let host = host(&[("chip", release(0, 4, 0)), ("board", release(1, 0, 0))]);

        let chip_first = host
            .resolve(&need(&[
                ("chip", release(0, 4, 0)),
                ("board", release(1, 0, 0)),
            ]))
            .unwrap();
        let board_first = host
            .resolve(&need(&[
                ("board", release(1, 0, 0)),
                ("chip", release(0, 4, 0)),
            ]))
            .unwrap();

        assert!(chip_first.fragment("chip").is_ok());
        assert!(board_first.fragment("board").is_ok());
        assert_ne!(
            chip_first.fragment("chip"),
            chip_first.fragment("board"),
            "the two stubs must be distinguishable for this to mean anything"
        );
    }

    #[test]
    fn a_plugin_may_offer_several_fragments() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let resolved = host.resolve(&need(&[("chip", release(0, 4, 0))])).unwrap();

        assert_eq!(
            resolved.fragment("chip"),
            resolved.fragment("chip:chip"),
            "the short form must mean the same as naming the fragment"
        );
        assert_eq!(
            resolved.fragment("chip:extra").as_deref(),
            Ok("extra fragment for chip")
        );
    }

    #[test]
    fn a_missing_plugin_and_a_missing_fragment_read_differently() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let resolved = host.resolve(&need(&[("chip", release(0, 4, 0))])).unwrap();

        let undeclared = resolved.fragment("board").unwrap_err();
        assert!(undeclared.contains("`[plugins]`"), "{undeclared}");
        assert!(undeclared.contains("board"), "{undeclared}");

        let no_such_fragment = resolved.fragment("chip:nope").unwrap_err();
        assert!(
            no_such_fragment.contains("has no fragment"),
            "{no_such_fragment}"
        );
        assert!(no_such_fragment.contains("nope"), "{no_such_fragment}");
        assert!(
            !no_such_fragment.contains("`[plugins]`"),
            "the plugin *is* declared; that must not be the complaint: {no_such_fragment}"
        );
    }

    #[test]
    fn a_template_needing_no_plugin_resolves_to_nothing() {
        let host = host(&[("chip", release(0, 4, 0))]);
        let resolved = host.resolve(&IndexMap::new()).unwrap();
        assert!(resolved.fragment("chip").is_err());
    }
}
