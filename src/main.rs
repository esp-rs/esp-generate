use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use esp_generate::Chip;
use esp_generate::template::{GeneratorOption, GeneratorOptionItem, SetValue, Template};
use esp_generate::{
    append_list_as_sentence,
    config::{ActiveConfiguration, Relationships},
};
use esp_generate::{
    cargo,
    config::{find_option, flatten_options},
};
use indexmap::IndexMap;
use inquire::Text;
use ratatui::crossterm::event;
use std::collections::HashSet;
use std::fmt::Write;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
    time::Duration,
};
use taplo::formatter::Options;

use esp_generate::template_files::TEMPLATE_FILES;

mod check;
mod chip_selector;
mod toolchain;
mod tui;

static TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    // Load `template.yaml` as the root and resolve every `!Include <path>`
    // against the bundled `TEMPLATE_FILES` table. This is the one place
    // that knows how include paths map onto real files; the rest of the
    // generator sees an already-flattened tree.
    let root_yaml = TEMPLATE_FILES
        .iter()
        .find_map(|(k, v)| (*k == "template.yaml").then_some(*v))
        .expect("bundled templates missing template.yaml");

    let template = Template::load(root_yaml, |path| {
        TEMPLATE_FILES
            .iter()
            .find_map(|(k, v)| (*k == path).then(|| v.to_string()))
    })
    .expect("failed to load bundled template");

    // The `chip` category is authored in YAML but must stay aligned with the
    // `Chip` enum for the generator to work.
    chip_selector::validate_chip_category(&template.options)
        .expect("invalid `chip` category in bundled template");
    template
        .validate_required()
        .expect("invalid `required` list in bundled template");

    template
});

#[derive(Parser, Debug)]
#[command(author, version, about = about(), long_about = None, subcommand_negates_reqs = true)]
struct Args {
    /// Name of the project to generate
    name: Option<String>,

    /// Run in headless mode (i.e. do not use the TUI)
    #[arg(long)]
    headless: bool,

    /// Generation options
    #[arg(short, long, help = {
        let mut all_options = Vec::new();
        for option in TEMPLATE.options.iter() {
            for opt in option.options() {
                // Remove duplicates, which usually are chip-specific variations of an option.
                // An example of this is probe-rs.
                if !all_options.contains(&opt) && opt != "PLACEHOLDER" {
                    all_options.push(opt);
                }
            }
        }
        format!("Generation options: {} - For more information regarding the different options check the esp-generate README.md (https://github.com/esp-rs/esp-generate/blob/main/README.md).",all_options.join(", "))
    })]
    option: Vec<String>,

    /// Directory in which to generate the project
    #[arg(short = 'O', long)]
    output_path: Option<PathBuf>,

    /// Do not check for updates
    #[arg(short, long, global = true, action)]
    #[cfg(feature = "update-informer")]
    skip_update_check: bool,

    /// Rust toolchain to use (rustup toolchain name; must support the selected chip target)
    ///
    /// Note that in headless mode this is not checked.
    #[arg(long)]
    toolchain: Option<String>,

    #[clap(subcommand)]
    subcommands: Option<SubCommands>,
}

#[derive(Subcommand, Debug)]
enum SubCommands {
    /// List available template options
    ListOptions,

    /// Print information about a template option
    Explain { option: String },
}

