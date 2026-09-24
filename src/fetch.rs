//! Acquiring a template that isn't already on disk.
//!
//! A `--template` value is either a directory to read in place or a repository
//! to clone. Cloning shells out to `git` rather than linking an HTTP client: it
//! reuses the user's existing credentials (so a private template repo works),
//! and it costs no new dependencies or archive-extraction surface.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use esp_generate::manifest::{MANIFEST_PATH, Manifest};
use esp_generate::source::is_skipped_dir;
use walkdir::WalkDir;

/// What a `--template` value names.
#[derive(Debug, PartialEq, Eq)]
pub enum TemplateRef {
    /// A directory to read in place.
    Local(PathBuf),
    /// A repository to clone, at an optional branch or tag.
    Repo {
        url: String,
        reference: Option<String>,
    },
}

/// Read a `--template` value.
///
/// An existing directory wins, so a local path is never mistaken for a repo.
/// Otherwise `owner/repo`, an `https://` URL or an `scp`-style `git@host:path`,
/// each with an optional `@<branch-or-tag>`.
pub fn parse_template_arg(value: &str) -> Result<TemplateRef> {
    let as_path = PathBuf::from(value);
    if as_path.is_dir() {
        return Ok(TemplateRef::Local(as_path));
    }
    if as_path.exists() {
        bail!("`--template {value}` exists but is not a directory");
    }

    // `git@host:owner/repo` also contains `@`, so only treat a trailing `@…` as
    // a ref when it looks like one: no `/` or `:` after it.
    let (base, reference) = match value.rsplit_once('@') {
        Some((base, rest)) if !rest.is_empty() && !rest.contains(['/', ':']) => {
            (base, Some(rest.to_string()))
        }
        _ => (value, None),
    };

    if base.is_empty() {
        bail!("`--template {value}` is neither an existing directory nor a repository");
    }

    let url = if base.contains("://") || base.starts_with("git@") {
        base.to_string()
    } else if base.split('/').count() == 2 && !base.starts_with('/') {
        format!("https://github.com/{base}")
    } else {
        bail!(
            "`--template {value}` is not an existing directory, and does not look like a \
             repository (expected `owner/repo[@ref]`, an `https://` URL, or `git@host:path`)"
        );
    };

    Ok(TemplateRef::Repo { url, reference })
}

/// A cloned repository, deleted when this is dropped.
pub struct Checkout {
    pub root: PathBuf,
    pub commit: String,
    _clone: TempDir,
}

/// Clone `url` shallowly and locate the template inside it.
pub fn clone(url: &str, reference: Option<&str>) -> Result<Checkout> {
    let clone = tempfile::Builder::new()
        .prefix("esp-generate-template-")
        .tempdir()
        .context("could not create a temporary directory for the clone")?;
    let path = clone.path();
    let mut command = Command::new("git");
    command.args(["clone", "--depth", "1", "--quiet"]);
    if let Some(reference) = reference {
        // `--branch` takes a branch or a tag. A commit SHA needs a fetch and
        // checkout instead, which shallow clones cannot do portably.
        command.args(["--branch", reference]);
    }
    command.arg(url).arg(path);

    let output = command
        .output()
        .context("could not run `git` — it must be installed to use a repository template")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        match reference {
            Some(reference) => bail!(
                "could not clone `{url}` at `{reference}`: {stderr}\n\
                 (a branch or tag is expected here; a commit SHA is not supported)"
            ),
            None => bail!("could not clone `{url}`: {stderr}"),
        }
    }

    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let root = find_template_root(path)?;

    Ok(Checkout {
        root,
        commit,
        _clone: clone,
    })
}

