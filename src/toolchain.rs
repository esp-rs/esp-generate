use std::process::Command;

use anyhow::{Result, bail};
use esp_generate::template::GeneratorOptionItem;
use esp_metadata::Chip;

use crate::check;

/// Return all installed rustup toolchains that support the given `target`
/// and meet the given MSRV.
fn filter_toolchains_for(target: &str, msrv: &check::Version) -> Result<Vec<String>> {
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
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // rustup prints things like: "stable-x86_64-unknown-linux-gnu (active, default)"
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };

        // check whether this toolchain's rustc knows the desired target
        // (rustup doesn't recognize some custom toolchains, e.g. `esp`)
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
                continue;
            }
        };

        if !output.status.success() {
            log::warn!(
                "`rustc +{name} --print target-list` exited with status {:?}",
                output.status.code()
            );
            continue;
        }

        if !String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|l| l.trim() == target)
        {
            // target not found - skip
            continue;
        }

        // call `rustc +<toolchain> --version` and compare to `msrv`
        if let Some(ver) = check::get_version("rustc", &[&format!("+{name}")]) {
            if !ver.is_at_least(msrv) {
                // toolchain version is below MSRV - skip
                continue;
            }
        } else {
            log::warn!(
                "Failed to detect rustc version for toolchain `{name}`; skipping MSRV check"
            );
        }

        available.push(name.to_string());
    }

    Ok(available)
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

/// Find the `toolchain` category in `template.yaml` and replace its placeholder
/// option with one option per installed rustup toolchain that supports the
/// required target and MSRV.
pub(crate) fn populate_toolchain_category(
    chip: Chip,
    options: &mut [GeneratorOptionItem],
    cli_toolchain: Option<&str>,
    msrv: &check::Version,
) -> Result<()> {
    let target = chip.target().to_string();

    let mut available = filter_toolchains_for(&target, msrv)?;

    // for now, we should hide the generic toolchains for Xtensa (stable-*, beta-*, nightly-*).
    if chip.is_xtensa() {
        available.retain(|name| {
            !(name.starts_with("stable") || name.starts_with("beta") || name.starts_with("nightly"))
        });
    }

    // sanity check
    if available.is_empty() {
        if let Some(cli) = cli_toolchain {
            if chip.is_xtensa()
                && (cli.starts_with("stable")
                    || cli.starts_with("beta")
                    || cli.starts_with("nightly"))
            {
                bail!(
                    "Toolchain `{cli}` is not supported for Xtensa targets; \
                     please use different toolchain (e.g. `esp`, see  https://docs.espressif.com/projects/rust/book/getting-started/toolchain.html#xtensa-devices)"
                );
            }

            bail!(
                "Toolchain `{cli}` does not have target `{target}` installed (or no toolchain does).\
                See https://docs.espressif.com/projects/rust/book/getting-started/toolchain.html"
            );
        }
        log::warn!(
            "No rustc toolchains found that have `{target}` installed; toolchain category will stay as placeholder"
        );
        return Ok(());
    }

    if let Some(cli) = cli_toolchain {
        if !available.iter().any(|t| t == cli) {
            if chip.is_xtensa()
                && (cli.starts_with("stable")
                    || cli.starts_with("beta")
                    || cli.starts_with("nightly"))
            {
                bail!(
                    "Toolchain `{cli}` is not supported for Xtensa targets; \
                     please use an ESP toolchain (e.g. `esp`)"
                );
            }

            bail!("Toolchain `{cli}` does not have target `{target}` installed");
        }
        // put CLI toolchain first in toolchain search in case it was provided.
        available.sort();
        available.sort_by_key(|t| if t == cli { 0 } else { 1 });
    }

    // get active/default toolchain to mark it properly
    let default = active_rustup_toolchain();

    // rewrite the `toolchain` category using the placeholder option as template
    for item in options.iter_mut() {
        let GeneratorOptionItem::Category(category) = item else {
            continue;
        };
        if category.name != "toolchain" {
            continue;
        }

        // we know exactly the template/placeholder structure, so we can just take `first` one
        let template_opt = match category.options.first() {
            Some(GeneratorOptionItem::Option(opt)) => opt.clone(),
            _ => {
                // If `template.yaml` is broken, fail loudly
                panic!("toolchain category must contain a placeholder !Option");
            }
        };

        // remove the placeholder, we've "scanned" it already
        category.options.clear();

        for toolchain in &available {
            // copy our placeholder option (again) to populate another toolchain instead of it
            let mut opt = template_opt.clone();

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

        break;
    }

    Ok(())
}