impl SubCommands {
    fn handle(&self) -> Result<()> {
        fn compatibility_info_text(options: &[&GeneratorOption], opt: &GeneratorOption) -> String {
            // Collect every `compatible` group key used by any variant sharing
            // this option's name (there can be more than one variant — see the
            // duplicate `probe-rs` entries in the template). Order is stable:
            // first appearance wins, so the rendered sentences line up with
            // the YAML.
            let variants: Vec<&GeneratorOption> = options
                .iter()
                .copied()
                .filter(|o| o.name == opt.name)
                .collect();

            let mut groups: Vec<&str> = Vec::new();
            for v in &variants {
                for key in v.compatible.keys() {
                    if !groups.contains(&key.as_str()) {
                        groups.push(key.as_str());
                    }
                }
            }

            let mut sentences: Vec<String> = Vec::new();
            for group in groups {
                // Union the allow-list across variants. If any variant doesn't
                // constrain this group, the name is effectively unconstrained
                // for that group — mirrors the "first matching variant wins"
                // semantics `find_option` uses at runtime — so we emit no
                // sentence for it.
                let mut allowed: Vec<String> = Vec::new();
                let mut unconstrained = false;
                for v in &variants {
                    match v.compatible.get(group) {
                        None => {
                            unconstrained = true;
                            break;
                        }
                        Some(names) => {
                            for n in names {
                                if !allowed.contains(n) {
                                    allowed.push(n.clone());
                                }
                            }
                        }
                    }
                }
                if unconstrained {
                    continue;
                }

                // Enumerate the full membership of the selection group from
                // the template itself, deduplicated by option name. This is
                // the generalisation of the old `Chip::iter().count()` — any
                // group whose options are authored in YAML (chip, module,
                // log-frontend, …) supplies its own denominator.
                let mut total: Vec<&str> = Vec::new();
                for o in options.iter().filter(|o| o.selection_group == group) {
                    if !total.contains(&o.name.as_str()) {
                        total.push(o.name.as_str());
                    }
                }

                // Nothing useful to say when the option is compatible with
                // every member of the group (or the group is empty — which
                // only happens for malformed templates, but we degrade
                // silently rather than emit a confusing sentence).
                if total.is_empty() || allowed.len() >= total.len() {
                    continue;
                }

                let sentence = if allowed.len() < total.len() / 2 {
                    format!("Compatible with {group}: {}.", allowed.join(", "))
                } else {
                    let excluded: Vec<&str> = total
                        .iter()
                        .copied()
                        .filter(|n| !allowed.iter().any(|a| a == n))
                        .collect();
                    format!("Not compatible with {group}: {}.", excluded.join(", "))
                };
                sentences.push(sentence);
            }

            sentences.join(" ")
        }

        let all_options = TEMPLATE.all_options();
        match self {
            SubCommands::ListOptions => {
                println!(
                    "The following template options are available. The group names are not part of the option name. Only one option in a group can be selected."
                );
                let mut groups = IndexMap::new();
                let mut seen = HashSet::new();
                for (index, option) in all_options
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| !["toolchain", "module"].contains(&o.selection_group.as_str()))
                {
                    let group = groups.entry(&option.selection_group).or_insert(Vec::new());

                    if seen.insert(&option.name) {
                        group.push(index);
                    }
                }
                for (group, options) in groups {
                    if TEMPLATE.required.contains(group) {
                        println!("Group: {} (required)", group);
                    } else {
                        println!("Group: {}", group);
                    }
                    for option in options {
                        let option = &all_options[option];
                        let mut help_text = option.display_name.clone();

                        if !option.requires.is_empty() {
                            help_text.push_str(" Requires: ");
                            let readable = option.requires.iter().map(|option| {
                                if let Some(stripped) = option.strip_prefix('!') {
                                    format!("{} unselected", stripped)
                                } else {
                                    option.to_string()
                                }
                            });
                            help_text.push_str(&readable.collect::<Vec<String>>().join(", "));
                            help_text.push('.');
                        }
                        let compat_info = compatibility_info_text(&all_options, option);
                        if !compat_info.is_empty() {
                            help_text.push(' ');
                            help_text.push_str(&compat_info);
                        }
                        println!("    {}: {help_text}", option.name);
                    }
                }
                Ok(())
            }
            SubCommands::Explain { option } => {
                if let Some(option) = all_options.iter().find(|o| &o.name == option) {
                    println!(
                        "Option: {}\n\n{}{}",
                        option.name,
                        option.display_name,
                        if option.help.is_empty() {
                            String::new()
                        } else {
                            format!("\n{}\n", option.help)
                        }
                    );
                    if !option.requires.is_empty() {
                        println!();
                        let positive_req = option.requires.iter().filter(|r| !r.starts_with("!"));
                        let negative_req = option.requires.iter().filter(|r| r.starts_with("!"));
                        if positive_req.clone().next().is_some() {
                            println!("Requires the following options to be set:");
                            for require in positive_req {
                                println!("    {}", require);
                            }
                        }
                        if negative_req.clone().next().is_some() {
                            println!("Requires the following options to NOT be set:");
                            for require in negative_req {
                                if let Some(stripped) = require.strip_prefix('!') {
                                    println!("    {}", stripped);
                                }
                            }
                        }
                    }
                    let compat_info = compatibility_info_text(&all_options, option);
                    if !compat_info.is_empty() {
                        println!("{}", compat_info);
                    }
                } else {
                    println!("Unknown option: {}", option);
                }
                Ok(())
            }
        }
    }
}

/// Check crates.io for a new version of the application
#[cfg(feature = "update-informer")]
fn check_for_update(name: &str, version: &str) {
    use update_informer::{Check, registry};
    // By setting the interval to 0 seconds we invalidate the cache with each
    // invocation and ensure we're getting up-to-date results
    let informer =
        update_informer::new(registry::Crates, name, version).interval(Duration::from_secs(0));

    if let Some(version) = informer.check_version().ok().flatten() {
        log::warn!("🚀 A new version of {name} is available: {version}");
    }
}

fn about() -> String {
    let mut about = String::from(
        "Template generation tool to create no_std applications targeting Espressif's chips.\n\nThe template will use these versions:\n",
    );

    let toml = cargo::CargoToml::load(
        TEMPLATE_FILES
            .iter()
            .find(|(k, _)| *k == "Cargo.toml")
            .expect("Cargo.toml not found in template")
            .1,
    )
    .expect("Failed to read Cargo.toml");

    toml.visit_dependencies(|_, name, table| {
        if name == "dependencies" {
            for entry in table.iter() {
                let name = entry.0;
                if name.starts_with("esp-") {
                    about.push_str(&format!("{:23 } {}\n", name, toml.dependency_version(name)));
                }
            }
        }
    });

    about
}

