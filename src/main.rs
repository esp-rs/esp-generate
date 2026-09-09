use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use esp_generate::plugin;
use esp_generate::sweep;
use esp_generate::template::{GeneratorOption, GeneratorOptionItem, Template};
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
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
    time::Duration,
};
use taplo::formatter::Options;

use esp_generate::{Loaded, TemplateSource};

mod check;
mod fetch;
mod render;
mod toolchain;
mod tui;
mod validate;

/// Whether any selected option declares `requires_nightly`.
///
/// Xtensa is exempt: its toolchain is `esp`, which already carries what nightly
/// would provide and has no nightly channel to filter down to.
fn requires_nightly(
    selected: &[String],
    flat_options: &[GeneratorOption],
    is_xtensa: bool,
) -> bool {
    if is_xtensa {
        return false;
    }

    selected
        .iter()
        .any(|name| find_option(name, flat_options).is_some_and(|(_, opt)| opt.requires_nightly))
}

/// Host tools the selected options declare they need. The binary still owns
/// the checking — it only pre-flights tools it knows how to check.
fn required_tools<'a>(
    selected: &[String],
    flat_options: &'a [GeneratorOption],
) -> HashSet<&'a str> {
    selected
        .iter()
        .filter_map(|name| find_option(name, flat_options))
        .flat_map(|(_, opt)| opt.requires_tools.iter().map(String::as_str))
        .collect()
}

#[derive(Parser, Debug)]
#[command(author, version, about = HELP.about.as_str(), long_about = None, subcommand_negates_reqs = true)]
struct Args {
    /// Name of the project to generate
    name: Option<String>,

    /// Run in headless mode (i.e. do not use the TUI)
    #[arg(long)]
    headless: bool,

    /// Generation options
    #[arg(short, long, help = HELP.options.as_str())]
    option: Vec<String>,

    /// Directory in which to generate the project
    #[arg(short = 'O', long)]
    output_path: Option<PathBuf>,

    /// Generate from an external template: a directory, or a repository to
    /// clone (`owner/repo[@branch-or-tag]`, an `https://` URL, or `git@host:path`)
    #[arg(long, global = true, value_name = "DIR_OR_REPO")]
    template: Option<PathBuf>,

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

    /// Render a template across its option combinations, reporting what breaks
    Check {
        /// Options every combination is generated with. A pick for a required
        /// group narrows the sweep to it instead of covering the whole group.
        #[arg(short, long)]
        option: Vec<String>,

        /// Cover every valid combination of options, not just each option once
        #[arg(short, long)]
        all_combinations: bool,

        /// Leave a selection group out of the sweep entirely
        #[arg(long, value_name = "GROUP")]
        exclude_group: Vec<String>,

        /// Leave a category, and everything nested under it, out of the sweep
        #[arg(long, value_name = "CATEGORY")]
        exclude_category: Vec<String>,

        /// Sweep every option once per member of this group, rather than
        /// treating its members as ordinary options
        #[arg(long, value_name = "GROUP")]
        cross_group: Vec<String>,

        /// Also generate each combination and run cargo check, clippy and fmt
        /// over it
        #[arg(short, long)]
        build: bool,

        /// Print the combinations that would be checked, and stop
        #[arg(short, long)]
        dry_run: bool,
    },
}

