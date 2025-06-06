use std::error::Error;

use toml_edit::{DocumentMut, Item, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct CargoToml {
    pub manifest: toml_edit::DocumentMut,
}

const DEPENDENCY_KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

impl CargoToml {
    pub fn load(manifest: &str) -> Result<Self> {
        // Parse the manifest string into a mutable TOML document.
        Ok(Self {
            manifest: manifest.parse::<DocumentMut>()?,
        })
    }

    pub fn is_published(&self) -> bool {
        // Check if the package is published by looking for the `publish` key
        // in the manifest.
        let Item::Table(package) = &self.manifest["package"] else {
            unreachable!("The package table is missing in the manifest");
        };

        let Some(publish) = package.get("publish") else {
            return true;
        };

        publish.as_bool().unwrap_or(true)
    }

    pub fn version(&self) -> &str {
        self.manifest["package"]["version"]
            .as_str()
            .unwrap()
            .trim()
            .trim_matches('"')
    }

    pub fn msrv(&self) -> &str {
        self.manifest["package"]["rust-version"]
            .as_str()
            .unwrap()
            .trim()
            .trim_matches('"')
    }

    /// Calls a callback for each table that contains dependencies.
    ///
    /// Callback arguments:
    /// - `path`: The path to the table (e.g. `dependencies.package`)
    /// - `dependency_kind`: The kind of dependency (e.g. `dependencies`,
    ///   `dev-dependencies`)
    /// - `table`: The table itself
    pub fn visit_dependencies(
        &self,
        mut handle_dependencies: impl FnMut(&str, &'static str, &toml_edit::Table),
    ) {
        fn recurse_dependencies(
            path: String,
            table: &toml_edit::Table,
            handle_dependencies: &mut impl FnMut(&str, &'static str, &toml_edit::Table),
        ) {
            // Walk through tables recursively so that we can find *all* dependencies.
            for (key, item) in table.iter() {
                if let Item::Table(table) = item {
                    let path = if path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{path}.{key}")
                    };
                    recurse_dependencies(path, table, handle_dependencies);
                }
            }
            for dependency_kind in DEPENDENCY_KINDS {
                let Some(Item::Table(table)) = table.get(dependency_kind) else {
                    continue;
                };

                handle_dependencies(&path, dependency_kind, table);
            }
        }

        recurse_dependencies(
            String::new(),
            self.manifest.as_table(),
            &mut handle_dependencies,
        );
    }

    pub fn dependency_version(&self, package_name: &str) -> String {
        let mut dep_version = String::new();
        self.visit_dependencies(|_, _, table| {
            // Update dependencies which specify a version:
            if !table.contains_key(package_name) {
                return;
            }
            match &table[package_name] {
                Item::Value(Value::String(value)) => {
                    // package = "version"
                    dep_version = value.value().to_string();
                }
                Item::Table(table) if table.contains_key("version") => {
                    // [package]
                    // version = "version"
                    dep_version = table["version"].as_value().unwrap().to_string();
                }
                Item::Value(Value::InlineTable(table)) if table.contains_key("version") => {
                    // package = { version = "version" }
                    dep_version = table["version"].as_str().unwrap().to_string();
                }
                Item::None => {
                    // alias = { package = "foo", version = "version" }
                    let update_renamed_dep = table.get_values().iter().find_map(|(k, p)| {
                        if let Value::InlineTable(table) = p {
                            if let Some(Value::String(name)) = &table.get("package") {
                                if name.value() == package_name {
                                    // Return the actual key of this dependency, e.g.:
                                    // `procmacros = { package = "esp-hal-procmacros" }`
                                    //  ^^^^^^^^^^
                                    return Some(k.last().unwrap().get().to_string());
                                }
                            }
                        }

                        None
                    });

                    if let Some(dependency_name) = update_renamed_dep {
                        dep_version = table[&dependency_name]["version"]
                            .as_value()
                            .unwrap()
                            .to_string();
                    }
                }
                _ => {}
            }
        });

        dep_version.trim_start_matches('=').to_string()
    }
}