/// Scan the user's `-o`/`--option` list for an entry that names a [`Chip`]
/// variant. The chip travels with the rest of the generation options (it's a
/// normal entry in the `chip` selection group), so `-o esp32c6` both picks
/// the target and ticks the matching option.
///
/// Returns the first match. If the user passes multiple chip options (which
/// is meaningless since they share a selection group), the conflict is
/// surfaced later by [`process_options`] via the `same_selection_group` check.
///
/// Higher-level *presence* checks (i.e. "did the user pick a chip at all?")
/// go through [`Template::missing_required_groups`] instead — this helper
/// exists purely to produce the typed [`Chip`] value the rest of the
/// generator pipeline consumes.
fn chip_from_options(options: &[String]) -> Option<Chip> {
    options.iter().find_map(|opt| opt.parse::<Chip>().ok())
}

fn setup_args_interactive(args: &mut Args) -> Result<()> {
    if args.headless {
        let mut missing = String::from(
            "You are in headless mode, but esp-generate needs more information to generate your project.",
        );
        // Surface every required selection group that doesn't have a pick
        // in `-o`, not just the chip. Templates declare their required
        // groups in `template.yaml::required`; `chip` happens to be the
        // only one today, but the generator doesn't hard-code that.
        for group in TEMPLATE.missing_required_groups(&args.option) {
            missing.push_str(&format!(
                "\nNo option selected for the required `{group}` group. \
                 Add `-o <name>` for one of its options \
                 (see `esp-generate list-options`)."
            ));
        }
        if args.name.is_none() {
            missing.push_str("\nThe project name is missing. Add the name of your project to the end of the command.");
        }

        bail!("{missing}");
    }

    // Required groups are not prompted for up front: the TUI exposes each
    // of them as a first-class selection group, and blocks the Save action
    // until every required group has a pick. When no value is passed on
    // the command line we just seed the TUI with a reasonable default
    // tree; the user picks from the first menu level.

    if args.name.is_none() {
        let project_name = Text::new("Enter project name:")
            .with_default("my-esp-project")
            .prompt()?;

        args.name = Some(project_name);
    }

    Ok(())
}

