use std::collections::BTreeSet;
use std::process::Command;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread;

use anyhow::Result;
use esp_generate::template::{GeneratorOption, GeneratorOptionItem};

use crate::{Chip, check};

/// Chip-agnostic metadata for a single installed rustup toolchain.
///
/// Scanning this information is expensive (one `rustc +tc --print target-list`
/// and one `rustc +tc --version` per toolchain), so we capture it once up front
/// and then derive per-chip filtered views via [`toolchains_for_chip`] without
/// spawning any more subprocesses.
#[derive(Debug, Clone)]
pub struct ToolchainInfo {
    pub name: String,
    pub targets: BTreeSet<String>,
    pub version: Option<check::Version>,
}

/// The filtered list of toolchains usable for a given chip, alongside any
/// non-fatal warnings the caller should surface (e.g. "`esp32` excluded because
/// the requested CLI toolchain is generic").
#[derive(Debug, Default)]
pub struct FilteredToolchains {
    pub names: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct ToolchainScan {
    rx: mpsc::Receiver<Result<Vec<ToolchainInfo>>>,
    cached: Option<Result<Vec<ToolchainInfo>>>,
}

impl ToolchainScan {
    /// Try to get the scanned toolchain list *without blocking*.
    pub fn try_get_toolchain_list(&mut self) -> Option<&Result<Vec<ToolchainInfo>>> {
        if self.cached.is_some() {
            return self.cached.as_ref();
        }

        match self.rx.try_recv() {
            Ok(res) => {
                self.cached = Some(res);
                self.cached.as_ref()
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                log::warn!(
                    "Toolchain scan thread failed or channel disconnected; treating as no toolchains"
                );
                self.cached = Some(Ok(Vec::new()));
                self.cached.as_ref()
            }
        }
    }
}
/// Start discovering all installed rustup toolchains in a background thread.
///
/// No chip or MSRV is involved here — the scan is chip-agnostic on purpose so
/// that switching chips in the TUI does not require re-scanning. Per-chip
/// filtering is done in-memory by [`toolchains_for_chip`].
pub fn start_toolchain_scan() -> ToolchainScan {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = scan_installed_toolchains();
        let _ = tx.send(result);
    });

    ToolchainScan { rx, cached: None }
}