impl SubCommands {
    fn handle(&self, loaded: &Loaded) -> Result<()> {
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

        let all_options = loaded.template.all_options();
        match self {
            SubCommands::ListOptions => {
                println!(
                    "The following template options are available. The group names are not part of the option name. Only one option in a group can be selected."
                );
                let mut groups = IndexMap::new();
                let mut seen = HashSet::new();
                for (index, option) in all_options.iter().enumerate() {
                    if option.name.is_empty() {
                        continue;
                    }
                    let group = groups.entry(&option.selection_group).or_insert(Vec::new());

                    if seen.insert(&option.name) {
                        group.push(index);
                    }
                }
                for (group, options) in groups {
                    if loaded.template.required.contains(group) {
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
            SubCommands::Check {
                option,
                all_combinations,
                exclude_group,
                exclude_category,
                cross_group,
                build,
                dry_run,
            } => validate::run(
                loaded,
                &validate::Request {
                    sweep: sweep::SweepOptions {
                        coverage: if *all_combinations {
                            sweep::Coverage::Combinations
                        } else {
                            sweep::Coverage::Individual
                        },
                        pinned: option.clone(),
                        excluded_groups: exclude_group.clone(),
                        excluded_categories: exclude_category.clone(),
                        crossed_groups: cross_group.clone(),
                    },
                    build: *build,
                    dry_run: *dry_run,
                },
            ),
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

static BUNDLED: LazyLock<Result<Loaded, String>> =
    LazyLock::new(|| Loaded::open(TemplateSource::Bundled).map_err(|e| format!("{e:#}")));

/// The pick for every required selection group that offers exactly one option.
fn forced_picks(template: &Template) -> Vec<String> {
    let all = template.all_options();
    template
        .required
        .iter()
        .filter_map(|group| {
            let mut members = all.iter().filter(|o| &o.selection_group == group);
            let only = members.next()?;
            members.next().is_none().then(|| only.name.clone())
        })
        .collect()
}

fn wants_interactive(
    headless: bool,
    user_chose_nothing: bool,
    missing_required: &[String],
    name: Option<&str>,
) -> bool {
    if !missing_required.is_empty() || name.is_none() {
        return true;
    }
    user_chose_nothing && !headless
}

/// Locate what a `--template` value names, cloning it first if it is remote.
fn locate_template(value: &str) -> Result<(PathBuf, Option<fetch::Checkout>)> {
    Ok(match fetch::parse_template_arg(value)? {
        fetch::TemplateRef::Local(dir) => (dir, None),
        fetch::TemplateRef::Repo { url, reference } => {
            log::info!(
                "Cloning template from {url}{}",
                reference
                    .as_deref()
                    .map(|r| format!(" at {r}"))
                    .unwrap_or_default()
            );
            let checkout = fetch::clone(&url, reference.as_deref())?;
            // The resolved commit, so a generated project can be traced back to
            // exactly what produced it even when the ref later moves.
            log::info!("Template resolved to {url}@{}", checkout.commit);
            (checkout.root.clone(), Some(checkout))
        }
    })
}

fn template_source(args: &Args) -> Result<(TemplateSource, Option<fetch::Checkout>)> {
    let Some(value) = args.template.as_ref() else {
        return Ok((TemplateSource::Bundled, None));
    };

    let (root, checkout) = locate_template(&value.to_string_lossy())?;

    log::warn!(
        "⚠️  Generating from the external template at `{}`. A template controls \
         what code and dependencies end up in your project — only use ones you trust.",
        root.display()
    );

    Ok((TemplateSource::Directory(root), checkout))
}

/// The text clap needs while it is building [`Args`] — that is, before any
/// argument has been parsed, so it cannot ask clap which template was chosen.
struct HelpText {
    about: String,
    options: String,
}

static HELP: LazyLock<HelpText> = LazyLock::new(HelpText::build);

impl HelpText {
    fn build() -> Self {
        let external = help_requested()
            .then(template_arg_from_env)
            .flatten()
            .and_then(|value| match Self::external(&value) {
                Ok(text) => Some(text),
                Err(e) => {
                    log::warn!("Describing the template at `{value}` failed: {e:#}");
                    None
                }
            });

        external.unwrap_or_else(Self::bundled)
    }

    fn bundled() -> Self {
        match BUNDLED.as_ref() {
            Ok(loaded) => Self::describe(loaded),
            Err(_) => Self {
                about: ABOUT.to_string(),
                options: "Generation options".to_string(),
            },
        }
    }

    fn external(value: &str) -> Result<Self> {
        // The checkout must outlive reading the template out of it.
        let (root, _checkout) = locate_template(value)?;
        Ok(Self::describe(&Loaded::open(TemplateSource::Directory(
            root,
        ))?))
    }

    fn describe(loaded: &Loaded) -> Self {
        Self {
            about: about_text(&loaded.source),
            options: option_help(&loaded.template),
        }
    }
}

/// Whether the user asked for help, and so whether the help text is worth
/// building properly.
fn help_requested() -> bool {
    env::args().any(|arg| ["-h", "--help", "help"].contains(&arg.as_str()))
}

/// The `--template` value, read straight from the process arguments.
fn template_arg_from_env() -> Option<String> {
    template_arg(env::args())
}

fn template_arg(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            return None;
        }
        if let Some(value) = arg.strip_prefix("--template=") {
            return Some(value.to_string());
        }
        if arg == "--template" {
            return args.next();
        }
    }
    None
}

const ABOUT: &str =
    "Template generation tool to create no_std applications targeting Espressif's chips.";

fn about_text(source: &TemplateSource) -> String {
    let mut about = ABOUT.to_string();

    let Some(toml) = source
        .get("Cargo.toml")
        .and_then(|raw| cargo::CargoToml::load(raw.as_ref()).ok())
    else {
        return about;
    };

    about.push_str("\n\nThe template will use these versions:\n");
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

/// Every name `-o` accepts, in template order.
fn option_names(template: &Template) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for option in template.options.iter() {
        for opt in option.options() {
            // Remove duplicates, which usually are chip-specific variations of an option.
            // An example of this is probe-rs. An unnamed entry is a placeholder
            // the generator fills in at runtime, so `-o` cannot name it.
            if !opt.is_empty() && !names.contains(&opt) {
                names.push(opt);
            }
        }
    }
    names
}

fn option_help(template: &Template) -> String {
    format!(
        "Generation options: {} - For more information regarding the different options check the esp-generate README.md (https://github.com/esp-rs/esp-generate/blob/main/README.md).",
        option_names(template).join(", ")
    )
}

fn setup_args_interactive(template: &Template, args: &mut Args) -> Result<()> {
    if args.headless {
        let mut missing = String::from(
            "You are in headless mode, but esp-generate needs more information to generate your project.",
        );
        // Surface every required selection group that doesn't have a pick
        // in `-o`, not just the chip. Templates declare their required
        // groups in `template.yaml::required`; `chip` happens to be the
        // only one today, but the generator doesn't hard-code that.
        for group in template.missing_required_groups(&args.option) {
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

    if let Some(subcommand) = args.subcommands.take() {
        let (source, _checkout) = template_source(&args)?;
        return subcommand.handle(&Loaded::open(source)?);
    }

    // Only check for updates once the command-line arguments have been processed,
    // to avoid printing any update notifications when the help message is
    // displayed.
    #[cfg(feature = "update-informer")]
    if !args.skip_update_check {
        check_for_update(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    }

    // Held for the whole run: a cloned template is deleted when this drops.
    let (source, _checkout) = template_source(&args)?;
    let loaded = Loaded::open(source)?;

    let user_chose_nothing = args.option.is_empty();

    for pick in forced_picks(&loaded.template) {
        if !args.option.contains(&pick) {
            args.option.push(pick);
        }
    }

    let missing_required = loaded.template.missing_required_groups(&args.option);
    if wants_interactive(
        args.headless,
        user_chose_nothing,
        &missing_required,
        args.name.as_deref(),
    ) {
        setup_args_interactive(&loaded.template, &mut args)?;
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
        loaded
            .source
            .get("Cargo.toml")
            .ok_or_else(|| anyhow::anyhow!("template has no `Cargo.toml`"))?
            .as_ref(),
    )
    .map_err(|e| anyhow::anyhow!("template `Cargo.toml` is unreadable: {e}"))?;

    // TODO: do not assume esp-hal version is present
    let esp_hal_version_full =
        render::esp_hal_version_full(&versions.dependency_version("esp-hal"));

    // A Cargo MSRV is not strict semver — `1.95` is legal — so parse leniently.
    let msrv_raw = versions.msrv();
    let Some(msrv) = check::parse_lenient(msrv_raw) else {
        bail!("template `Cargo.toml` has an unparsable `rust-version`: `{msrv_raw}`");
    };

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
    let toolchain_category = toolchain::ToolchainCategory::capture(&loaded.template.options);

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
        for opt in loaded.template.all_options() {
            for key in opt.compatible.keys() {
                if seen.insert(key) {
                    keys.push(key.clone());
                }
            }
        }
        keys
    };

    // Initial pruning
    let initial_selections: HashMap<String, String> = loaded
        .template
        .all_options()
        .iter()
        .filter(|o| args.option.iter().any(|n| n == &o.name) && !o.selection_group.is_empty())
        .map(|o| (o.selection_group.clone(), o.name.clone()))
        .collect();
    let initial_options = build_options(
        &loaded.template,
        &initial_selections,
        toolchain_category.as_ref(),
        headless_toolchain,
    );

    process_options(
        &loaded,
        &Template {
            options: initial_options.clone(),
            required: loaded.template.required.clone(),
        },
        &args,
    )?;

    let mut initial_selected = args.option.clone();
    if let Some(ref tc) = args.toolchain {
        initial_selected.push(tc.clone());
    }

    // Facts come from the resolved plugins, so a template sees the vocabulary
    // version it pinned.
    let initial_facts = Some(
        loaded
            .resolved
            .facts(&plugin::selection(
                initial_selected.clone(),
                &flatten_options(&initial_options),
            ))
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    );

    let repository = tui::Repository::new(initial_options, &initial_selected, initial_facts);

    let (selected, flat_options) = if !args.headless {
        let mut app = tui::App::new(repository, loaded.template.required.clone());

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
                let picked =
                    plugin::selection(app.selected_options(), &app.repository.config.flat_options);
                let new_facts = loaded
                    .resolved
                    .facts(&picked)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                let filtered = toolchain::toolchains_for_chip(
                    &cached_toolchains,
                    toolchain::ChipTarget::from_facts(&new_facts).as_ref(),
                    &msrv,
                    args.toolchain.as_deref(),
                );
                for warning in &filtered.warnings {
                    log::warn!("{warning}");
                }

                // Facts before options: the cascade must run against the new
                // chip's capabilities, not the outgoing chip's.
                app.repository.config.set_facts(Some(new_facts));

                let new_options = build_options(
                    &loaded.template,
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

        (sel, app.repository.config.flat_options)
    } else {
        (initial_selected, repository.config.flat_options)
    };

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
    let selected_toolchain = selected
        .iter()
        .find(|name| {
            find_option(name, &flat_options)
                .is_some_and(|(_, opt)| opt.selection_group == "toolchain")
        })
        .cloned();

    let (facts, target) = render::facts(
        &loaded,
        &selected,
        &flat_options,
        &render::HostValues {
            project_name: name.clone(),
            generate_parameters: selected_options,
            esp_hal_version_full,
            rust_toolchain: selected_toolchain.clone(),
        },
    )?;

    if let Some(target) = target {
        let tools = required_tools(&selected, &flat_options);
        check::check(
            target.is_xtensa,
            tools.contains("probe-rs"),
            msrv,
            requires_nightly(&selected, &flat_options, target.is_xtensa),
            args.headless,
            selected_toolchain.as_deref(),
        );
    }

    let project_dir = path.join(&name);

    if check::offensive_cargo_config_check(&project_dir) {
        println!(
            "⚠️ `.cargo/config.toml` files found in parent directories - this can cause undesired behavior. See https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure"
        );
    }

    // Before rendering, so an existing directory fails immediately.
    fs::create_dir(&project_dir)?;

    for (out_path, contents) in render::plan(&loaded, &selected, &flat_options, &facts)?.files {
        let out_path = project_dir.join(out_path);
        fs::create_dir_all(out_path.parent().unwrap())?;
        fs::write(out_path, contents)?;
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
    template: &Template,
    selections: &HashMap<String, String>,
    toolchain_category: Option<&toolchain::ToolchainCategory>,
    toolchains: &[String],
) -> Vec<GeneratorOptionItem> {
    let mut options = template.options.clone();
    prune_incompatible_options(selections, &mut options);
    if let Some(category) = toolchain_category {
        category.populate(&mut options, toolchains);
    }
    options
}

fn process_options(loaded: &Loaded, template: &Template, args: &Args) -> Result<()> {
    let mut success = true;
    // Two option catalogues, with complementary coverage:
    //   - `populated_options` is the pruned, post-`build_options` view: it
    //     knows about dynamically-populated entries (module options, etc.)
    //     but only those compatible with the current selections.
    //   - `pristine_options` is the raw template: it lists every option
    //     (including those pruned by `compatible`), so we can tell
    //     "pruned by selection" apart from "unknown name".
    let populated_options = template.all_options();
    let pristine_options = loaded.template.all_options();

    let flat_options = flatten_options(&template.options);
    let selected: Vec<usize> = args
        .option
        .iter()
        .flat_map(|opt_name| flat_options.iter().position(|o| &o.name == opt_name))
        .collect();

    let facts = Some(
        loaded
            .resolved
            .facts(&plugin::selection(args.option.clone(), &flat_options))
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    );

    let selected_config = ActiveConfiguration {
        selected,
        flat_options,
        options: template.options.clone(),
        facts,
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
    use esp_generate::config::flatten_options;
    use esp_generate::manifest;
    use esp_generate::template::SetValue;
    use esp_template_plugin_chip::Chip;
    use strum::IntoEnumIterator;

    use super::*;

    /// The bundled template, for the tests that assert against the real thing.
    pub fn bundled() -> &'static Loaded {
        super::BUNDLED
            .as_ref()
            .expect("the bundled template must load")
    }

    /// Loading `MANIFEST` runs the same check, but only when generating — this
    /// makes a rename that outdates a rule fail `cargo test`.
    #[test]
    fn the_bundled_manifest_matches_the_bundled_files() {
        let _ = bundled();
    }

    fn template_arg_of(args: &[&str]) -> Option<String> {
        template_arg(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn the_template_argument_is_found_in_either_spelling() {
        assert_eq!(
            template_arg_of(&["esp-generate", "--template", "dir", "name"]).as_deref(),
            Some("dir")
        );
        assert_eq!(
            template_arg_of(&["esp-generate", "--template=dir", "name"]).as_deref(),
            Some("dir")
        );
        assert_eq!(template_arg_of(&["esp-generate", "name"]), None);
        assert_eq!(template_arg_of(&["esp-generate", "--template"]), None);
    }

    /// After `--` it is a project name, not the flag.
    #[test]
    fn a_template_argument_past_the_separator_is_not_one() {
        assert_eq!(
            template_arg_of(&["esp-generate", "--", "--template", "dir"]),
            None
        );
    }

    /// The bundled toolchain row is an unnamed placeholder, filled in from the
    /// installed toolchains at runtime. `-o` has no name to accept for it.
    #[test]
    fn an_unnamed_placeholder_is_never_offered_as_an_option() {
        let template = &bundled().template;
        assert!(
            template.all_options().iter().any(|o| o.name.is_empty()),
            "the bundled template should still have a placeholder to filter"
        );
        assert!(!option_names(template).iter().any(|n| n.is_empty()));
    }

    /// Old-syntax directives are emitted verbatim rather than rejected, so a
    /// missed one is invisible until someone reads a generated project. Walks
    /// `bundled().source.files()` rather than globbing, so dotfiles can't be skipped.
    #[test]
    fn no_file_uses_the_pre_somni_directive_syntax() {
        const GONE: [&str; 3] = ["REPLACE", "INCLUDEFILE", "INCLUDE_AS"];

        for (path, contents) in bundled().source.files().unwrap() {
            for (n, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start();
                for prefix in ["//", "#", "--"] {
                    let Some(rest) = trimmed.strip_prefix(prefix) else {
                        continue;
                    };
                    if let Some(directive) = GONE.iter().find(|d| rest.starts_with(**d)) {
                        panic!(
                            "{path}:{} uses the removed `{prefix}{directive}` directive: {trimmed}",
                            n + 1
                        );
                    }
                }
            }
        }
    }

    /// The reserved directory is why partials need no per-file marker; if the
    /// rule stopped applying, every partial would be emitted as a source file.
    #[test]
    fn no_template_machinery_is_emitted() {
        let manifest = &bundled().manifest;
        for (path, _) in bundled().source.files().unwrap() {
            let machinery = path == manifest::MANIFEST_PATH
                || path == "template.yaml"
                || path.starts_with(&format!("{}/", manifest::RESERVED_DIR));

            assert_eq!(
                manifest.emit(&path) == manifest::Emit::Never,
                machinery,
                "`{path}` is emitted iff it is not template machinery"
            );
        }
    }

    #[test]
    fn every_chip_has_at_least_one_module() {
        let module_category = bundled()
            .template
            .options
            .iter()
            .find_map(|item| match item {
                GeneratorOptionItem::Category(c) if c.name == "module" => Some(c),
                _ => None,
            })
            .expect("module category is required by the generator");

        for chip in Chip::iter() {
            let chip_name = chip.to_string();
            let has_module = module_category.options.iter().any(|item| {
                let GeneratorOptionItem::Option(o) = item else {
                    return false;
                };
                o.compatible
                    .get("chip")
                    .is_some_and(|list| list.iter().any(|n| n == &chip_name))
            });
            assert!(has_module, "no modules declared for {chip_name}");
        }
    }

    #[test]
    fn the_tui_runs_when_input_is_needed_or_nothing_was_asked_for() {
        let nothing: &[String] = &[];
        let missing = &["chip".to_string()][..];

        // Something is missing: always ask, headless included — that is where
        // headless reports what it needs.
        assert!(wants_interactive(false, false, missing, Some("p")));
        assert!(wants_interactive(true, false, missing, Some("p")));
        assert!(wants_interactive(true, false, nothing, None));

        // Nothing asked for, but everything satisfiable: offer the menu.
        assert!(wants_interactive(false, true, nothing, Some("p")));

        // …except in headless, which means "do not ask me".
        assert!(!wants_interactive(true, true, nothing, Some("p")));

        // The user made a choice and named the project: get on with it.
        assert!(!wants_interactive(false, false, nothing, Some("p")));
    }

    #[test]
    fn a_required_group_with_one_option_is_picked_for_you() {
        let one = |group: &str, name: &str| {
            GeneratorOptionItem::Option(GeneratorOption {
                name: name.to_string(),
                selection_group: group.to_string(),
                ..Default::default()
            })
        };

        let single = Template {
            required: vec!["chip".to_string()],
            options: vec![one("chip", "esp32c6"), one("editor", "vscode")],
        };
        assert_eq!(forced_picks(&single), ["esp32c6"], "the only chip");

        let several = Template {
            required: vec!["chip".to_string()],
            options: vec![one("chip", "esp32c6"), one("chip", "esp32h2")],
        };
        assert!(
            forced_picks(&several).is_empty(),
            "a real choice must stay the user's"
        );

        // The bundled template offers every chip, so nothing is forced there.
        assert!(forced_picks(&bundled().template).is_empty());
    }

    /// `diagram.json` picks its board with an `if`/`else if` chain and no
    /// `else`, so a chip the chain misses emits JSON with no `"type"` key —
    /// invalid, but only noticed when Wokwi refuses to open it.
    #[test]
    fn every_wokwi_chip_has_a_board_in_the_diagram() {
        let allowed = bundled()
            .template
            .all_options()
            .into_iter()
            .find(|o| o.name == "wokwi")
            .expect("the bundled template offers wokwi")
            .compatible
            .get("chip")
            .expect("wokwi is chip-restricted")
            .clone();

        let diagram = bundled()
            .source
            .get("diagram.json")
            .expect("wokwi ships a diagram");

        for chip in &allowed {
            assert!(
                diagram.contains(&format!("chip.name == \"{chip}\"")),
                "`diagram.json` has no board for `{chip}`, which wokwi allows"
            );
        }
    }

    /// Groups the binary populates at runtime, picked with a dedicated flag
    /// rather than `-o`.
    const NOT_DASH_O: &[&str] = &["toolchain"];

    /// Everything about an option that changes what selecting it *does*.
    /// Excludes `display_name`/`help` (presentation) and `compatible` (already
    /// applied by pruning before we compare).
    #[derive(Debug, PartialEq)]
    struct SelectionBehaviour<'a> {
        selection_group: &'a str,
        requires: &'a [String],
        requires_capabilities: &'a [String],
        requires_nightly: bool,
        sets: Vec<(&'a str, &'a SetValue)>,
    }

    impl<'a> SelectionBehaviour<'a> {
        fn of(option: &'a GeneratorOption) -> Self {
            Self {
                selection_group: &option.selection_group,
                requires: &option.requires,
                requires_capabilities: &option.requires_capabilities,
                requires_nightly: option.requires_nightly,
                sets: option.sets.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            }
        }
    }

    /// Every TUI-selectable option must be expressible on the command line, or
    /// the two front ends drift.
    ///
    /// `-o <name>` resolves to the *first* option with that name, so a duplicate
    /// name is only safe while both entries behave identically — as the bundled
    /// `probe-rs` and `defmt` pairs do.
    #[test]
    fn every_selectable_option_is_expressible_as_dash_o() {
        for chip in Chip::iter() {
            let selections = HashMap::from([("chip".to_string(), chip.to_string())]);
            let options = build_options(&bundled().template, &selections, None, &[]);
            let flat = flatten_options(&options);

            let mut by_name: HashMap<&str, Vec<&GeneratorOption>> = HashMap::new();
            for option in &flat {
                if NOT_DASH_O.contains(&option.selection_group.as_str()) {
                    continue;
                }
                assert!(
                    !option.name.is_empty(),
                    "{chip}: an option with no name cannot be selected with `-o`"
                );
                by_name
                    .entry(option.name.as_str())
                    .or_default()
                    .push(option);
            }

            for (name, variants) in by_name {
                let [first, rest @ ..] = variants.as_slice() else {
                    unreachable!("every entry has at least one variant")
                };
                for other in rest {
                    assert_eq!(
                        SelectionBehaviour::of(first),
                        SelectionBehaviour::of(other),
                        "{chip}: `-o {name}` resolves to the first of several options that do \
                         not behave the same, so the flag cannot express what the TUI can"
                    );
                }
            }
        }
    }

    /// The exemption above must stay earned: if the toolchain group stops
    /// carrying a name `-o` can't express, the entry is dead.
    #[test]
    fn the_dash_o_exemption_is_still_needed() {
        let flat = flatten_options(&bundled().template.options);

        for group in NOT_DASH_O {
            let members: Vec<&str> = flat
                .iter()
                .filter(|o| o.selection_group == *group)
                .map(|o| o.name.as_str())
                .collect();

            assert!(!members.is_empty(), "`{group}` has no options at all");
            assert!(
                members.iter().any(|name| name.is_empty()),
                "`{group}` is exempt from `-o` parity, but every member is nameable: {members:?}"
            );
        }
    }

    /// The nightly requirement comes from the option tree, not a known name.
    /// `find_option` returns the first match by name, so every variant of an
    /// option must declare the same tools.
    #[test]
    fn the_tool_requirement_is_read_from_the_option_tree() {
        let flat = flatten_options(&bundled().template.options);

        assert!(
            required_tools(&["probe-rs".to_string()], &flat).contains("probe-rs"),
            "the bundled template must declare `requires_tools` on probe-rs"
        );
        assert!(required_tools(&["alloc".to_string()], &flat).is_empty());
        assert!(required_tools(&[], &flat).is_empty());

        for opt in flat.iter().filter(|o| o.name == "probe-rs") {
            assert!(
                opt.requires_tools.iter().any(|t| t == "probe-rs"),
                "every `probe-rs` variant must declare the tool"
            );
        }
    }

    #[test]
    fn the_nightly_requirement_is_read_from_the_option_tree() {
        let flat = flatten_options(&bundled().template.options);
        let ssp = vec!["stack-smashing-protection".to_string()];

        assert!(
            requires_nightly(&ssp, &flat, false),
            "the bundled template must declare `requires_nightly` on this option"
        );
        assert!(!requires_nightly(&["alloc".to_string()], &flat, false));
        assert!(!requires_nightly(&[], &flat, false));

        assert!(!requires_nightly(&ssp, &flat, true), "Xtensa is exempt");
    }
}
