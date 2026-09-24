//! Where a template's files come from.
//!
//! Every consumer goes through [`TemplateSource`] rather than touching
//! [`TEMPLATE_FILES`], so adding a source that isn't compiled in — a directory,
//! an unpacked archive — is a change to this module alone.
//!
//! Paths are template-root-relative throughout: the key space `build.rs` emits,
//! that `!Include` and `include` resolve against, and that a generated file
//! lands on before `emit.as` gets a say.

use std::borrow::Cow;
use std::path::PathBuf;

use walkdir::{DirEntry, WalkDir};

use crate::process::is_safe_relative_path;
use crate::template_files::TEMPLATE_FILES;

/// Never walked into: a template directory is often a git checkout, and
/// `target/` is whatever the author last built.
pub const SKIPPED_DIRS: &[&str] = &[".git", "target"];

/// Whether a walk should refuse to descend into `entry`. The walk root itself
/// is never skipped, whatever it happens to be called.
pub fn is_skipped_dir(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && SKIPPED_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
}

/// A template's file set.
///
/// Contents are [`Cow`] so a compiled-in template stays zero-copy while a
/// directory owns what it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSource {
    /// The template compiled into this binary by `build.rs`.
    Bundled,
    /// A directory on disk, read as generation goes.
    Directory(PathBuf),
}

impl TemplateSource {
    pub fn read(&self, path: &str) -> Result<Cow<'static, str>, String> {
        match self {
            TemplateSource::Bundled => TEMPLATE_FILES
                .iter()
                .find_map(|(k, v)| (*k == path).then_some(Cow::Borrowed(*v)))
                .ok_or_else(|| format!("template has no file `{path}`")),

            TemplateSource::Directory(root) => {
                if !is_safe_relative_path(path) {
                    return Err(format!("`{path}` escapes the template directory"));
                }
                let full = root.join(path);
                if !full.is_file() {
                    return Err(format!("template has no file `{path}`"));
                }
                std::fs::read_to_string(&full)
                    .map(Cow::Owned)
                    .map_err(|e| format!("cannot read `{path}`: {e}"))
            }
        }
    }

    /// The contents of `path`, or `None` if this template has no such file.
    pub fn get(&self, path: &str) -> Option<Cow<'static, str>> {
        self.read(path).ok()
    }

    /// Every `(path, contents)` pair — the order generation writes them in.
    ///
    /// Sorted by path, so a directory template generates in a stable order
    /// rather than whatever the filesystem hands back.
    pub fn files(&self) -> Result<Vec<(String, Cow<'static, str>)>, String> {
        match self {
            TemplateSource::Bundled => Ok(TEMPLATE_FILES
                .iter()
                .map(|(k, v)| (k.to_string(), Cow::Borrowed(*v)))
                .collect()),

            TemplateSource::Directory(root) => {
                let mut out = Vec::new();
                for entry in WalkDir::new(root)
                    .into_iter()
                    .filter_entry(|e| !is_skipped_dir(e))
                {
                    let entry =
                        entry.map_err(|e| format!("cannot read `{}`: {e}", root.display()))?;
                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let relative = entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| {
                            format!("`{}` is outside the template", entry.path().display())
                        })?
                        .to_string_lossy()
                        .replace('\\', "/");

                    let contents = std::fs::read_to_string(entry.path()).map_err(|e| {
                        format!("cannot read `{relative}`: {e} (templates must be UTF-8 text)")
                    })?;

                    out.push((relative, Cow::Owned(contents)));
                }
                out.sort_by(|a, b| a.0.cmp(&b.0));
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Build a throwaway directory template. Returns its root.
    fn dir_with(files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "esp-generate-src-test-{}-{:?}",
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

    #[test]
    fn a_directory_reads_root_relative_paths() {
        let root = dir_with(&[
            ("metadata.toml", "sdk_version = \"0.1.0\"\n"),
            ("src/bin/main.rs", "fn main() {}\n"),
            (".vscode/settings.json", "{}\n"),
        ]);
        let source = TemplateSource::Directory(root.clone());

        assert_eq!(
            source.get("src/bin/main.rs").as_deref(),
            Some("fn main() {}\n")
        );
        // Dotfiles are ordinary files, as in the bundled key space.
        assert_eq!(source.get(".vscode/settings.json").as_deref(), Some("{}\n"));
        assert_eq!(source.get("no/such/file"), None);

        let paths: Vec<String> = source
            .files()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(
            paths,
            [".vscode/settings.json", "metadata.toml", "src/bin/main.rs"],
            "files() is sorted, so generation order does not follow the filesystem"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A template directory is usually a checkout, so `.git` would otherwise be
    /// read as template files — and its contents are not even UTF-8.
    #[test]
    fn a_directory_skips_git_and_target() {
        let root = dir_with(&[
            ("metadata.toml", "sdk_version = \"0.1.0\"\n"),
            (".git/config", "[core]\n"),
            ("target/debug/whatever", "junk\n"),
        ]);
        let source = TemplateSource::Directory(root.clone());

        let paths: Vec<String> = source
            .files()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(paths, ["metadata.toml"]);

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An `include` that walks out of the template is refused before it reaches
    /// the filesystem, so a directory source cannot be used to read the host.
    #[test]
    fn a_directory_refuses_a_path_that_escapes_it() {
        let root = dir_with(&[("metadata.toml", "sdk_version = \"0.1.0\"\n")]);
        let source = TemplateSource::Directory(root.clone());

        for escape in ["../secret", "/etc/passwd", "a/../../b"] {
            let err = source
                .read(escape)
                .expect_err("an escaping path must not be read");
            assert!(err.contains("escapes"), "{escape}: {err}");
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_bundled_source_resolves_root_relative_paths() {
        let source = TemplateSource::Bundled;

        assert!(source.get("template.yaml").is_some());
        assert!(source.get("src/bin/main.rs").is_some());
        // Nested and dotfile paths use the same key space.
        assert!(source.get(".template/module.yaml").is_some());
        assert!(source.get(".nvim.lua").is_some());

        // `plugin:chip` comes from a plugin, so it is deliberately not a file.
        assert_eq!(source.get("plugin:chip"), None);

        assert_eq!(source.get("no/such/file"), None);
    }

    #[test]
    fn read_names_the_missing_path() {
        let err = TemplateSource::Bundled
            .read("no/such/file")
            .expect_err("a missing file must be an error, not empty content");
        assert!(err.contains("no/such/file"), "{err}");
    }

    #[test]
    fn files_agrees_with_get() {
        let source = TemplateSource::Bundled;
        let files = source.files().unwrap();

        assert!(!files.is_empty(), "the bundled template is not empty");
        for (path, contents) in files {
            assert_eq!(
                source.get(&path),
                Some(contents),
                "`{path}` from `files()` must resolve through `get()`"
            );
        }
    }
}
