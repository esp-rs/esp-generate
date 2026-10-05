//! Turning a selection into the files a project would contain. Generation
//! writes the result; `check` discards it and keeps the errors.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use esp_generate::Loaded;
use esp_generate::config::find_option;
use esp_generate::manifest;
use esp_generate::plugin::selection;
use esp_generate::process;
use esp_generate::template::GeneratorOption;

/// Where the template was read from.
#[derive(Clone, Debug, PartialEq)]
pub enum TemplateOrigin {
    /// A directory inside the git repository rooted here, canonicalized.
    Repository(PathBuf),
    /// A clone of `url` at `commit`.
    Clone { url: String, commit: String },
    /// The bundled template, or a directory outside any git repository.
    Unknown,
}

/// The values the host supplies to every render.
pub struct HostValues {
    pub project_name: String,
    /// The `-o …` line that would reproduce this project.
    pub generate_parameters: String,
    /// `None` when the template is not a Cargo project, so no `Cargo.toml` was
    /// read.
    pub esp_hal_version_full: Option<String>,
    /// `None` when the template declares no toolchain-bearing target.
    pub rust_toolchain: Option<String>,
    pub origin: TemplateOrigin,
    /// Where the project is, or would be, written; need not exist. Read only
    /// for a [`TemplateOrigin::Repository`].
    pub project_dir: PathBuf,
}

const ORIGIN_VALUES: [&str; 4] = [
    "template_origin",
    "template_repo",
    "template_url",
    "template_commit",
];

/// Why `error` names an origin value that was not set, if it does.
fn origin_hint(facts: &process::Facts, error: &str) -> Option<String> {
    let value = ORIGIN_VALUES
        .iter()
        .find(|v| error.contains(**v) && !facts.values.contains_key(**v))?;
    Some(match facts.values.get("template_origin") {
        Some(process::FactValue::Str(origin)) => format!(
            "`{value}` is not set when `template_origin` is \"{origin}\"; branch on \
             `template_origin` before using it."
        ),
        _ => format!(
            "`{value}` is unavailable: the template directory is not inside a git checkout, \
             and was not cloned by `--template`."
        ),
    })
}

