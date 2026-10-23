//! Finding the templates in a directory, and picking one.
//!
//! Whatever `--template` names — a local directory or a fresh clone — may be a
//! template, or may hold several. The same rule decides it everywhere, the
//! searched directory included: a directory is a template when its manifest
//! parses, or when it does not but an option tree sits beside it. A template is
//! not searched further, so templates do not nest.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use esp_generate::manifest::{MANIFEST_PATH, Manifest, OPTION_TREE_PATH};
use esp_generate::source::{is_skipped_dir, relative_path};

/// A directory holding a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Relative to the directory searched, `/`-separated; `.` for the directory itself.
    pub path: String,
    pub name: String,
    pub description: String,
    /// Why its manifest does not load, for a template that is recognisably one
    /// but broken. Choosing it reports the full error.
    pub problem: Option<String>,
}

/// A `metadata.toml` that is not a template's: it does not parse as a manifest,
/// and no option tree sits beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The manifest file, relative to the directory searched, `/`-separated.
    pub path: String,
    pub reason: String,
}

impl std::fmt::Display for Skipped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}`: {}", self.path, self.reason)
    }
}

impl Candidate {
    /// What `dir` holds: `None` without a manifest, otherwise a template or a
    /// manifest that is not one.
    ///
    /// A manifest that parses makes a template. So does one that does not, as
    /// long as an option tree sits beside it: that is a broken template, listed
    /// with what is wrong rather than skipped — skipping it could leave a
    /// different template looking like the only choice.
    fn read(dir: &Path, path: String) -> Option<Result<Self, Skipped>> {
        let raw = std::fs::read_to_string(dir.join(MANIFEST_PATH)).ok()?;
        Some(match Manifest::parse(&raw) {
            Ok(manifest) => Ok(Candidate {
                path,
                name: manifest.name,
                description: manifest.description,
                problem: None,
            }),
            Err(e) if dir.join(OPTION_TREE_PATH).is_file() => Ok(Candidate {
                path,
                name: String::new(),
                description: String::new(),
                problem: Some(format!("{e}")),
            }),
            Err(e) => Err(Skipped {
                path: match path.as_str() {
                    "." => MANIFEST_PATH.to_string(),
                    _ => format!("{path}/{MANIFEST_PATH}"),
                },
                reason: format!("{e}"),
            }),
        })
    }

    /// Where this template is, given the directory it was found in.
    ///
    /// Joined a segment at a time, so the result uses the platform's separator:
    /// a `/` inside a Windows verbatim (`\\?\`) path is not one.
    pub fn dir_in(&self, searched: &Path) -> PathBuf {
        self.path
            .split('/')
            .filter(|segment| *segment != ".")
            .fold(searched.to_path_buf(), |dir, segment| dir.join(segment))
    }
}

impl std::fmt::Display for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path)?;
        if !self.name.is_empty() && self.name != self.path {
            write!(f, " ({})", self.name)?;
        }
        if let Some(problem) = &self.problem {
            write!(f, " — unusable: {problem}")?;
        } else if !self.description.is_empty() {
            write!(f, " — {}", self.description)?;
        }
        Ok(())
    }
}

/// What [`find_templates`] found.
#[derive(Debug)]
pub struct Found {
    /// Sorted by path.
    pub templates: Vec<Candidate>,
    /// Manifests that are not templates', for the caller to mention.
    pub skipped: Vec<Skipped>,
}

/// The templates in `dir`.
///
/// `dir` itself is checked first, by the same rule as any directory below it;
/// if it is a template, nothing below it is searched. A template that is
/// recognisable but broken is still found, so loading it reports exactly what
/// is wrong.
pub fn find_templates(dir: &Path) -> Result<Found> {
    let mut templates = Vec::new();
    let mut skipped = Vec::new();

    match Candidate::read(dir, ".".to_string()) {
        Some(Ok(template)) => {
            return Ok(Found {
                templates: vec![template],
                skipped,
            });
        }
        Some(Err(not_a_template)) => skipped.push(not_a_template),
        None => {}
    }

    let mut walk = WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_type().is_dir() && !is_skipped_dir(e));

    while let Some(entry) = walk.next() {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let Some(relative) = relative_path(dir, entry.path()) else {
            continue;
        };
        match Candidate::read(entry.path(), relative) {
            Some(Ok(template)) => {
                templates.push(template);
                walk.skip_current_dir();
            }
            Some(Err(not_a_template)) => skipped.push(not_a_template),
            None => {}
        }
    }

    if templates.is_empty() {
        let mut message = format!(
            "no template found in `{}`: no directory in it holds a `{MANIFEST_PATH}` that \
             parses as a template manifest, or one beside a `{OPTION_TREE_PATH}`",
            dir.display()
        );
        for not_a_template in &skipped {
            message.push_str(&format!("\n  {not_a_template}"));
        }
        bail!("{message}");
    }
    templates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Found { templates, skipped })
}

/// The outcome of picking among [`find_templates`]' results.
#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    One(Candidate),
    /// Nothing decides between these; the user has to.
    Ambiguous(Vec<Candidate>),
}

/// `path` as [`Candidate::path`] spells it: `/`-separated, without `.`
/// segments or a trailing separator.
fn normalise(path: &str) -> String {
    let normalised = path
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/");
    match normalised.as_str() {
        "" => ".".to_string(),
        _ => normalised,
    }
}

/// Pick the template `wanted` names, or the only one there is.
///
/// `wanted` is matched against paths first and manifest names only if no path
/// matches, so a path always picks its template even when another template's
/// name happens to be the same string.
pub fn select(mut candidates: Vec<Candidate>, wanted: Option<&str>) -> Result<Selection> {
    let Some(wanted) = wanted else {
        return Ok(match candidates.len() {
            1 => Selection::One(candidates.remove(0)),
            _ => Selection::Ambiguous(candidates),
        });
    };

    let path = normalise(wanted);
    if let Some(i) = candidates.iter().position(|c| c.path == path) {
        return Ok(Selection::One(candidates.swap_remove(i)));
    }

    let mut named: Vec<_> = candidates
        .iter()
        .filter(|c| !c.name.is_empty() && c.name == wanted)
        .collect();
    match named.len() {
        1 => Ok(Selection::One(named.remove(0).clone())),
        0 if matches!(candidates.as_slice(), [only] if only.path == ".") => bail!(
            "no template named `{wanted}`: the directory is itself a template, and templates \
             inside a template are not supported:\n{}",
            list(&candidates)
        ),
        0 => bail!(
            "no template named `{wanted}`. Available:\n{}",
            list(&candidates)
        ),
        _ => bail!(
            "more than one template is named `{wanted}`; pick one by path instead:\n{}",
            list(named.into_iter().cloned().collect::<Vec<_>>().as_slice())
        ),
    }
}

/// One candidate per line, for an error message.
pub fn list(candidates: &[Candidate]) -> String {
    candidates
        .iter()
        .map(|c| format!("  {c}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A manifest that parses, for tests.
#[cfg(test)]
pub(crate) const MANIFEST: &str = "sdk_version = \"0.1.0\"\n";

/// A scratch directory holding `files`, removed when dropped.
#[cfg(test)]
pub(crate) fn scratch(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = root.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    root
}

#[cfg(test)]
mod test {
    use super::*;

    /// The path of the template picked out of `dir`.
    fn picked(dir: &Path, wanted: Option<&str>) -> Result<String> {
        match select(find_templates(dir)?.templates, wanted)? {
            Selection::One(c) => Ok(c.path),
            Selection::Ambiguous(all) => bail!("ambiguous:\n{}", list(&all)),
        }
    }

    #[test]
    fn the_template_may_live_in_a_subdirectory() {
        let root = scratch(&[
            ("README.md", "a repo that is not itself a template\n"),
            ("templates/board/metadata.toml", MANIFEST),
            ("templates/board/template.yaml", "options: []\n"),
        ]);
        assert_eq!(picked(root.path(), None).unwrap(), "templates/board");
    }

    /// The common case: the repository is the template. It must not render as
    /// `<clone>/.` in the path a user is shown.
    #[test]
    fn a_repository_may_itself_be_the_template() {
        let root = scratch(&[
            ("metadata.toml", MANIFEST),
            ("template.yaml", "options: []\n"),
        ]);
        assert_eq!(picked(root.path(), None).unwrap(), ".");
    }

    /// A clone carries its own `.git`, which holds no template and is not UTF-8.
    #[test]
    fn the_walk_does_not_descend_into_git_or_target() {
        let root = scratch(&[
            ("tpl/metadata.toml", MANIFEST),
            (".git/modules/x/metadata.toml", MANIFEST),
            ("target/debug/metadata.toml", MANIFEST),
        ]);
        assert_eq!(picked(root.path(), None).unwrap(), "tpl");
    }

    /// `metadata.toml` is a common enough name that finding one proves nothing;
    /// it has to parse as a manifest.
    #[test]
    fn an_unrelated_metadata_toml_is_not_a_template() {
        let root = scratch(&[
            ("docs/metadata.toml", "title = \"nothing to do with us\"\n"),
            ("tpl/metadata.toml", MANIFEST),
        ]);
        assert_eq!(picked(root.path(), None).unwrap(), "tpl");
    }

    #[test]
    fn a_repository_with_no_template_says_so() {
        let root = scratch(&[("README.md", "nothing here\n")]);
        let err = find_templates(root.path()).unwrap_err().to_string();
        assert!(err.contains("no template found"), "{err}");
    }

    /// Picking one silently would make the choice depend on directory order.
    #[test]
    fn several_templates_are_left_for_the_user_to_choose_between() {
        let root = scratch(&[("b/metadata.toml", MANIFEST), ("a/metadata.toml", MANIFEST)]);
        let Selection::Ambiguous(all) =
            select(find_templates(root.path()).unwrap().templates, None).unwrap()
        else {
            panic!("two nested templates must not be guessed between");
        };
        let paths: Vec<_> = all.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["a", "b"]);
    }

    /// The root's manifest is not parsed during discovery, so loading it can say
    /// what is wrong — not "no template found".
    /// Broken but recognisable by its option tree, the root is still the
    /// template, so loading it can say what is wrong — not "no template found".
    #[test]
    fn a_template_at_the_root_is_used_without_searching_further() {
        let root = scratch(&[
            ("metadata.toml", "sdk_version = \"9.0.0\"\n"),
            ("template.yaml", "options: []\n"),
            ("examples/x/metadata.toml", MANIFEST),
        ]);
        assert_eq!(picked(root.path(), None).unwrap(), ".");
        assert_eq!(picked(root.path(), Some("./")).unwrap(), ".");

        let err = picked(root.path(), Some("examples/x"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not supported"), "{err}");
    }

    /// The root is held to the same rule as any directory below it: a manifest
    /// of someone else's there must not hide the templates.
    #[test]
    fn an_unrelated_metadata_toml_at_the_root_does_not_hide_templates() {
        let root = scratch(&[
            ("metadata.toml", "title = \"another tool's file\"\n"),
            ("nested/metadata.toml", MANIFEST),
        ]);
        let found = find_templates(root.path()).unwrap();
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].path, "metadata.toml");
        assert_eq!(picked(root.path(), None).unwrap(), "nested");
    }

    #[test]
    fn a_template_at_the_root_is_named_by_its_manifest_name() {
        let root = scratch(&[(
            "metadata.toml",
            "sdk_version = \"0.1.0\"\nname = \"solo\"\n",
        )]);
        assert_eq!(picked(root.path(), Some("solo")).unwrap(), ".");
    }

    /// Without an option tree beside it, a manifest that does not parse may be
    /// another tool's file, so it is not a template — but it is not dropped
    /// silently either: the caller gets it, with the reason, to mention.
    #[test]
    fn a_manifest_that_is_not_a_template_is_reported() {
        let root = scratch(&[
            ("a/metadata.toml", "sdk_version = \"9.0.0\"\n"),
            ("b/metadata.toml", MANIFEST),
        ]);
        let found = find_templates(root.path()).unwrap();
        assert_eq!(found.templates.len(), 1);
        let [skipped] = found.skipped.as_slice() else {
            panic!("expected one skipped manifest: {:?}", found.skipped);
        };
        assert_eq!(skipped.path, "a/metadata.toml");
        assert!(
            skipped.reason.contains("esp-template-sdk 9.0.0"),
            "{skipped}"
        );
    }

    #[test]
    fn finding_nothing_names_what_was_passed_over() {
        let root = scratch(&[("docs/metadata.toml", "title = \"nothing to do with us\"\n")]);
        let err = find_templates(root.path()).unwrap_err().to_string();
        assert!(err.contains("no template found"), "{err}");
        assert!(err.contains("docs/metadata.toml"), "{err}");
    }

    #[test]
    fn templates_do_not_nest() {
        let root = scratch(&[
            ("a/metadata.toml", MANIFEST),
            ("a/examples/x/metadata.toml", MANIFEST),
            ("b/metadata.toml", MANIFEST),
        ]);
        let paths: Vec<_> = find_templates(root.path())
            .unwrap()
            .templates
            .into_iter()
            .map(|c| c.path)
            .collect();
        assert_eq!(paths, ["a", "b"]);
    }

    /// Skipping it would leave `b` looking like the only template, and pick it
    /// for a user who meant `a`.
    #[test]
    fn a_broken_template_is_listed_rather_than_skipped() {
        let root = scratch(&[
            ("a/metadata.toml", "sdk_version = \"9.0.0\"\n"),
            ("a/template.yaml", "options: []\n"),
            ("b/metadata.toml", MANIFEST),
        ]);
        let Selection::Ambiguous(all) =
            select(find_templates(root.path()).unwrap().templates, None).unwrap()
        else {
            panic!("a broken template must still be a choice");
        };
        let problem = all[0].problem.as_deref().unwrap_or_default();
        assert!(problem.contains("esp-template-sdk 9.0.0"), "{problem}");
        assert!(all[0].to_string().contains("unusable"), "{}", all[0]);
        assert_eq!(all[1].problem, None);
    }

    #[test]
    fn a_template_is_named_by_path_or_by_manifest_name() {
        let root = scratch(&[
            (
                "a/metadata.toml",
                "sdk_version = \"0.1.0\"\nname = \"blinky\"\n",
            ),
            ("b/metadata.toml", MANIFEST),
        ]);
        assert_eq!(picked(root.path(), Some("blinky")).unwrap(), "a");
        assert_eq!(picked(root.path(), Some("b")).unwrap(), "b");

        let err = picked(root.path(), Some("c")).unwrap_err().to_string();
        assert!(err.contains("no template named `c`"), "{err}");
        assert!(
            err.contains("blinky"),
            "the error lists what is there: {err}"
        );
    }

    /// Otherwise a template named after another's path would make that path
    /// impossible to pick.
    #[test]
    fn a_path_takes_precedence_over_a_name() {
        let root = scratch(&[
            ("a/metadata.toml", "sdk_version = \"0.1.0\"\nname = \"b\"\n"),
            ("b/metadata.toml", MANIFEST),
        ]);
        assert_eq!(picked(root.path(), Some("b")).unwrap(), "b");
    }

    /// Shell completion writes `./templates/board/`; Windows users write `\`.
    #[test]
    fn a_wanted_path_is_normalised() {
        let root = scratch(&[("templates/board/metadata.toml", MANIFEST)]);
        for spelling in [
            "templates/board",
            "./templates/board/",
            "templates//board",
            "templates\\board",
        ] {
            assert_eq!(
                picked(root.path(), Some(spelling)).unwrap(),
                "templates/board",
                "{spelling}"
            );
        }
    }

    /// Compared with `Path::join`, so on Windows this checks the separators are
    /// native rather than the `/` that [`Candidate::path`] is spelled with.
    #[test]
    fn a_candidate_is_located_with_the_platform_separator() {
        let root = scratch(&[("templates/board/metadata.toml", MANIFEST)]);
        let [board] = find_templates(root.path())
            .unwrap()
            .templates
            .try_into()
            .unwrap();
        assert_eq!(
            board.dir_in(root.path()),
            root.path().join("templates").join("board")
        );

        let root = scratch(&[("metadata.toml", MANIFEST)]);
        let [itself] = find_templates(root.path())
            .unwrap()
            .templates
            .try_into()
            .unwrap();
        assert_eq!(itself.dir_in(root.path()), root.path());
    }
}