/// Find the one directory in `dir` holding a template manifest.
fn find_template_root(dir: &Path) -> Result<PathBuf> {
    let mut found = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_skipped_dir(e))
    {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if !entry.file_type().is_file() || entry.file_name() != MANIFEST_PATH {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if Manifest::parse(&raw).is_err() {
            continue;
        }

        let holder = entry.path().parent().unwrap_or(dir);
        let relative = holder.strip_prefix(dir).unwrap_or(Path::new(""));
        found.push(match relative.to_string_lossy().as_ref() {
            "" => ".".to_string(),
            other => other.replace('\\', "/"),
        });
    }

    match found.len() {
        0 => bail!(
            "no template found: nothing in the repository is a directory containing a \
             `{MANIFEST_PATH}` that parses as a template manifest"
        ),
        1 if found[0] == "." => Ok(dir.to_path_buf()),
        1 => Ok(dir.join(&found[0])),
        _ => {
            found.sort();
            bail!(
                "the repository holds more than one template: {}. \
                 Clone it and point `--template` at the one you want.",
                found.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn an_existing_directory_is_never_read_as_a_repo() {
        let dir = std::env::temp_dir();
        assert_eq!(
            parse_template_arg(dir.to_str().unwrap()).unwrap(),
            TemplateRef::Local(dir)
        );
    }

    #[test]
    fn a_shorthand_repo_expands_to_github() {
        assert_eq!(
            parse_template_arg("espressif/esp-hal").unwrap(),
            TemplateRef::Repo {
                url: "https://github.com/espressif/esp-hal".to_string(),
                reference: None,
            }
        );
        assert_eq!(
            parse_template_arg("espressif/esp-hal@v1.x").unwrap(),
            TemplateRef::Repo {
                url: "https://github.com/espressif/esp-hal".to_string(),
                reference: Some("v1.x".to_string()),
            }
        );
    }

    /// `git@host:owner/repo` contains an `@` that is not a ref separator.
    #[test]
    fn an_ssh_url_keeps_its_at_sign() {
        assert_eq!(
            parse_template_arg("git@github.com:espressif/esp-hal.git").unwrap(),
            TemplateRef::Repo {
                url: "git@github.com:espressif/esp-hal.git".to_string(),
                reference: None,
            }
        );
        assert_eq!(
            parse_template_arg("git@github.com:espressif/esp-hal.git@v1.x").unwrap(),
            TemplateRef::Repo {
                url: "git@github.com:espressif/esp-hal.git".to_string(),
                reference: Some("v1.x".to_string()),
            }
        );
    }

    #[test]
    fn an_https_url_is_used_as_is() {
        assert_eq!(
            parse_template_arg("https://example.com/a/b.git@main").unwrap(),
            TemplateRef::Repo {
                url: "https://example.com/a/b.git".to_string(),
                reference: Some("main".to_string()),
            }
        );
    }

    #[test]
    fn something_that_is_neither_is_refused() {
        let err = parse_template_arg("./not/a/real/path").unwrap_err();
        assert!(err.to_string().contains("does not look like a repository"));
    }

    fn scratch(files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "esp-generate-fetch-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (path, contents) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        root
    }

    const MANIFEST: &str = "sdk_version = \"0.1.0\"\n";

    #[test]
    fn the_template_may_live_in_a_subdirectory() {
        let root = scratch(&[
            ("README.md", "a repo that is not itself a template\n"),
            ("templates/board/metadata.toml", MANIFEST),
            ("templates/board/template.yaml", "options: []\n"),
        ]);
        assert_eq!(
            find_template_root(&root).unwrap(),
            root.join("templates/board")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `metadata.toml` is a common enough name that finding one proves nothing;
    /// it has to parse as a manifest.
    /// The common case: the repository is the template. It must not render as
    /// `<clone>/.` in the path a user is shown.
    #[test]
    fn a_repository_may_itself_be_the_template() {
        let root = scratch(&[
            ("metadata.toml", MANIFEST),
            ("template.yaml", "options: []\n"),
        ]);
        assert_eq!(find_template_root(&root).unwrap(), root);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A clone carries its own `.git`, which holds no template and is not UTF-8.
    #[test]
    fn the_walk_does_not_descend_into_git_or_target() {
        let root = scratch(&[
            ("tpl/metadata.toml", MANIFEST),
            (".git/modules/x/metadata.toml", MANIFEST),
            ("target/debug/metadata.toml", MANIFEST),
        ]);
        assert_eq!(find_template_root(&root).unwrap(), root.join("tpl"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unrelated_metadata_toml_is_not_a_template() {
        let root = scratch(&[
            ("docs/metadata.toml", "title = \"nothing to do with us\"\n"),
            ("tpl/metadata.toml", MANIFEST),
        ]);
        assert_eq!(find_template_root(&root).unwrap(), root.join("tpl"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_repository_with_no_template_says_so() {
        let root = scratch(&[("README.md", "nothing here\n")]);
        let err = find_template_root(&root).unwrap_err().to_string();
        assert!(err.contains("no template found"), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Picking one silently would make the choice depend on directory order.
    #[test]
    fn several_templates_are_reported_rather_than_guessed_between() {
        let root = scratch(&[("a/metadata.toml", MANIFEST), ("b/metadata.toml", MANIFEST)]);
        let err = find_template_root(&root).unwrap_err().to_string();
        assert!(err.contains("more than one template"), "{err}");
        assert!(err.contains('a') && err.contains('b'), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