/// `to` as a `/`-separated path relative to `from`, both canonical.
fn relative_path(from: &Path, to: &Path) -> Result<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    if from.first() != to.first() {
        bail!(
            "the project at {} and the template's repository at {} are on different drives, \
             so the project cannot refer to the repository by a relative path. Generate it on \
             the same drive as the repository. `check` places its project in the current \
             directory, or with `--build` in the temporary directory, which `TMP` moves.",
            from.iter().collect::<PathBuf>().display(),
            to.iter().collect::<PathBuf>().display(),
        );
    }

    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - common];
    for component in &to[common..] {
        match component.as_os_str().to_str() {
            Some(part) if !part.contains(['"', '\\']) => parts.push(part),
            _ => bail!(
                "the template's repository at {} cannot be written into a project manifest: \
                 its path must be UTF-8 and contain neither `\"` nor `\\`.",
                to.iter().collect::<PathBuf>().display(),
            ),
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

/// `template_repo`: the repository root `repo`, relative to the project.
pub fn template_repo(repo: &Path, project_dir: &Path) -> Result<String> {
    let (Some(parent), Some(name)) = (project_dir.parent(), project_dir.file_name()) else {
        bail!("`{}` is not a project directory", project_dir.display());
    };
    let parent = std::fs::canonicalize(parent)
        .map_err(|e| anyhow::anyhow!("cannot resolve {}: {e}", parent.display()))?;
    relative_path(&parent.join(name), repo)
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
    match &host.origin {
        TemplateOrigin::Repository(repo) => {
            facts.set_value("template_origin", "repository");
            facts.set_value("template_repo", template_repo(repo, &host.project_dir)?);
        }
        TemplateOrigin::Clone { url, commit } => {
            facts.set_value("template_origin", "clone");
            facts.set_value("template_url", url.clone());
            facts.set_value("template_commit", commit.clone());
        }
        TemplateOrigin::Unknown => {}
    }
    if let Some(version) = &host.esp_hal_version_full {
        facts.set_value("esp_hal_version_full", version.clone());
    }
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

/// What `cargo fmt` is given on top of any `rustfmt.toml` it finds.
pub const RUSTFMT_CONFIG: [&str; 4] = [
    "--config",
    "group_imports=StdExternalCrate",
    "--config",
    "imports_granularity=Module",
];

/// Format a freshly written project the way generation does.
///
/// `check --build` runs `cargo fmt --check` over the result, so it has to see
/// the same formatting a generated project gets rather than the raw render.
pub fn format_project(steps: &manifest::Steps, project_dir: &std::path::Path) -> Result<()> {
    if steps.cargo_fmt {
        std::process::Command::new("cargo")
            .args(["fmt", "--"])
            .args(RUSTFMT_CONFIG)
            .current_dir(project_dir)
            .output()?;
    }

    let cargo_toml = project_dir.join("Cargo.toml");
    if steps.taplo && cargo_toml.exists() {
        let input = std::fs::read_to_string(&cargo_toml)?;
        let options = taplo::formatter::Options {
            align_entries: true,
            reorder_keys: true,
            reorder_arrays: true,
            ..Default::default()
        };
        std::fs::write(cargo_toml, taplo::formatter::format(&input, options))?;
    }

    Ok(())
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

        let processed =
            renderer.render(&contents, &mut load_partial).map_err(|e| {
                match origin_hint(facts, &e.to_string()) {
                    Some(hint) => anyhow::anyhow!("{source_path}:{e}\n{hint}"),
                    None => anyhow::anyhow!("{source_path}:{e}"),
                }
            })?;

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

    /// Unformatted on purpose: both steps would rewrite it.
    const RAGGED: &str = "[package]\nname=\"x\"\nversion=\"0.1.0\"\n";

    #[test]
    fn a_directory_and_a_clone_report_disjoint_origins() {
        let dir = tempfile::Builder::new()
            .prefix("esp-generate-origin-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("metadata.toml"),
            "sdk_version = \"0.1.0\"\n[project]\ntype = \"other\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("template.yaml"),
            "options:\n  - !Option\n    name: pinned\n    display_name: Pinned\n",
        )
        .unwrap();

        let loaded =
            esp_generate::Loaded::open(esp_generate::TemplateSource::Directory(dir.path().into()))
                .expect("fixture must load");
        let flat = esp_generate::config::flatten_options(&loaded.template.options);

        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("esp-hal")).unwrap();
        std::fs::create_dir_all(root.join("work/repro")).unwrap();

        let host = |origin, project_dir| HostValues {
            project_name: "p".to_string(),
            generate_parameters: String::new(),
            esp_hal_version_full: None,
            rust_toolchain: None,
            origin,
            project_dir,
        };
        let value = |facts: &process::Facts, k: &str| match facts.values.get(k) {
            Some(process::FactValue::Str(s)) => s.to_string(),
            other => panic!("`{k}` is {other:?}"),
        };

        let absent = |facts: &process::Facts, k: &str| {
            assert!(!facts.values.contains_key(k), "`{k}` is set");
        };

        let (local, _) = facts(
            &loaded,
            &[],
            &flat,
            &host(
                TemplateOrigin::Repository(root.join("esp-hal")),
                root.join("work/repro/p"),
            ),
        )
        .unwrap();
        assert_eq!(value(&local, "template_origin"), "repository");
        assert_eq!(value(&local, "template_repo"), "../../../esp-hal");
        absent(&local, "template_url");
        absent(&local, "template_commit");

        let (inside, _) = facts(
            &loaded,
            &[],
            &flat,
            &host(
                TemplateOrigin::Repository(root.join("esp-hal")),
                root.join("esp-hal/p"),
            ),
        )
        .unwrap();
        assert_eq!(value(&inside, "template_repo"), "..");

        let (cloned, _) = facts(
            &loaded,
            &[],
            &flat,
            &host(
                TemplateOrigin::Clone {
                    url: "https://github.com/esp-rs/esp-hal".to_string(),
                    commit: "c4e5ba70c".to_string(),
                },
                root.join("work/repro/p"),
            ),
        )
        .unwrap();
        assert_eq!(value(&cloned, "template_origin"), "clone");
        assert_eq!(
            value(&cloned, "template_url"),
            "https://github.com/esp-rs/esp-hal"
        );
        assert_eq!(value(&cloned, "template_commit"), "c4e5ba70c");
        absent(&cloned, "template_repo");
    }

    #[test]
    fn reading_an_unset_origin_value_fails_with_the_reason() {
        let dir = tempfile::Builder::new()
            .prefix("esp-generate-origin-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("metadata.toml"),
            "sdk_version = \"0.1.0\"\n[project]\ntype = \"other\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("template.yaml"), "options: []\n").unwrap();
        std::fs::write(dir.path().join("deps.txt"), "{{ template_repo }}\n").unwrap();

        let loaded =
            esp_generate::Loaded::open(esp_generate::TemplateSource::Directory(dir.path().into()))
                .expect("fixture must load");
        let host = |origin| HostValues {
            project_name: "p".to_string(),
            generate_parameters: String::new(),
            esp_hal_version_full: None,
            rust_toolchain: None,
            origin,
            project_dir: dir.path().join("p"),
        };

        let (unknown, _) = facts(&loaded, &[], &[], &host(TemplateOrigin::Unknown)).unwrap();
        for value in ORIGIN_VALUES {
            assert!(
                !unknown.values.contains_key(value),
                "`{value}` claims an origin"
            );
        }
        let err = plan(&loaded, &[], &[], &unknown)
            .err()
            .expect("reading `template_repo` must fail");
        assert!(
            err.to_string().contains("not inside a git checkout"),
            "{err}"
        );

        let clone = TemplateOrigin::Clone {
            url: "https://github.com/esp-rs/esp-hal".to_string(),
            commit: "c4e5ba70c".to_string(),
        };
        let (cloned, _) = facts(&loaded, &[], &[], &host(clone)).unwrap();
        let err = plan(&loaded, &[], &[], &cloned)
            .err()
            .expect("a clone has no `template_repo`");
        assert!(
            err.to_string().contains("branch on `template_origin`"),
            "{err}"
        );
    }

    #[test]
    fn a_repository_path_that_cannot_be_written_into_toml_is_refused() {
        let err = relative_path(Path::new("/w/p"), Path::new("/w/a\"b")).unwrap_err();
        assert!(err.to_string().contains("cannot be written"), "{err}");

        assert_eq!(
            relative_path(Path::new("/a\"b/p"), Path::new("/a\"b/esp-hal")).unwrap(),
            "../esp-hal",
            "only the part that reaches the manifest has to be writable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_or_non_utf8_repository_path_is_refused() {
        use std::os::unix::ffi::OsStrExt;

        assert!(relative_path(Path::new("/w/p"), Path::new("/w/a\\b")).is_err());
        let non_utf8 = Path::new("/w").join(std::ffi::OsStr::from_bytes(b"\xff"));
        assert!(relative_path(Path::new("/w/p"), &non_utf8).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_repository_on_another_drive_is_refused() {
        let err = relative_path(Path::new(r"C:\work\p"), Path::new(r"D:\esp-hal")).unwrap_err();
        assert!(err.to_string().contains("different drives"), "{err}");
    }

    #[test]
    fn a_template_that_opted_out_gets_neither_formatter() {
        let dir = tempfile::Builder::new()
            .prefix("esp-generate-format-test-")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), RAGGED).unwrap();

        let off = manifest::Steps {
            toolchain_check: false,
            cargo_fmt: false,
            taplo: false,
            git_init: true,
        };
        format_project(&off, dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(),
            RAGGED
        );

        format_project(&manifest::Steps { taplo: true, ..off }, dir.path()).unwrap();
        assert_ne!(
            std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(),
            RAGGED,
            "taplo was enabled and did nothing"
        );
    }

    /// A project with no `Cargo.toml` must not make the formatting step fail.
    #[test]
    fn formatting_a_project_without_a_cargo_manifest_is_not_an_error() {
        let dir = tempfile::Builder::new()
            .prefix("esp-generate-format-test-")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("main.c"), "int main(void){return 0;}\n").unwrap();

        format_project(
            &manifest::Steps {
                toolchain_check: true,
                cargo_fmt: true,
                taplo: true,
                git_init: true,
            },
            dir.path(),
        )
        .expect("a missing `Cargo.toml` must not be fatal");
    }

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
