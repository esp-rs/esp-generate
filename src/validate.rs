//! `esp-generate check` — render a template across its option sweep and report
//! what breaks, without writing anything.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

use esp_generate::config::flatten_options;
use esp_generate::contract;
use esp_generate::sweep::{self, SweepOptions};
use esp_generate::template::GeneratorOption;

use esp_generate::Loaded;

use crate::render;

/// What went wrong, and for which selection.
struct Failure {
    options: Vec<String>,
    message: String,
}

/// How much of the template to exercise.
pub struct Request {
    pub sweep: SweepOptions,
    /// Also generate each combination to a temporary directory and run cargo
    /// over it.
    pub build: bool,
    /// List the combinations and stop.
    pub dry_run: bool,
}

pub fn run(loaded: &Loaded, request: &Request) -> Result<()> {
    let combinations = sweep::enumerate(&loaded.template, &loaded.resolved, &request.sweep)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if request.dry_run {
        for combination in &combinations {
            println!("{}", combination.join(", "));
        }
        println!("{} combinations", combinations.len());
        return Ok(());
    }

    println!(
        "Checking {} combination{}{}",
        combinations.len(),
        if combinations.len() == 1 { "" } else { "s" },
        if request.build { ", with a build" } else { "" }
    );

    let flat_options = flatten_options(&loaded.template.options);
    let esp_hal_version = render::esp_hal_version_full(
        &crate::cargo::CargoToml::load(
            loaded
                .source
                .get("Cargo.toml")
                .ok_or_else(|| anyhow::anyhow!("template has no `Cargo.toml`"))?
                .as_ref(),
        )
        .map_err(|e| anyhow::anyhow!("template `Cargo.toml` is unreadable: {e}"))?
        .dependency_version("esp-hal"),
    );

    let mut failures = Vec::new();
    let mut predicates_used: Vec<&'static str> = Vec::new();
    for combination in &combinations {
        match check_one(
            loaded,
            combination,
            &flat_options,
            &esp_hal_version,
            request.build,
        ) {
            Ok(used) => {
                for name in used {
                    if !predicates_used.contains(&name) {
                        predicates_used.push(name);
                    }
                }
            }
            Err(e) => failures.push(Failure {
                options: combination.clone(),
                message: format!("{e:#}"),
            }),
        }
    }

    report_required_version(loaded, &predicates_used, failures.is_empty());

    if failures.is_empty() {
        println!("No problems found.");
        return Ok(());
    }

    let mut report = String::new();
    for failure in &failures {
        let _ = write!(
            report,
            "\n  -o {}\n     {}\n",
            failure.options.join(" -o "),
            failure.message.replace('\n', "\n     ")
        );
    }
    bail!(
        "{} of {} combinations failed:\n{report}",
        failures.len(),
        combinations.len()
    );
}

/// Report the contract version the template needs against the one it declares.
/// Counts only SDK predicates, and only when every combination rendered.
fn report_required_version(loaded: &Loaded, predicates_used: &[&str], complete: bool) {
    if !complete {
        return;
    }

    let declared = &loaded.manifest.sdk_version;
    let required = contract::minimum_version(predicates_used.iter().copied());

    if &required > declared {
        println!(
            "This template declares `sdk_version = \"{declared}\"` but uses features from \
             {required}. Raise it, or older generators will fail on it."
        );
    } else {
        println!("Requires sdk_version {required} (declared {declared}).");
    }
}

/// Render one combination, returning the SDK predicates it evaluated.
fn check_one(
    loaded: &Loaded,
    selected: &[String],
    flat_options: &[GeneratorOption],
    esp_hal_version: &str,
    build: bool,
) -> Result<Vec<&'static str>> {
    let (facts, _target) = render::facts(
        loaded,
        selected,
        flat_options,
        &render::HostValues {
            project_name: "check".to_string(),
            generate_parameters: selected
                .iter()
                .map(|o| format!("-o {o}"))
                .collect::<Vec<_>>()
                .join(" "),
            esp_hal_version_full: esp_hal_version.to_string(),
            rust_toolchain: None,
        },
    )?;

    let planned = render::plan(loaded, selected, flat_options, &facts)?;

    if !build {
        return Ok(planned.predicates_used);
    }

    let dir = tempfile::Builder::new()
        .prefix("esp-generate-check-")
        .tempdir()?;
    let project = dir.path().join("check");
    for (out_path, contents) in planned.files {
        let out_path = project.join(out_path);
        std::fs::create_dir_all(out_path.parent().unwrap())?;
        std::fs::write(out_path, contents)?;
    }

    cargo(&project, &["check"])?;
    if selected.iter().any(|o| o == "embedded-test") {
        cargo(&project, &["test", "--no-run"])?;
    }
    cargo(&project, &["clippy", "--no-deps", "--", "-Dwarnings"])?;
    cargo(&project, &["fmt", "--", "--check"])?;

    Ok(planned.predicates_used)
}

/// Run cargo in the generated project, surfacing its own diagnostics.
fn cargo(project: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("cargo")
        .args(args)
        // The generated project pins its own toolchain.
        .env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(project)
        .output()?;

    if !output.status.success() {
        bail!(
            "`cargo {}` failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use esp_generate::TemplateSource;

    /// Write a template to a fresh directory and load it.
    fn template(files: &[(&str, &str)]) -> (tempfile::TempDir, Loaded) {
        let dir = tempfile::Builder::new()
            .prefix("esp-generate-check-test-")
            .tempdir()
            .unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        let loaded = Loaded::open(TemplateSource::Directory(dir.path().to_path_buf()))
            .expect("fixture template must load");
        (dir, loaded)
    }

    const MANIFEST: &str = "sdk_version = \"0.1.0\"\n";
    const OPTIONS: &str = r#"
options:
  - !Option
    name: alpha
    display_name: Alpha
  - !Option
    name: beta
    display_name: Beta
"#;

    fn request() -> Request {
        Request {
            sweep: SweepOptions::default(),
            build: false,
            dry_run: false,
        }
    }

    #[test]
    fn a_template_that_renders_everywhere_passes() {
        let (_dir, loaded) = template(&[
            ("metadata.toml", MANIFEST),
            ("template.yaml", OPTIONS),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (
                "src/main.rs",
                "//%if option(\"alpha\")\n//+let a = 1;\n//%endif\n",
            ),
        ]);
        assert!(run(&loaded, &request()).is_ok());
    }

    /// The gap `check` exists to close: a name in a branch one generation
    /// never evaluates.
    #[test]
    fn a_typo_in_a_branch_no_single_generation_reaches_is_caught() {
        let (_dir, loaded) = template(&[
            ("metadata.toml", MANIFEST),
            ("template.yaml", OPTIONS),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (
                "src/main.rs",
                "//%if option(\"alpha\")\n//%if option(\"nonexistent\")\n//+let a = 1;\n//%endif\n//%endif\n",
            ),
        ]);

        let err = run(&loaded, &request()).unwrap_err().to_string();
        assert!(err.contains("nonexistent"), "{err}");
        assert!(err.contains("-o alpha"), "{err}");
    }

    #[test]
    fn an_output_path_escaping_the_project_is_caught() {
        let (_dir, loaded) = template(&[
            (
                "metadata.toml",
                format!(
                    "{MANIFEST}emit = [{{ path = \"src/main.rs\", as = \"../escaped.rs\" }}]\n"
                )
                .as_str(),
            ),
            ("template.yaml", OPTIONS),
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            ("src/main.rs", "fn main() {}\n"),
        ]);

        let err = run(&loaded, &request()).unwrap_err().to_string();
        assert!(err.contains("unsafe output path"), "{err}");
    }
}
