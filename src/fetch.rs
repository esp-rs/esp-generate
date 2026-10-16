//! Acquiring a template that isn't already on disk.
//!
//! A `--template` value is either a directory to read in place or a repository
//! to clone. Cloning shells out to `git` rather than linking an HTTP client: it
//! reuses the user's existing credentials (so a private template repo works),
//! and it costs no new dependencies or archive-extraction surface.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

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
    pub dir: PathBuf,
    /// In the form cargo accepts as a `git` dependency.
    pub url: String,
    pub commit: String,
    _clone: TempDir,
}

/// Clone `url` shallowly.
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

    let rev_parse = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .context("could not run `git rev-parse` in the clone")?;
    if !rev_parse.status.success() {
        bail!(
            "could not read the commit `{url}` was cloned at: {}",
            String::from_utf8_lossy(&rev_parse.stderr).trim()
        );
    }
    let commit = String::from_utf8_lossy(&rev_parse.stdout)
        .trim()
        .to_string();

    Ok(Checkout {
        dir: path.to_path_buf(),
        url: cargo_url(url),
        commit,
        _clone: clone,
    })
}

/// Cargo rejects `scp`-style `git@host:path`, so it becomes `ssh://git@host/path`.
fn cargo_url(url: &str) -> String {
    match url.split_once(':') {
        Some((host, path)) if !url.contains("://") => format!("ssh://{host}/{path}"),
        _ => url.to_string(),
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
    fn a_template_reports_a_url_cargo_accepts() {
        assert_eq!(
            cargo_url("git@github.com:espressif/esp-hal.git"),
            "ssh://git@github.com/espressif/esp-hal.git"
        );
        for url in [
            "https://github.com/esp-rs/esp-hal",
            "ssh://git@github.com/esp-rs/esp-hal",
            "file:///tmp/esp-hal",
        ] {
            assert_eq!(cargo_url(url), url);
        }
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
}