fn main() -> Result<()> {
    tui::setup_logger().expect("logger should only be initialized once");

    let mut args = Args::parse();

    if let Some(subcommand) = args.subcommands {
        return subcommand.handle();
    }

    // Only check for updates once the command-line arguments have been processed,
    // to avoid printing any update notifications when the help message is
    // displayed.
    #[cfg(feature = "update-informer")]
    if !args.skip_update_check {
        check_for_update(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    }

    // Run the interactive TUI only if some required group is unpicked or
    // the name is missing. Required-group membership is driven by the
    // template's `required` list (see `Template::missing_required_groups`);
    // headless mode is rejected inside `setup_args_interactive` if either
    // piece is still missing.
    let missing_required = TEMPLATE.missing_required_groups(&args.option);
    if !missing_required.is_empty() || args.name.is_none() {
        setup_args_interactive(&mut args)?;
    }

    let name = args.name.clone().unwrap();

    let path = &args
        .output_path
        .clone()
        .unwrap_or_else(|| env::current_dir().unwrap());

    if !path.is_dir() {
        bail!("Output path must be a directory");
    }

    if path.join(&name).exists() {
        bail!("Directory already exists");
    }

    let versions = cargo::CargoToml::load(
        TEMPLATE_FILES
            .iter()
            .find(|(k, _)| *k == "Cargo.toml")
            .expect("Cargo.toml not found in template")
            .1,
    )
    .expect("Failed to read Cargo.toml");

    let esp_hal_version = versions.dependency_version("esp-hal");
    let esp_hal_version_full = if let Some(stripped) = esp_hal_version.strip_prefix("~") {
        let mut processed = stripped.to_string();
        while processed.chars().filter(|c| *c == '.').count() < 2 {
            processed.push_str(".0");
        }
        processed
    } else {
        esp_hal_version.clone()
    };

    let msrv: check::Version = versions.msrv().parse().unwrap();

    // Start toolchain scan as early as possible (TUI only). The scan itself is
    // chip-agnostic — chip/MSRV/CLI hint are applied later by
    // `toolchain::toolchains_for_chip` against the cached result, which makes
    // dynamic chip selection possible without re-scanning.
    let mut toolchain_scan = if args.headless {
        None
    } else {
        Some(toolchain::start_toolchain_scan())
    };

    // Stash the toolchain-category placeholder now, before anything mutates it.
    // `populate` is idempotent against this anchor, so repeated population
    // (e.g. after a future chip switch) always starts from a known baseline.
    let toolchain_category = toolchain::ToolchainCategory::capture(&TEMPLATE.options);

    // Build the initial options tree for the current chip. In headless mode
    // the toolchain scan never runs, so we seed the toolchain category with
    // `--toolchain` (if any) up front — otherwise post-build lookups for the
    // CLI toolchain name would fail.
    let headless_toolchain: &[String] = match (args.headless, args.toolchain.as_ref()) {
        (true, Some(tc)) => std::slice::from_ref(tc),
        _ => &[],
    };
    // Compat groups referenced anywhere in the pristine template — the set
    // of selections the TUI loop watches to trigger rebuilds.
    let compat_groups: Vec<String> = {
        let mut seen = HashSet::new();
        let mut keys = Vec::new();
        for opt in TEMPLATE.all_options() {
            for key in opt.compatible.keys() {
                if seen.insert(key) {
                    keys.push(key.clone());
                }
            }
        }
        keys
    };

    // `None` until the user picks one in the TUI; headless mode has
    // already rejected a missing required `chip` selection above.
    let mut chip: Option<Chip> = chip_from_options(&args.option);

    // Initial pruning
    let initial_selections: HashMap<String, String> = TEMPLATE
        .all_options()
        .iter()
        .filter(|o| args.option.iter().any(|n| n == &o.name) && !o.selection_group.is_empty())
        .map(|o| (o.selection_group.clone(), o.name.clone()))
        .collect();
    let initial_options = build_options(
        &initial_selections,
        toolchain_category.as_ref(),
        headless_toolchain,
    );

    process_options(
        &Template {
            options: initial_options.clone(),
            required: TEMPLATE.required.clone(),
        },
        &args,
    )?;

    let mut initial_selected = args.option.clone();
    if let Some(ref tc) = args.toolchain {
        initial_selected.push(tc.clone());
    }

    let repository = tui::Repository::new(initial_options, &initial_selected);

    let (mut selected, flat_options) = if !args.headless {
        let mut app = tui::App::new(repository, TEMPLATE.required.clone());

        let mut terminal = tui::init_terminal()?;

        let mut final_selected: Option<Vec<String>> = None;
        let mut running = true;

        let mut cached_toolchains: Vec<toolchain::ToolchainInfo> = Vec::new();
        let mut scan_finished = toolchain_scan.is_none();
        let mut populated_compat: Option<HashMap<String, String>> = None;
        let mut populated_with_scan = scan_finished;

        while running {
            if let Some(scan) = toolchain_scan.as_mut() {
                match scan.try_get_toolchain_list() {
                    None => {
                        app.set_toolchains_loading(true);
                    }
                    Some(Ok(list)) => {
                        if !scan_finished {
                            cached_toolchains = list.clone();
                            scan_finished = true;
                        }
                        app.set_toolchains_loading(false);
                    }
                    Some(Err(err)) => {
                        if !scan_finished {
                            log::warn!("Toolchain scan failed: {err}");
                            scan_finished = true;
                        }
                        app.set_toolchains_loading(false);
                    }
                }
            }

            // Rebuild-on-demand:
            //   * the `compatible` signature changed → some compat-relevant
            //     option (chip, log-frontend, …) was toggled; rebuild so the
            //     tree reflects the new constraints.
            //   * scan just finished or we haven't populated yet → rebuild to
            //     swap the toolchain placeholder for real entries.
            // Both paths flow through the same `build_options_for_chip` +
            // `App::set_options` pair, keeping compat-driven rebuilds and
            // toolchain refresh on one code path.
            let current_compat = app
                .repository
                .config
                .compatibility_signature(&compat_groups);
            let signature_changed = populated_compat.as_ref() != Some(&current_compat);
            let scan_needs_reflecting = scan_finished && !populated_with_scan;

            if signature_changed || scan_needs_reflecting {
                let filtered = toolchain::toolchains_for_chip(
                    &cached_toolchains,
                    current_compat
                        .get("chip")
                        .and_then(|name| name.parse().ok()),
                    &msrv,
                    args.toolchain.as_deref(),
                );
                for warning in &filtered.warnings {
                    log::warn!("{warning}");
                }

                let new_options = build_options(
                    &current_compat,
                    toolchain_category.as_ref(),
                    &filtered.names,
                );
                app.set_options(new_options);
                populated_compat = Some(
                    app.repository
                        .config
                        .compatibility_signature(&compat_groups),
                );
                populated_with_scan = scan_finished;
            }

            // draw a frame
            app.draw(&mut terminal)?;

            // handle input (non-blocking poll)
            if event::poll(Duration::from_millis(100))? {
                match app.handle_event(event::read()?)? {
                    tui::AppResult::Continue => {}
                    tui::AppResult::Quit => {
                        final_selected = None;
                        running = false;
                    }
                    tui::AppResult::Save => {
                        final_selected = Some(app.selected_options());
                        running = false;
                    }
                }
            }
        }

        tui::restore_terminal()?;
        // done with the TUI

        let Some(sel) = final_selected else {
            return Ok(());
        };

        chip = app.repository.selected_chip();

        (sel, app.repository.config.flat_options)
    } else {
        (initial_selected, repository.config.flat_options)
    };

    // FIXME: do not assume the template even has a "chip" property
    let chip = chip.expect("chip must be set by the time the TUI exits / headless validates");

    let mut toolchain_replaced = false;

    let selected_options = selected
        .iter()
        .fold(String::new(), |mut acc, s| {
            if Some(s) == args.toolchain.as_ref() && !toolchain_replaced {
                acc.push_str(" --toolchain ");
                // Just in case someone decides to call their toolchain `defmt`, make sure we only replace it once
                toolchain_replaced = true;
            } else {
                acc.push_str(" -o ");
            };
            acc.push_str(s);
            acc
        })
        .trim_start()
        .to_string();
    if !args.headless {
        println!("Selected options: {selected_options}");
    }

    // Same lookup for TUI and headless: both branches populated the toolchain
    // category in `flat_options` (TUI via scan results, headless via the
    // `--toolchain` CLI hint), so `find_option` resolves in either case.
    let selected_toolchain = selected.iter().find_map(|name| {
        let (_, opt) = find_option(name, &flat_options)?;
        if opt.selection_group == "toolchain" {
            Some(name.clone())
        } else {
            None
        }
    });

    let selected_module = selected.iter().find_map(|name| {
        let (_, opt) = find_option(name, &flat_options)?;
        if opt.selection_group == "module" {
            Some(name.clone())
        } else {
            None
        }
    });

    for idx in 0..selected.len() {
        let (_, option) = find_option(&selected[idx], &flat_options).unwrap();
        selected.push(option.selection_group.clone());
    }

    selected.push(if chip.metadata().is_xtensa() {
        "xtensa".to_string()
    } else {
        "riscv".to_string()
    });

    // mark that a toolchain was explicitly selected for template replacements
    if selected_toolchain.is_some() {
        selected.push("toolchain-selected".to_string());
    }

    let max_dram2 = chip.dram2_region().size();

    let mut variables = vec![
        ("project-name".to_string(), name.clone()),
        ("mcu".to_string(), chip.to_string()),
        (
            "generate-version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
        ("generate-parameters".to_string(), selected_options),
        ("esp-hal-version-full".to_string(), esp_hal_version_full),
        ("max-dram2-uninit".to_string(), format!("{max_dram2}")),
    ];

    // Merge scalar `sets` entries contributed by the selected options (e.g.
    // the chip-group option contributes `wokwi-board`). Generator-provided
    // variables above take precedence — `#REPLACE` lookup is first-match-wins
    // — so a template author can't accidentally shadow `project-name` /
    // `mcu` / etc. by declaring them in an option's `sets`.
    //
    // List-valued entries (e.g. `remove_pins`) aren't substitutable text and
    // are consumed directly by the code-generation paths that know what to
    // do with them (see the pin-reservation block below), so they're
    // deliberately skipped here instead of being joined into a string.
    for name in &selected {
        let Some((_, opt)) = find_option(name, &flat_options) else {
            continue;
        };
        for (key, value) in &opt.sets {
            if let Some(scalar) = value.as_scalar() {
                variables.push((key.clone(), scalar.to_string()));
            }
        }
    }

    variables.push((
        "rust_target".to_string(),
        chip.metadata().target().to_string(),
    ));

    if let Some(tc) = selected_toolchain.as_ref() {
        variables.push(("rust_toolchain".to_string(), tc.clone()));
    }

    let mut reserved_gpio_code = String::new();

    if let Some(ref module_name) = selected_module {
        if let Some((_, module_option)) = find_option(module_name, &flat_options) {
            // The module option carries its limitation tags as a list-valued
            // `sets` entry; a missing entry means "no pins to reserve", not
            // an error — e.g. a chip-specific module with nothing special
            // about its wiring.
            let remove_pins = module_option
                .sets
                .get("remove_pins")
                .and_then(SetValue::as_list)
                .unwrap_or(&[]);
            let restricted_pins = chip.pins().iter().filter(|pin| {
                remove_pins
                    .iter()
                    .any(|lim| pin.limitations.contains(&lim.as_str()))
            });
            let strapping_pins = chip
                .pins()
                .iter()
                .filter(|pin| pin.limitations.contains(&"strapping"))
                .collect::<Vec<_>>();

            if !strapping_pins.is_empty() {
                let strapping = strapping_pins
                    .iter()
                    .map(|pin| format!("// - GPIO{}", pin.pin))
                    .collect::<Vec<_>>()
                    .join("\n");
                writeln!(
                    &mut reserved_gpio_code,
                    r#"// The following pins are used to bootstrap the chip. They are available
                    // for use, but check the datasheet of the module for more information on them.
                    {strapping}"#
                )
                .unwrap();
            }

            // Only set module-selected if there are GPIOs to reserve
            if restricted_pins.clone().next().is_some() {
                selected.push("module-selected".to_string());

                let pin_plucker = restricted_pins
                    .map(|pin| format!("    let _ = peripherals.GPIO{};", pin.pin))
                    .collect::<Vec<_>>()
                    .join("\n");
                writeln!(
                    &mut reserved_gpio_code,
                    r#"// These GPIO pins are in use by some feature of the module and should not be used.
                    {pin_plucker}"#
                )
                .unwrap();
            };
        }
    }
    variables.push(("reserved_gpio_code".to_string(), reserved_gpio_code));

    let project_dir = path.join(&name);
    fs::create_dir(&project_dir)?;

    for &(file_path, contents) in TEMPLATE_FILES.iter() {
        let mut file_path = file_path.to_string();
        if let Some(processed) = process_file(contents, &selected, &variables, &mut file_path) {
            let file_path = project_dir.join(file_path);

            fs::create_dir_all(file_path.parent().unwrap())?;
            fs::write(file_path, processed)?;
        }
    }

    // Run cargo fmt:
    Command::new("cargo")
        .args([
            "fmt",
            "--",
            "--config",
            "group_imports=StdExternalCrate",
            "--config",
            "imports_granularity=Module",
        ])
        .current_dir(&project_dir)
        .output()?;

    // Format Cargo.toml:
    let input = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    let format_options = Options {
        align_entries: true,
        reorder_keys: true,
        reorder_arrays: true,
        ..Default::default()
    };
    let formated = taplo::formatter::format(&input, format_options);
    fs::write(project_dir.join("Cargo.toml"), formated)?;

    if should_initialize_git_repo(&project_dir) {
        // Run git init:
        Command::new("git")
            .arg("init")
            .current_dir(&project_dir)
            .output()?;
    } else {
        log::warn!("Current directory is already in a git repository, skipping git initialization");
    }

    check::check(
        &project_dir,
        chip.metadata(),
        selected.contains(&"probe-rs".to_string()),
        msrv,
        selected.contains(&"stack-smashing-protection".to_string())
            && selected.contains(&"riscv".to_string()),
        args.headless,
        selected_toolchain.as_deref(),
    );

    Ok(())
}

/// Prune options whose `compatible` constraints are actively violated by
/// `selections`. A group that is absent from `selections`, or present with an
/// empty value, is treated as unconstrained — the option is kept and the
/// runtime compatibility check handles it once the user makes a pick.
/// Categories that end up empty are dropped.
fn prune_incompatible_options(
    selections: &HashMap<String, String>,
    options: &mut Vec<GeneratorOptionItem>,
) {
    options.retain_mut(|opt| match opt {
        GeneratorOptionItem::Category(category) => {
            prune_incompatible_options(selections, &mut category.options);
            !category.options.is_empty()
        }
        GeneratorOptionItem::Option(option) => option.compatible.iter().all(|(group, allowed)| {
            match selections.get(group).filter(|s| !s.is_empty()) {
                Some(picked) => allowed.iter().any(|n| n == picked),
                None => true,
            }
        }),
    });
}

/// Build a fully-prepared options tree off the pristine [`TEMPLATE`].
///
/// Applies, in order:
///   1. compat pruning against `selections` (see
///      [`prune_incompatible_options`]),
///   2. toolchain-category population (`ToolchainCategory::populate`), if a
///      `ToolchainCategory` was captured off the original template.
///
/// The `chip` and `module` categories are both authored statically in
/// `template.yaml` and validated once at [`TEMPLATE`] load; no runtime
/// population is needed for either.
///
/// `selections` typically carries at least `{ "chip" => <chip name> }` — any
/// other `(group, pick)` entries enable additional build-time pruning (e.g.
/// dropping options incompatible with the current `log-frontend`). Groups
/// absent from `selections`, or present with an empty value, are treated as
/// unconstrained and left to the runtime compatibility check.
fn build_options(
    selections: &HashMap<String, String>,
    toolchain_category: Option<&toolchain::ToolchainCategory>,
    toolchains: &[String],
) -> Vec<GeneratorOptionItem> {
    let mut options = TEMPLATE.options.clone();
    prune_incompatible_options(selections, &mut options);
    if let Some(category) = toolchain_category {
        category.populate(&mut options, toolchains);
    }
    options
}

#[derive(Clone, Copy)]
enum BlockKind {
    // All lines are included
    Root,

    // (current branch to be included, any previous branches included)
    IfElse(bool, bool),
}

impl BlockKind {
    fn include_line(self) -> bool {
        match self {
            BlockKind::Root => true,
            BlockKind::IfElse(current, any) => current && !any,
        }
    }

    fn new_if(current: bool) -> BlockKind {
        BlockKind::IfElse(current, false)
    }

    fn into_else_if(self, condition: bool) -> BlockKind {
        let BlockKind::IfElse(previous, any) = self else {
            panic!("ELIF without IF");
        };
        BlockKind::IfElse(condition, any || previous)
    }

    fn into_else(self) -> BlockKind {
        let BlockKind::IfElse(previous, any) = self else {
            panic!("ELSE without IF");
        };
        BlockKind::IfElse(!any, any || previous)
    }
}

fn process_file(
    contents: &str,                 // Raw content of the file
    options: &[String],             // Selected options
    variables: &[(String, String)], // Variables and their values in tuples
    file_path: &mut String,         // File path to be modified
) -> Option<String> {
    let mut res = String::new();

    let mut replace: Option<Vec<(String, String)>> = None;
    let mut include = vec![BlockKind::Root];
    let mut file_directives = true;

    // Create a new Rhai engine and scope
    let mut engine = somni_expr::Context::new();

    // Define a custom function to check if conditions of the options.
    engine.add_function("option", move |cond: &str| -> bool {
        options.iter().any(|c| c == cond)
    });

    let mut include_file = true;

    for (line_no, line) in contents.lines().enumerate() {
        let line_no = line_no + 1;
        let trimmed: &str = line.trim();

        // We check for the first line to see if we should include the file
        if file_directives {
            // Determine if the line starts with a known include directive
            if let Some(cond) = trimmed
                .strip_prefix("//INCLUDEFILE ")
                .or_else(|| trimmed.strip_prefix("#INCLUDEFILE "))
                .or_else(|| trimmed.strip_prefix("--INCLUDEFILE "))
            {
                include_file = engine.evaluate::<bool>(cond).unwrap();
                continue;
            } else if let Some(include_as) = trimmed
                .strip_prefix("//INCLUDE_AS ")
                .or_else(|| trimmed.strip_prefix("#INCLUDE_AS "))
                .or_else(|| trimmed.strip_prefix("--INCLUDE_AS "))
            {
                let mut include_as = include_as.trim().to_string();
                for (key, value) in variables {
                    include_as = include_as.replace(&format!("{{{key}}}"), value);
                }
                *file_path = include_as;
                continue;
            }
        }
        if !include_file {
            return None;
        }

        file_directives = false;

        // that's a bad workaround
        if trimmed == "#[rustfmt::skip]" {
            log::info!("Skipping rustfmt");
            continue;
        }

        // Check if we should replace the next line with the key/value of a variable
        if let Some(what) = trimmed
            .strip_prefix("#REPLACE ")
            .or_else(|| trimmed.strip_prefix("//REPLACE "))
            .or_else(|| trimmed.strip_prefix("--REPLACE "))
        {
            let replacements = what
                .split(" && ")
                .filter_map(|pair| {
                    let mut parts = pair.split_whitespace();
                    if let (Some(pattern), Some(var_name)) = (parts.next(), parts.next()) {
                        if let Some((_, value)) = variables.iter().find(|(key, _)| key == var_name)
                        {
                            Some((pattern.to_string(), value.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            if !replacements.is_empty() {
                replace = Some(replacements);
            }
        // Check if we should include the next line(s)
        } else if trimmed.starts_with("#IF ")
            || trimmed.starts_with("//IF ")
            || trimmed.starts_with("--IF ")
        {
            let cond = trimmed
                .strip_prefix("#IF ")
                .or_else(|| trimmed.strip_prefix("//IF "))
                .or_else(|| trimmed.strip_prefix("--IF "))
                .unwrap();
            let last = *include.last().unwrap();

            // Only evaluate condition if this IF is in a branch that should be included
            let current = if last.include_line() {
                engine.evaluate::<bool>(cond).unwrap()
            } else {
                false
            };

            include.push(BlockKind::new_if(current));
        } else if trimmed.starts_with("#ELIF ")
            || trimmed.starts_with("//ELIF ")
            || trimmed.starts_with("--ELIF ")
        {
            let cond = trimmed
                .strip_prefix("#ELIF ")
                .or_else(|| trimmed.strip_prefix("//ELIF "))
                .or_else(|| trimmed.strip_prefix("--ELIF "))
                .unwrap();
            let last = include.pop().unwrap();

            // Only evaluate condition if no other branches evaluated to true
            let current = if matches!(last, BlockKind::IfElse(false, false)) {
                engine.evaluate::<bool>(cond).unwrap()
            } else {
                false
            };

            include.push(last.into_else_if(current));
        } else if trimmed.starts_with("#ELSE")
            || trimmed.starts_with("//ELSE")
            || trimmed.starts_with("--ELSE")
        {
            let last = include.pop().unwrap();
            include.push(last.into_else());
        } else if trimmed.starts_with("#ENDIF")
            || trimmed.starts_with("//ENDIF")
            || trimmed.starts_with("--ENDIF")
        {
            let prev = include.pop();
            assert!(
                matches!(prev, Some(BlockKind::IfElse(_, _))),
                "ENDIF without IF in {file_path}:{line_no}"
            );
        // Trim #+ and //+
        } else if include.iter().all(|v| v.include_line()) {
            let mut line = line.to_string();

            if trimmed.starts_with("#+") {
                line = line.replace("#+", "");
            }

            if trimmed.starts_with("//+") {
                line = line.replace("//+", "");
            }

            if trimmed.starts_with("--+") {
                line = line.replace("--+", "");
            }

            if let Some(replacements) = &replace {
                for (pattern, value) in replacements {
                    line = line.replace(pattern, value);
                }
            }

            res.push_str(&line);
            res.push('\n');

            replace = None;
        }
    }

    Some(res)
}

fn process_options(template: &Template, args: &Args) -> Result<()> {
    let mut success = true;
    // Two option catalogues, with complementary coverage:
    //   - `populated_options` is the pruned, post-`build_options` view: it
    //     knows about dynamically-populated entries (module options, etc.)
    //     but only those compatible with the current selections.
    //   - `pristine_options` is the raw template: it lists every option
    //     (including those pruned by `compatible`), so we can tell
    //     "pruned by selection" apart from "unknown name".
    let populated_options = template.all_options();
    let pristine_options = TEMPLATE.all_options();

    let flat_options = flatten_options(&template.options);
    let selected: Vec<usize> = args
        .option
        .iter()
        .flat_map(|opt_name| flat_options.iter().position(|o| &o.name == opt_name))
        .collect();

    let selected_config = ActiveConfiguration {
        selected,
        flat_options,
        options: template.options.clone(),
    };

    let mut same_selection_group: HashMap<&str, Vec<&str>> = HashMap::new();

    for option in &args.option {
        let option = option.as_str();
        let mut option_found_populated = false;
        let mut option_found_pristine = false;

        for &option_item in populated_options.iter().filter(|item| item.name == option) {
            option_found_populated = true;

            if selected_config.is_option_active(option_item) {
                // Even if the option is active, another from the same selection group may be present.
                // The TUI would deselect the previous option, but when specified from the command line,
                // we shouldn't assume which one the user actually wants. Therefore, we collect the selected
                // options that belong to a selection group and return an error (later) if multiple ones
                // are selected in the same group.
                if !option_item.selection_group.is_empty() {
                    let options = same_selection_group
                        .entry(&option_item.selection_group)
                        .or_default();

                    if !options.contains(&option) {
                        options.push(option);
                    }
                }
                continue;
            }

            success = false;
            let o = GeneratorOptionItem::Option(option_item.clone());
            let Relationships {
                requires,
                disabled_by,
                ..
            } = selected_config.collect_relationships(&o);

            if !requires
                .iter()
                .all(|requirement| args.option.iter().any(|r| r == requirement))
            {
                log::error!(
                    "Option '{}' requires {}",
                    option_item.name,
                    option_item.requires.join(", ")
                );
            }

            for disabled in disabled_by {
                log::error!("Option '{}' is disabled by {}", option_item.name, disabled);
            }
        }

        if !option_found_populated {
            option_found_pristine = pristine_options.iter().any(|item| item.name == option);
        }

        if !option_found_populated && !option_found_pristine {
            log::error!("Unknown option '{option}'");
            success = false;
        } else if !option_found_populated {
            let pristine = pristine_options
                .iter()
                .find(|item| item.name == option)
                .unwrap();
            let constraints = pristine
                .compatible
                .iter()
                .map(|(group, allowed)| format!("{group} in [{}]", allowed.join(", ")))
                .collect::<Vec<_>>()
                .join("; ");
            log::error!(
                "Option '{option}' is not compatible with the current selection \
                 (requires {constraints})"
            );
            success = false;
        }
    }

    for (_group, entries) in same_selection_group {
        if entries.len() > 1 {
            log::error!(
                "{}",
                append_list_as_sentence(
                    "The following options can not be enabled together:",
                    "",
                    &entries
                )
            );
            success = false;
        }
    }

    if !success {
        bail!("Invalid options provided");
    } else {
        Ok(())
    }
}

fn should_initialize_git_repo(mut path: &Path) -> bool {
    loop {
        let dotgit_path = path.join(".git");
        if dotgit_path.exists() && dotgit_path.is_dir() {
            return false;
        }

        if let Some(parent) = path.parent() {
            path = parent;
        } else {
            break;
        }
    }

    true
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_nested_if_else1() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &["opt1".to_string(), "opt2".to_string()],
            &[],
            &mut String::from("main.rs"),
        )
        .unwrap();

        assert_eq!(
            r#"
        opt1
        opt2
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else2() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &[],
            &[],
            &mut String::from("main.rs"),
        )
        .unwrap();

        assert_eq!(
            r#"
        !opt1
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else3() {
        let res = process_file(
            r#"
        #IF option("opt1")
        opt1
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        #ELSE
        !opt1
        #ENDIF
        "#,
            &["opt1".to_string()],
            &[],
            &mut String::from("main.rs"),
        )
        .unwrap();

        assert_eq!(
            r#"
        opt1
        !opt2
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else4() {
        let res = process_file(
            r#"
        #IF option("opt1")
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        opt1
        #ENDIF
        "#,
            &["opt1".to_string()],
            &[],
            &mut String::from("main.rs"),
        )
        .unwrap();

        assert_eq!(
            r#"
        !opt2
        opt1
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_nested_if_else5() {
        let res = process_file(
            r#"
        #IF option("opt1")
        #IF option("opt2")
        opt2
        #ELSE
        !opt2
        #ENDIF
        opt1
        #ENDIF
        "#,
            &["opt2".to_string()],
            &[],
            &mut String::from("main.rs"),
        )
        .unwrap();

        assert_eq!(
            r#"
        "#
            .trim(),
            res.trim()
        );
    }

    #[test]
    fn test_basic_elseif() {
        let template = r#"
        #IF option("opt1")
        opt1
        #ELIF option("opt2")
        opt2
        #ELIF option("opt3")
        opt3
        #ELSE
        opt4
        #ENDIF
        "#;

        const PAIRS: &[(&[&str], &str)] = &[
            (&["opt1"], "opt1"),
            (&["opt1", "opt2"], "opt1"),
            (&["opt1", "opt3"], "opt1"),
            (&["opt1", "opt2", "opt3"], "opt1"),
            (&["opt2"], "opt2"),
            (&["opt2", "opt3"], "opt2"),
            (&["opt3"], "opt3"),
            (&["opt4"], "opt4"),
            (&[], "opt4"),
        ];

        for (options, expected) in PAIRS.iter().cloned() {
            let res = process_file(
                template,
                &options.iter().map(|o| o.to_string()).collect::<Vec<_>>(),
                &[],
                &mut String::from("main.rs"),
            )
            .unwrap();
            assert_eq!(expected, res.trim(), "options: {:?}", options);
        }
    }
}