/// Enumerate every installed rustup toolchain and capture its target list and
/// version. Never fails hard — subprocess errors degrade to "no toolchains".
fn scan_installed_toolchains() -> Result<Vec<ToolchainInfo>> {
    let output = match Command::new("rustup").args(["toolchain", "list"]).output() {
        Ok(res) => res,
        Err(err) => {
            // unlikely to happen, how did user even get to this point if ended up here?
            log::warn!("Failed to run `rustup toolchain list`: {err}");
            return Ok(Vec::new());
        }
    };

    if !output.status.success() {
        log::warn!(
            "`rustup toolchain list` exited with status {:?}",
            output.status.code()
        );
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut available = Vec::new();

    for line in stdout.lines() {
        // rustup prints things like: "stable-x86_64-unknown-linux-gnu (active, default)"
        if let Some(name) = line.split_whitespace().next()
            && let Some(info) = inspect_toolchain(name)
        {
            available.push(info);
        }
    }

    Ok(available)
}

/// Collect target list + version for a single toolchain. Returns `None` if the
/// toolchain can't be introspected at all (e.g. not a real rustup toolchain —
/// we skip rather than poison the whole scan).
fn inspect_toolchain(name: &str) -> Option<ToolchainInfo> {
    let output = match Command::new("rustc")
        .args([
            format!("+{name}"),
            "--print".to_string(),
            "target-list".to_string(),
        ])
        .output()
    {
        Ok(res) => res,
        Err(err) => {
            log::warn!("Failed to run `rustc +{name} --print target-list`: {err}");
            return None;
        }
    };

    if !output.status.success() {
        log::warn!(
            "`rustc +{name} --print target-list` exited with status {:?}",
            output.status.code()
        );
        return None;
    }

    let targets: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let version = check::get_version("rustc", &[&format!("+{name}")]);
    if version.is_none() {
        log::warn!("Failed to detect rustc version for toolchain `{name}`; skipping MSRV check");
    }

    Some(ToolchainInfo {
        name: name.to_string(),
        targets,
        version,
    })
}

/// Return the currently active rustup toolchain name, if any
fn active_rustup_toolchain() -> Option<String> {
    let output = Command::new("rustup")
        .args(["show", "active-toolchain"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next().map(|name| name.to_string()))
}

/// Pure, cheap filter over a [`ToolchainInfo`] slice for the given chip.
///
/// Recomputed on demand (e.g. whenever the user switches chip in the TUI).
/// All per-chip concerns live here — target-triple match, MSRV gate, Xtensa
/// exclusion of generic toolchains, and CLI-toolchain sort/validation — so
/// that the cached scan data stays untouched across chip switches.
///
/// When `chip` is `None` every scanned toolchain is returned unfiltered;
/// the per-chip gates run again on the next rebuild once a chip is picked.
///
/// Previously-fatal CLI-toolchain mismatches are now returned as warnings:
/// the caller decides whether to promote them (headless) or surface them in
/// the TUI footer. Dynamic chip switching requires this: a chip switch that
/// invalidates the CLI toolchain must not be unrecoverable.
pub fn toolchains_for_chip(
    all: &[ToolchainInfo],
    chip: Option<Chip>,
    msrv: &check::Version,
    cli_hint: Option<&str>,
) -> FilteredToolchains {
    let Some(chip) = chip else {
        let names: Vec<String> = all.iter().map(|tc| tc.name.clone()).collect();
        return FilteredToolchains {
            names,
            warnings: Vec::new(),
        };
    };

    let target = chip.metadata().target().to_string();

    let mut names: Vec<String> = all
        .iter()
        .filter(|tc| tc.targets.contains(target.as_str()))
        .filter(|tc| tc.version.as_ref().is_none_or(|v| v.is_at_least(msrv)))
        .map(|tc| tc.name.clone())
        .collect();

    // for now, we should hide the generic toolchains for Xtensa (stable-*, beta-*, nightly-*).
    if chip.metadata().is_xtensa() {
        names.retain(|name| {
            !(name.starts_with("stable") || name.starts_with("beta") || name.starts_with("nightly"))
        });
    }

    let mut warnings = Vec::new();

    if names.is_empty() {
        if let Some(cli) = cli_hint {
            if chip.metadata().is_xtensa() && is_generic_toolchain(cli) {
                warnings.push(format!(
                    "Toolchain `{cli}` is not supported for Xtensa targets; \
                     please use different toolchain (e.g. `esp`, see \
                     https://docs.espressif.com/projects/rust/book/getting-started/toolchain.html#xtensa-devices)"
                ));
            } else {
                warnings.push(format!(
                    "Toolchain `{cli}` does not have target `{target}` installed (or no toolchain does). \
                     See https://docs.espressif.com/projects/rust/book/getting-started/toolchain.html"
                ));
            }
        } else {
            warnings.push(format!(
                "No rustc toolchains found that have `{target}` installed; \
                 toolchain category will stay as placeholder"
            ));
        }
        return FilteredToolchains { names, warnings };
    }

    if let Some(cli) = cli_hint {
        if !names.iter().any(|t| t == cli) {
            if chip.metadata().is_xtensa() && is_generic_toolchain(cli) {
                warnings.push(format!(
                    "Toolchain `{cli}` is not supported for Xtensa targets; \
                     please use an ESP toolchain (e.g. `esp`)"
                ));
            } else {
                warnings.push(format!(
                    "Toolchain `{cli}` does not have target `{target}` installed"
                ));
            }
        } else {
            // CLI toolchain is valid: float it to the top for the selector.
            names.sort();
            names.sort_by_key(|t| if t == cli { 0 } else { 1 });
        }
    }

    FilteredToolchains { names, warnings }
}

fn is_generic_toolchain(name: &str) -> bool {
    name.starts_with("stable") || name.starts_with("beta") || name.starts_with("nightly")
}

/// Stash for the original "Scanning installed toolchains…" placeholder.
///
/// Captured once before any population and reused on every [`Self::populate`]
/// call. This makes the populate step idempotent and reversible — crucial for
/// dynamic chip switching, where a chip change may grow, shrink, or empty the
/// list of valid toolchains. Mutating the placeholder in place (as the old API
/// did) loses that anchor forever.
#[derive(Debug, Clone)]
pub struct ToolchainCategory {
    placeholder: GeneratorOption,
}

impl ToolchainCategory {
    /// Capture the placeholder option sitting under the `toolchain` category.
    ///
    /// Returns `None` if the template has no `toolchain` category at all; the
    /// caller can choose to skip toolchain handling entirely in that case.
    pub fn capture(options: &[GeneratorOptionItem]) -> Option<Self> {
        for item in options {
            let GeneratorOptionItem::Category(category) = item else {
                continue;
            };
            if category.name != "toolchain" {
                continue;
            }
            let GeneratorOptionItem::Option(first) = category.options.first()? else {
                return None;
            };
            return Some(Self {
                placeholder: first.clone(),
            });
        }
        None
    }

    /// Rebuild the `toolchain` category wholesale from `available`.
    ///
    /// - Empty `available` leaves / restores the placeholder row, so the
    ///   "Scanning installed toolchains…" text reappears if a chip switch
    ///   wipes the list.
    /// - Non-empty `available` replaces the category with one option per
    ///   discovered toolchain, each derived from the stashed placeholder
    ///   (so `selection_group` / help text stay consistent with the YAML).
    ///
    /// The caller is responsible for rebuilding dependent state after this:
    /// in particular [`crate::config::ActiveConfiguration::rebuild_indices`]
    /// must be invoked on any live `ActiveConfiguration` whose `options` were
    /// mutated, or `selected` indices will dangle.
    pub fn populate(&self, options: &mut [GeneratorOptionItem], available: &[String]) {
        let default = active_rustup_toolchain();

        for item in options.iter_mut() {
            let GeneratorOptionItem::Category(category) = item else {
                continue;
            };
            if category.name != "toolchain" {
                continue;
            }

            category.options.clear();

            if available.is_empty() {
                category
                    .options
                    .push(GeneratorOptionItem::Option(self.placeholder.clone()));
                return;
            }

            for toolchain in available {
                let mut opt = self.placeholder.clone();
                let is_default = default.as_deref() == Some(toolchain.as_str());
                opt.name = toolchain.clone();
                opt.display_name = if is_default {
                    format!("Use `{toolchain}` toolchain [default]")
                } else {
                    format!("Use `{toolchain}` toolchain")
                };
                opt.selection_group = "toolchain".to_string();
                category.options.push(GeneratorOptionItem::Option(opt));
            }
            return;
        }
    }
}
