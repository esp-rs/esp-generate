use anyhow::Result;
use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Chip;
use env_logger::{Builder, Env, Logger};
use esp_generate::{
    append_list_as_sentence,
    config::{ActiveConfiguration, Relationships, flatten_options},
    template::GeneratorOptionItem,
};
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use ratatui::crossterm::{
    ExecutableCommand,
    event::{Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, style::palette::tailwind, widgets::*};

static DEFER_WARNS: AtomicBool = AtomicBool::new(false);
static WARN_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Initializes and binds the global logger with default `env_logger` settings, wrapped into [DeferringLogger],
/// so `log::warn!` can be deferred while the TUI is active.
pub fn setup_logger() -> Result<(), SetLoggerError> {
    let logger = Builder::from_env(Env::default().default_filter_or(LevelFilter::Info.as_str()))
        .format_target(false)
        .build();

    let max_level = logger.filter();
    let result = log::set_boxed_logger(Box::new(DeferringLogger { inner: logger }));
    if result.is_ok() {
        // `Builder::init()` sets this, it `log::warn!` is off otherwise.
        log::set_max_level(max_level);
    }
    result
}

pub struct Repository {
    pub config: ActiveConfiguration,
    path: Vec<usize>,
}

impl Repository {
    pub fn new(chip: Chip, options: Vec<GeneratorOptionItem>, selected: &[String]) -> Self {
        let flat_options = flatten_options(&options);
        Self {
            config: ActiveConfiguration {
                chip,
                selected: selected
                    .iter()
                    .flat_map(|option| flat_options.iter().position(|o| &o.name == option))
                    .collect(),
                flat_options,
                options,
            },
            path: Vec::new(),
        }
    }

    /// Rebuild the options tree for a (possibly different) chip.
    ///
    /// The `options` argument is the *fully prepared* tree: the caller is
    /// responsible for running chip-filtering, module population and toolchain
    /// population against a pristine template. This keeps `Repository` free of
    /// chip-specific knowledge — it only owns the mechanical "my tree changed,
    /// keep my state consistent" primitive.
    ///
    /// The menu path is trimmed to the depth that still resolves against the
    /// new tree rather than reset wholesale, so callers that repopulate a
    /// single category on the *same* chip (notably the toolchain scan) keep
    /// their cursor where it is; callers that truly switch chips typically
    /// want to [`Self::reset_path`] after this.
    pub fn set_options(&mut self, chip: Chip, options: Vec<GeneratorOptionItem>) {
        self.config.reset_options(chip, options);

        // Trim the navigation path to whatever still resolves in the new tree.
        let mut current: &[GeneratorOptionItem] = &self.config.options;
        let mut valid_depth = 0;
        for &idx in &self.path {
            match current.get(idx) {
                Some(GeneratorOptionItem::Category(category)) => {
                    valid_depth += 1;
                    current = category.options.as_slice();
                }
                _ => break,
            }
        }
        self.path.truncate(valid_depth);
    }

    /// Collapse the menu path back to the root. Intended for callers that have
    /// just switched chip and want the UI to start from a known location.
    pub fn reset_path(&mut self) {
        self.path.clear();
    }

    /// Returns the *explicitly* selected toolchain, if there is any
    fn selected_toolchain(&self) -> Option<String> {
        self.config.selected.iter().find_map(|idx| {
            let option = &self.config.flat_options[*idx];
            if option.selection_group == "toolchain" {
                Some(option.name.clone())
            } else {
                None
            }
        })
    }

    /// Returns the chip that is currently ticked in the `chip` selection
    /// group, if any. The main loop polls this after every event to detect
    /// user-driven chip switches; disagreement with [`ActiveConfiguration::chip`]
    /// triggers a full options-tree rebuild.
    ///
    /// Returns `None` only in degenerate situations (e.g. tests that build a
    /// `Repository` without a chip category). Callers should fall back to
    /// `self.config.chip` in that case.
    pub fn selected_chip(&self) -> Option<Chip> {
        self.config.selected.iter().find_map(|idx| {
            let option = &self.config.flat_options[*idx];
            if option.selection_group == "chip" {
                option.name.parse().ok()
            } else {
                None
            }
        })
    }

    /// Current depth of the menu navigation path. Used by `App` to keep its
    /// `ListState` stack synchronized with `Repository::path` after a
    /// `set_options` may have trimmed the path.
    pub fn path_len(&self) -> usize {
        self.path.len()
    }

    fn current_level(&self) -> &[GeneratorOptionItem] {
        let mut current: &[GeneratorOptionItem] = &self.config.options;

        for &index in &self.path {
            current = match &current[index] {
                GeneratorOptionItem::Category(category) => category.options.as_slice(),
                GeneratorOptionItem::Option(_) => unreachable!(),
            }
        }

        current
    }

    /// Tree-indices of the items that pass their `compatible` check at the
    /// current menu level, in their original order.
    ///
    /// The menu renders and navigates over this filtered view so that items
    /// failing their `compatible` constraint are invisible to the user — the
    /// generalised replacement for the chip-level pruning that used to happen
    /// at tree-build time. Row indices produced by ratatui's `ListState`
    /// therefore index into `visible_indices()`, not into `current_level()`,
    /// and every caller that needs the underlying tree item must translate via
    /// [`Self::visible_item`].
    fn visible_indices(&self) -> Vec<usize> {
        let level = self.current_level();
        level
            .iter()
            .enumerate()
            .filter(|(_, v)| self.is_item_visible(v))
            .map(|(i, _)| i)
            .collect()
    }

    /// Translate a row index (as delivered by the TUI's `ListState`) into the
    /// underlying item at the current level.
    pub fn visible_item(&self, row: usize) -> &GeneratorOptionItem {
        let level = self.current_level();
        let tree_idx = self.visible_indices()[row];
        &level[tree_idx]
    }

    /// Number of items currently visible at this level. The TUI renders
    /// exactly this many rows.
    pub fn visible_count(&self) -> usize {
        self.visible_indices().len()
    }

    /// An item is visible in the TUI when:
    ///   * for a plain option: its `compatible` constraint is satisfied —
    ///     i.e. every referenced selection group has an allowed option active;
    ///   * for a category: at least one of its children (recursively) is
    ///     visible. An empty category — real or filtered — is hidden too.
    fn is_item_visible(&self, item: &GeneratorOptionItem) -> bool {
        match item {
            GeneratorOptionItem::Option(option) => self.config.is_option_compatible(option),
            GeneratorOptionItem::Category(category) => {
                category.options.iter().any(|child| self.is_item_visible(child))
            }
        }
    }

    /// Returns `true` if the current menu level is inside a category called `name`.
    fn is_in_category(&self, name: &str) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let mut current: &[GeneratorOptionItem] = &self.config.options;
        let mut last: Option<&GeneratorOptionItem> = None;

        for &index in &self.path {
            last = current.get(index);
            current = match last {
                Some(GeneratorOptionItem::Category(category)) => category.options.as_slice(),
                Some(GeneratorOptionItem::Option(_)) | None => return false,
            };
        }

        matches!(
            last,
            Some(GeneratorOptionItem::Category(category)) if category.name == name
        )
    }

    fn current_level_is_active(&self) -> bool {
        let mut current: &[GeneratorOptionItem] = &self.config.options;

        for &index in &self.path {
            if !self.config.is_active(&current[index]) {
                return false;
            }
            current = match &current[index] {
                GeneratorOptionItem::Category(category) => category.options.as_slice(),
                GeneratorOptionItem::Option(_) => unreachable!(),
            }
        }

        true
    }

    fn is_item_actionable(&self, item: &GeneratorOptionItem) -> bool {
        match item {
            GeneratorOptionItem::Category(_) => self.config.is_active(item),
            // Permissive: negative-requirement conflicts are treated as force-deselect
            // hints, not gates. `toggle_current` routes through `select_idx`, which
            // cascades them out.
            GeneratorOptionItem::Option(option) => self.config.is_option_toggleable(option),
        }
    }

    /// `row` is the index delivered by the TUI's `ListState`, i.e. a position
    /// inside the filtered visible view. It's translated to the real tree
    /// index before being pushed onto `path`.
    fn enter_group(&mut self, row: usize) {
        let tree_idx = self.visible_indices()[row];
        self.path.push(tree_idx);
    }

    /// `row` is a visible-row index (see [`Self::enter_group`]). Options in
    /// the `chip` selection group behave as a radio — clicking the already-
    /// selected chip is a no-op; switching chips goes through the usual
    /// `select_idx` cascade.
    fn toggle_current(&mut self, row: usize) {
        if !self.current_level_is_active() {
            return;
        }

        let GeneratorOptionItem::Option(ref option) = *self.visible_item(row) else {
            ratatui::restore();
            unreachable!();
        };

        let option_name = option.name.clone();
        // The chip group behaves like a required radio: there must always be
        // exactly one chip ticked (the one backing the current options tree),
        // so clicking the already-selected chip is a no-op instead of a
        // deselect. Switching to a different chip still works through the
        // usual selection_group mutex in `select_idx`.
        let is_chip_group = option.selection_group == "chip";

        if let Some(i) = self
            .config
            .selected
            .iter()
            .position(|s| self.config.flat_options[*s].name == option_name)
        {
            if is_chip_group {
                return;
            }
            let idx = self.config.selected[i];
            self.config.deselect_idx(idx);
        } else if self.config.is_option_toggleable(option) {
            self.config.select(&option_name);
        }
    }

    /// `row` is a visible-row index (see [`Self::enter_group`]).
    fn is_option(&self, row: usize) -> bool {
        matches!(self.visible_item(row), GeneratorOptionItem::Option(_))
    }

    fn up(&mut self) {
        self.path.pop();
    }

    /// Builds the visible list rows.
    ///
    /// `hovered` is the index of the currently highlighted row (what the cursor is
    /// on). Only that row gets the "will deselect: …" warning trailer (styled yellow)
    /// when a selection there would force-deselect something; every other row keeps
    /// its normal right-aligned name. This keeps the list quiet and surfaces the
    /// side-effect info only when the user is actually considering the action.
    fn current_level_desc(
        &self,
        width: u16,
        style: &UiElements,
        hovered: Option<usize>,
    ) -> Vec<(bool, Line<'static>)> {
        let level = self.current_level();
        let level_active = self.current_level_is_active();

        // Iterate the *visible* view only: items whose `compatible` constraint
        // currently fails are hidden from the TUI as part of the cascade, and
        // their stale selections (if any) have already been cleared by
        // `drop_unsatisfied`. `idx` is the row index handed back to the
        // `ListState`, which is why every navigation helper translates through
        // `Self::visible_indices` before touching the real tree.
        let visible: Vec<(usize, &GeneratorOptionItem)> = level
            .iter()
            .enumerate()
            .filter(|(_, v)| self.is_item_visible(v))
            .collect();

        visible
            .iter()
            .enumerate()
            .map(|(idx, &(_, v))| {
                let is_selected = self
                    .config
                    .selected
                    .iter()
                    .any(|o| self.config.flat_options[*o].name == v.name())
                    && level_active;
                let indicator = if is_selected {
                    style.selected
                } else if v.is_category() {
                    style.category
                } else {
                    style.unselected
                };

                // The option's internal name is always shown on the right; only the
                // hovered row additionally appends a yellow " - will deselect: …"
                // clause when selecting it would force-deselect something. When the
                // detailed list wouldn't fit, the clause degrades to
                // " - will deselect N" (count) before any actual truncation kicks in.
                let name_part: String = match v {
                    GeneratorOptionItem::Option(_) => v.name().to_string(),
                    GeneratorOptionItem::Category(_) => String::new(),
                };

                // reserve indicator spacing; saturating_sub keeps padding non-negative so narrow widths don't overflow
                let padding = (width as usize).saturating_sub(v.title().len() + 4);

                let is_hovered = hovered == Some(idx);
                let is_actionable = self.is_item_actionable(v);
                // Symmetric: whether the row is selected or not, toggling it may
                // force-deselect others — always ask the config what would happen.
                // Gated on `is_actionable` so rows the user can't actually toggle
                // (chip-mismatch or unmet positive requirements) don't advertise a
                // phantom cascade. Rows with only a negative-requirement conflict
                // remain actionable (that's the whole point of the warning).
                let warning_part: Option<String> = match v {
                    GeneratorOptionItem::Option(option)
                        if is_hovered && level_active && is_actionable =>
                    {
                        let evicted = self.config.would_force_deselect(option);
                        if evicted.is_empty() {
                            None
                        } else {
                            let detailed = format!(" - will deselect: {}", evicted.join(", "));
                            let budget = padding.saturating_sub(name_part.len());
                            if detailed.len() <= budget {
                                Some(detailed)
                            } else {
                                Some(format!(" - will deselect {}", evicted.len()))
                            }
                        }
                    }
                    _ => None,
                };

                let trailer_len = name_part.len() + warning_part.as_deref().map_or(0, str::len);
                let lead = format!(" {} {}", indicator, v.title());
                let pad = " ".repeat(padding.saturating_sub(trailer_len));

                let mut spans = vec![Span::raw(lead), Span::raw(pad), Span::raw(name_part)];
                if let Some(warning) = warning_part {
                    spans.push(Span::styled(warning, style.force_deselect_style));
                }

                (level_active && is_actionable, Line::from(spans))
            })
            .collect()
    }
}

pub fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    enable_deferred_logging();
    Ok(terminal)
}

pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    flush_deferred_logs();
    Ok(())
}

/// Enable routing `log::warn!` into a buffer instead of stderr.
fn enable_deferred_logging() {
    let mut guard = WARN_BUFFER.lock().unwrap();
    guard.clear();
    DEFER_WARNS.store(true, Ordering::Relaxed);
}

/// Emits buffered warnings, then disables deferred logging.
fn flush_deferred_logs() {
    let mut guard = WARN_BUFFER.lock().unwrap();
    DEFER_WARNS.store(false, Ordering::Relaxed);
    let msgs = std::mem::take(&mut *guard);
    drop(guard);

    for msg in msgs {
        log::warn!("{msg}");
    }
}

struct DeferringLogger {
    inner: Logger,
}

impl Log for DeferringLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if DEFER_WARNS.load(Ordering::Relaxed)
            && record.level() == Level::Warn
            && self.inner.matches(record)
        {
            WARN_BUFFER
                .lock()
                .unwrap()
                .push(format!("{}", record.args()));
            return;
        }
        self.inner.log(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

struct UiElements {
    selected: &'static str,
    unselected: &'static str,
    category: &'static str,
    /// Style applied to the " - will deselect …" hover clause.
    force_deselect_style: Style,
}

struct Colors {
    app_background: Color,
    header_bg: Color,
    normal_row_color: Color,
    help_row_color: Color,
    disabled_style_fg: Color,
    text_color: Color,

    selected_active_style: Style,
    selected_inactive_style: Style,
}

impl Colors {
    const RGB: Self = Self {
        app_background: tailwind::SLATE.c950,
        header_bg: tailwind::BLUE.c950,
        normal_row_color: tailwind::SLATE.c950,
        help_row_color: tailwind::SLATE.c800,
        disabled_style_fg: tailwind::GRAY.c600,
        text_color: tailwind::SLATE.c200,

        selected_active_style: Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(tailwind::SLATE.c200)
            .bg(tailwind::BLUE.c950),
        selected_inactive_style: Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(tailwind::SLATE.c400)
            .bg(tailwind::GRAY.c800),
    };
    const ANSI: Self = Self {
        app_background: Color::Black,
        header_bg: Color::DarkGray,
        normal_row_color: Color::Black,
        help_row_color: Color::DarkGray,
        disabled_style_fg: Color::DarkGray,
        text_color: Color::Gray,

        selected_active_style: Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(Color::White)
            .bg(Color::Blue),
        selected_inactive_style: Style::new()
            .add_modifier(Modifier::BOLD)
            .fg(Color::DarkGray)
            .bg(Color::LightBlue),
    };
}

impl UiElements {
    const FANCY: Self = Self {
        selected: "✅",
        unselected: "  ",
        category: "▶️",
        force_deselect_style: Style::new().fg(Color::Yellow),
    };
    const FALLBACK: Self = Self {
        selected: "*",
        unselected: " ",
        category: ">",
        force_deselect_style: Style::new().fg(Color::Yellow),
    };
}

pub enum AppResult {
    /// Keep running
    Continue,
    /// User confirmed quit (q + y)
    Quit,
    /// User pressed s/S to save and generate
    Save,
}

pub struct App {
    state: Vec<ListState>,
    pub repository: Repository,
    confirm_quit: bool,
    ui_elements: UiElements,
    colors: Colors,
    toolchains_loading: bool,
}

impl App {
    pub fn new(repository: Repository) -> Self {
        let mut initial_state = ListState::default();
        initial_state.select(Some(0));

        let (ui_elements, colors) = match std::env::var("TERM_PROGRAM").as_deref() {
            Ok("vscode") => (UiElements::FALLBACK, Colors::RGB),
            Ok("Apple_Terminal") => (UiElements::FALLBACK, Colors::ANSI),
            _ => (UiElements::FANCY, Colors::RGB),
        };

        Self {
            repository,
            state: vec![initial_state],
            confirm_quit: false,
            ui_elements,
            colors,
            toolchains_loading: false,
        }
    }
    pub fn selected(&self) -> usize {
        if let Some(current) = self.state.last() {
            current.selected().unwrap_or_default()
        } else {
            0
        }
    }

    pub fn select_next(&mut self) {
        if let Some(current) = self.state.last_mut() {
            current.select_next();
        }
    }
    pub fn select_previous(&mut self) {
        if let Some(current) = self.state.last_mut() {
            current.select_previous();
        }
    }
    pub fn enter_menu(&mut self) {
        let mut new_state = ListState::default();
        new_state.select(Some(0));
        self.state.push(new_state);
    }
    pub fn exit_menu(&mut self) {
        if self.state.len() > 1 {
            self.state.pop();
        }
    }
    pub fn set_toolchains_loading(&mut self, loading: bool) {
        self.toolchains_loading = loading;
    }

    /// Swap the repository's options tree and keep the UI state consistent.
    ///
    /// `Repository::set_options` trims the navigation path to whatever still
    /// resolves in the new tree; we mirror that here on the `ListState` stack.
    /// When the caller asks for a path reset (the typical chip-switch case),
    /// we also collapse back to the root menu with a fresh selection at the
    /// top so the user isn't dropped inside a now-unrelated category.
    pub fn set_options(
        &mut self,
        chip: Chip,
        options: Vec<GeneratorOptionItem>,
        reset_path: bool,
    ) {
        self.repository.set_options(chip, options);
        if reset_path {
            self.repository.reset_path();
        }

        // The state stack is always `path_len + 1` — one for the root level,
        // one per entered category. Truncate mirrors the path trim; the min-1
        // floor guarantees we always have a state for the visible level.
        let desired = self.repository.path_len() + 1;
        self.state.truncate(desired.max(1));
        if self.state.is_empty() {
            let mut fresh = ListState::default();
            fresh.select(Some(0));
            self.state.push(fresh);
        }
        if reset_path
            && let Some(last) = self.state.last_mut()
        {
            last.select(Some(0));
        }
    }
    pub fn selected_options(&self) -> Vec<String> {
        self.repository
            .config
            .selected
            .iter()
            .map(|idx| self.repository.config.flat_options[*idx].name.clone())
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::Repository;
    use crate::Chip;
    use esp_generate::template::{GeneratorOption, GeneratorOptionItem};
    use indexmap::IndexMap;

    fn option(name: &str, requires: &[&str]) -> GeneratorOptionItem {
        GeneratorOptionItem::Option(GeneratorOption {
            name: name.to_string(),
            display_name: name.to_string(),
            selection_group: String::new(),
            help: String::new(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            compatible: IndexMap::new(),
        })
    }

    /// Build an option that is only compatible with the given chips. The
    /// `compatible` map uses the `chip` selection group as its key, which is
    /// the generalisation of the old `chips: [...]` field — callers still
    /// just pass a `&[Chip]` for convenience.
    fn option_for_chips(name: &str, requires: &[&str], chips: &[Chip]) -> GeneratorOptionItem {
        let mut compatible = IndexMap::new();
        compatible.insert(
            "chip".to_string(),
            chips.iter().map(|c| c.to_string()).collect(),
        );
        GeneratorOptionItem::Option(GeneratorOption {
            name: name.to_string(),
            display_name: name.to_string(),
            selection_group: String::new(),
            help: String::new(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            compatible,
        })
    }

    /// Build an option belonging to the `chip` selection group, mirroring what
    /// the in-tree chip selector category looks like at runtime.
    fn chip_group_option(chip: Chip) -> GeneratorOptionItem {
        GeneratorOptionItem::Option(GeneratorOption {
            name: chip.to_string(),
            display_name: chip.to_string(),
            selection_group: "chip".to_string(),
            help: String::new(),
            requires: Vec::new(),
            compatible: IndexMap::new(),
        })
    }

    /// `UiElements` without any ratatui styling — keeps assertions about row text
    /// simple and independent of the themes `FANCY` / `FALLBACK` use.
    fn plain_ui() -> super::UiElements {
        super::UiElements {
            selected: "*",
            unselected: " ",
            category: ">",
            force_deselect_style: ratatui::style::Style::new(),
        }
    }

    #[test]
    fn toggling_method_clears_other_side_without_restoring_on_toggle_back() {
        // Mirrors the espflash ↔ probe-rs scenario: selecting `method` kicks out all
        // options that depend on `!method`. Toggling `method` back off must NOT
        // resurrect them — the old save/restore behaviour is gone on purpose.
        let options = vec![
            option("method", &[]),
            option("method-selected-a", &["method"]),
            option("method-unselected-a", &["!method"]),
            option("method-unselected-b", &["!method"]),
            option("defmt", &[]),
        ];

        let mut repository = Repository::new(
            Chip::Esp32,
            options,
            &[
                "method-unselected-a".to_string(),
                "method-unselected-b".to_string(),
                "defmt".to_string(),
            ],
        );

        repository.toggle_current(0);
        assert!(repository.config.is_selected("method"));
        assert!(!repository.config.is_selected("method-unselected-a"));
        assert!(!repository.config.is_selected("method-unselected-b"));
        assert!(repository.config.is_selected("defmt"));

        repository.config.select("method-selected-a");

        repository.toggle_current(0);
        assert!(!repository.config.is_selected("method"));
        assert!(!repository.config.is_selected("method-selected-a"));
        // None of the previously-cleared `!method` options come back.
        assert!(!repository.config.is_selected("method-unselected-a"));
        assert!(!repository.config.is_selected("method-unselected-b"));
        assert!(repository.config.is_selected("defmt"));
    }

    #[test]
    fn force_deselect_trailer_appears_only_on_hovered_row() {
        // The "will deselect …" trailer must:
        //  * only show on the hovered row (keeps the list quiet),
        //  * live in its own span so the theme can style it,
        //  * appear for BOTH directions — selecting a row that force-deselects
        //    others, and deselecting a selected row whose dependents cascade.
        let options = vec![
            option("method", &[]),
            option("method-unselected-a", &["!method"]),
            option("dependent", &["method"]),
        ];

        let repository = Repository::new(
            Chip::Esp32,
            options,
            &[
                "method".to_string(),
                "dependent".to_string(),
            ],
        );

        let ui = plain_ui();

        let row_text = |idx: usize, hover: Option<usize>| -> String {
            repository
                .current_level_desc(80, &ui, hover)
                .into_iter()
                .nth(idx)
                .map(|(_, line)| {
                    line.spans
                        .iter()
                        .map(|s| s.content.to_string())
                        .collect::<String>()
                })
                .unwrap()
        };

        // Deselect-side: hovering the already-selected `method` warns that
        // toggling it off would cascade out `dependent`.
        let method_hover = row_text(0, Some(0));
        assert!(
            method_hover.contains("method")
                && method_hover.contains("- will deselect: dependent"),
            "expected deselect-side warning on selected hovered row, got: {method_hover:?}"
        );

        // Select-side: hovering unselected `method-unselected-a` warns that
        // selecting it would kick out `method` (and, via cascade, `dependent`).
        let unselected_hover = row_text(1, Some(1));
        assert!(
            unselected_hover.contains("method-unselected-a")
                && unselected_hover.contains("- will deselect:")
                && unselected_hover.contains("method")
                && unselected_hover.contains("dependent"),
            "expected select-side warning on hovered row, got: {unselected_hover:?}"
        );

        // The warning must live in its own span (so the theme can style it) and
        // must not include the option name that was printed before it.
        let hover_line = repository
            .current_level_desc(80, &ui, Some(1))
            .into_iter()
            .nth(1)
            .map(|(_, line)| line)
            .unwrap();
        let warning_span = hover_line
            .spans
            .iter()
            .find(|s| s.content.contains("will deselect"))
            .expect("warning span");
        assert!(!warning_span.content.contains("method-unselected-a "));

        // Hovering a row whose toggle is non-destructive means nothing on any
        // row can carry the warning.
        for idx in 0..3 {
            let text = row_text(idx, Some(2 /* `dependent` — non-destructive */));
            assert!(
                !text.contains("will deselect"),
                "no row must show the warning when hover is non-destructive, got: {text:?}"
            );
        }

        // Visible-but-non-toggleable rows must stay silent even when
        // `would_force_deselect` would otherwise report evictions — a user
        // can't actually toggle the row, so advertising a phantom cascade is
        // misleading. `unmet-pos` has an unmet positive requirement but its
        // `compatible` constraint is trivially satisfied, so it still renders.
        //
        // (The `compatible` failure case is exercised separately below —
        // those rows are now HIDDEN from the TUI entirely, not shown silent.)
        let options = vec![
            option("method", &[]),
            option("unmet-pos", &["needs-x", "!method"]),
        ];
        let repository = Repository::new(Chip::Esp32, options, &["method".to_string()]);

        let (actionable, line) = repository
            .current_level_desc(80, &ui, Some(1))
            .into_iter()
            .nth(1)
            .unwrap();
        let text: String = line
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            !actionable,
            "unmet-pos row must report as non-actionable, got: {text:?}"
        );
        assert!(
            !text.contains("will deselect"),
            "non-toggleable hovered row must not show the warning, got: {text:?}"
        );

        // Incompatible rows are now hidden from the TUI by
        // `current_level_desc` — they don't even enter the visible list, so
        // `will deselect` is impossible to render. This is the generalised
        // replacement for the old "chip-mismatch shown as a silent, greyed-out
        // row" behaviour: `compatible: { chip: [esp32c6] }` with an `esp32`
        // active in the chip selection group filters the row out outright.
        let mut wrong_chip_compat = IndexMap::new();
        wrong_chip_compat.insert("chip".to_string(), vec!["esp32c6".to_string()]);
        let options = vec![
            // Stand-in for the runtime chip selector category.
            chip_group_option(Chip::Esp32),
            option("method", &[]),
            GeneratorOptionItem::Option(GeneratorOption {
                name: "wrong-chip".to_string(),
                display_name: "wrong-chip".to_string(),
                selection_group: String::new(),
                help: String::new(),
                requires: vec!["!method".to_string()],
                compatible: wrong_chip_compat,
            }),
        ];
        let repository = Repository::new(
            Chip::Esp32,
            options,
            &["esp32".to_string(), "method".to_string()],
        );
        let rows = repository.current_level_desc(80, &ui, Some(0));
        assert!(
            rows.iter()
                .all(|(_, line)| !line.spans.iter().any(|s| s.content.contains("wrong-chip"))),
            "incompatible row must be hidden from current_level_desc, got rows: {:?}",
            rows.iter()
                .map(|(_, l)| l
                    .spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            rows.len(),
            2,
            "expected the chip-group option and `method` to remain visible"
        );
    }

    #[test]
    fn force_deselect_trailer_collapses_to_count_when_too_wide() {
        // With several long-named options being evicted and a narrow row, the
        // detailed list must collapse to "will deselect N" rather than overflow /
        // truncate the text.
        let options = vec![
            option("m", &[]),
            option("loooooong-one", &["!m"]),
            option("loooooong-two", &["!m"]),
            option("loooooong-three", &["!m"]),
        ];

        let repository = Repository::new(
            Chip::Esp32,
            options,
            &[
                "loooooong-one".to_string(),
                "loooooong-two".to_string(),
                "loooooong-three".to_string(),
            ],
        );

        let ui = plain_ui();

        // Narrow width — the detailed trailer won't fit.
        let narrow = repository
            .current_level_desc(40, &ui, Some(0))
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        let narrow_text: String = narrow[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            narrow_text.contains("will deselect 3"),
            "expected count fallback, got: {narrow_text:?}"
        );
        assert!(
            !narrow_text.contains("will deselect:"),
            "detailed list must not be present when it doesn't fit, got: {narrow_text:?}"
        );

        // Wide enough — the detailed list must be shown.
        let wide = repository
            .current_level_desc(200, &ui, Some(0))
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        let wide_text: String = wide[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            wide_text.contains("will deselect: loooooong-one, loooooong-two, loooooong-three"),
            "expected detailed trailer, got: {wide_text:?}"
        );
    }

    #[test]
    fn set_options_preserves_survivors_drops_missing_and_cascades() {
        // Rebuilding the options tree (e.g. on chip switch) must:
        //   * keep selections whose option name survives in the new tree,
        //   * drop selections whose option was removed (remap-by-name),
        //   * cascade out anything whose requirements are now unmet,
        //   * trim `path` to whatever still resolves in the new tree.
        let initial = vec![
            GeneratorOptionItem::Category(esp_generate::template::GeneratorOptionCategory {
                name: "cat".to_string(),
                display_name: "cat".to_string(),
                help: String::new(),
                requires: Vec::new(),
                options: vec![
                    option("survivor", &[]),
                    option("will-vanish", &[]),
                    option("dependent", &["will-vanish"]),
                ],
            }),
        ];

        let mut repository = Repository::new(
            Chip::Esp32,
            initial,
            &[
                "survivor".to_string(),
                "will-vanish".to_string(),
                "dependent".to_string(),
            ],
        );
        // Simulate the user having entered the `cat` category.
        repository.enter_group(0);
        assert_eq!(repository.path.len(), 1);

        // New tree for a "different chip" — `will-vanish` is gone; the category
        // itself is preserved so the path stays valid. `dependent` has its
        // required option removed so it must cascade out.
        let rebuilt = vec![
            GeneratorOptionItem::Category(esp_generate::template::GeneratorOptionCategory {
                name: "cat".to_string(),
                display_name: "cat".to_string(),
                help: String::new(),
                requires: Vec::new(),
                options: vec![option("survivor", &[]), option("dependent", &["will-vanish"])],
            }),
        ];

        repository.set_options(Chip::Esp32c6, rebuilt);

        assert_eq!(repository.config.chip, Chip::Esp32c6);
        assert!(repository.config.is_selected("survivor"));
        // Name no longer exists in the new tree: remap-by-name drops it.
        assert!(!repository.config.is_selected("will-vanish"));
        // Present but requirement unmet: cascade evicts it.
        assert!(!repository.config.is_selected("dependent"));
        // `cat` category still exists at the same index → path is preserved.
        assert_eq!(repository.path.len(), 1);

        // Now rebuild with the category itself gone: path must be trimmed.
        let reshaped = vec![option("survivor", &[])];
        repository.set_options(Chip::Esp32c6, reshaped);
        assert_eq!(repository.path.len(), 0);
        assert!(repository.config.is_selected("survivor"));
    }

    #[test]
    fn selected_chip_reports_chip_group_selection() {
        // `selected_chip` is the bridge the main loop uses to notice user-driven
        // chip switches. It must return the chip whose group entry is currently
        // ticked, regardless of what `config.chip` says (divergence is the
        // whole point — that's how we detect a pending switch), and `None`
        // when no chip-group option is present in the tree at all.
        let options = vec![
            chip_group_option(Chip::Esp32),
            chip_group_option(Chip::Esp32c6),
            option("alloc", &[]),
        ];

        let repository = Repository::new(
            Chip::Esp32,
            options,
            &["esp32c6".to_string(), "alloc".to_string()],
        );

        // config.chip still says Esp32 (the tree hasn't been rebuilt yet) but
        // the user has ticked `esp32c6` in the chip group — the main loop
        // picks this up and triggers the rebuild.
        assert_eq!(repository.config.chip, Chip::Esp32);
        assert_eq!(repository.selected_chip(), Some(Chip::Esp32c6));

        // A tree without a chip category means no chip group selection —
        // falls back to `None`, which the main loop maps to `config.chip`.
        let no_chip_group = Repository::new(Chip::Esp32, vec![option("alloc", &[])], &[]);
        assert_eq!(no_chip_group.selected_chip(), None);
    }

    #[test]
    fn toggle_chip_group_is_a_radio_not_a_toggle() {
        // The chip selection group is a *required* radio: clicking the
        // currently-selected chip must be a no-op (there is no "no chip"
        // state the options tree can be in). Clicking a different chip
        // swaps via the normal selection_group mutex in `select_idx`.
        let options = vec![
            chip_group_option(Chip::Esp32),
            chip_group_option(Chip::Esp32c6),
        ];
        let mut repository =
            Repository::new(Chip::Esp32, options, &["esp32".to_string()]);

        repository.toggle_current(0);
        assert!(
            repository.config.is_selected("esp32"),
            "clicking the already-selected chip must be a no-op"
        );

        repository.toggle_current(1);
        assert!(
            repository.config.is_selected("esp32c6"),
            "clicking a different chip must swap the selection"
        );
        assert!(
            !repository.config.is_selected("esp32"),
            "same-group mutex must deselect the previous chip"
        );
    }

    #[test]
    fn chip_switch_preview_lists_incompatible_options() {
        // Hovering a different chip must preview the full blast radius of
        // the impending switch, not just the same-group eviction of the
        // current chip entry. Anything whose `chips` filter excludes the
        // target chip would be removed from the tree by
        // `remove_incompatible_chip_options` on rebuild — from the user's
        // perspective that's a force-deselect, so it belongs in the
        // "will deselect" trailer.
        let options = vec![
            chip_group_option(Chip::Esp32),
            chip_group_option(Chip::Esp32c6),
            // Chip-specific options selected on the current chip.
            option_for_chips("wroom", &[], &[Chip::Esp32]),
            option_for_chips("another-esp32-only", &[], &[Chip::Esp32]),
            // Chip-agnostic: must NOT appear in the preview.
            option("alloc", &[]),
            // Cascade target: requires `wroom`, which the switch drops, so
            // this must be listed too (indirect fallout).
            option("depends-on-wroom", &["wroom"]),
        ];

        let repository = Repository::new(
            Chip::Esp32,
            options,
            &[
                "esp32".to_string(),
                "wroom".to_string(),
                "another-esp32-only".to_string(),
                "alloc".to_string(),
                "depends-on-wroom".to_string(),
            ],
        );

        // Look up the esp32c6 chip-group option as a plain `GeneratorOption`
        // — `would_force_deselect` takes the unwrapped leaf.
        let esp32c6 = match &repository.current_level()[1] {
            GeneratorOptionItem::Option(o) => o.clone(),
            _ => panic!("expected chip-group option"),
        };

        let evicted = repository.config.would_force_deselect(&esp32c6);

        // Chip-incompatible options appear directly.
        assert!(
            evicted.iter().any(|n| n == "wroom"),
            "expected `wroom` in preview, got: {evicted:?}"
        );
        assert!(
            evicted.iter().any(|n| n == "another-esp32-only"),
            "expected `another-esp32-only` in preview, got: {evicted:?}"
        );
        // The chip-group sibling (the currently selected chip) appears too —
        // that's the same-group mutex kicking in.
        assert!(
            evicted.iter().any(|n| n == "esp32"),
            "expected the old chip entry in preview, got: {evicted:?}"
        );
        // Indirect fallout via cascade.
        assert!(
            evicted.iter().any(|n| n == "depends-on-wroom"),
            "expected cascade victim `depends-on-wroom` in preview, got: {evicted:?}"
        );
        // Chip-agnostic options survive the switch.
        assert!(
            !evicted.iter().any(|n| n == "alloc"),
            "chip-agnostic options must not be listed, got: {evicted:?}"
        );
    }

    #[test]
    fn chip_switch_via_set_options_drops_incompatible_survivors() {
        // Simulates what main.rs does when the user picks a different chip
        // in the TUI: build a fresh tree for the new chip (which drops
        // options tagged for other chips) and feed it through `set_options`.
        //
        // The `chips`-based filter is structural: incompatible options are
        // *removed* from the new tree, so `rebuild_indices` drops them by
        // name on the way through. This is exactly what we want — options
        // the user had selected but that don't exist on the new chip simply
        // vanish from `selected`.
        let initial = vec![
            chip_group_option(Chip::Esp32),
            chip_group_option(Chip::Esp32c6),
            // Only valid on the first chip.
            option_for_chips("only-on-esp32", &[], &[Chip::Esp32]),
            // Valid on both — should survive the switch.
            option_for_chips("shared", &[], &[Chip::Esp32, Chip::Esp32c6]),
        ];

        let mut repository = Repository::new(
            Chip::Esp32,
            initial,
            &[
                "esp32".to_string(),
                "only-on-esp32".to_string(),
                "shared".to_string(),
            ],
        );

        // Rebuild for the new chip as if `build_options_for_chip(Esp32c6, …)`
        // had been called: `only-on-esp32` is gone, the chip group still
        // carries both entries (it's chip-agnostic), and the user has now
        // ticked `esp32c6`.
        let rebuilt = vec![
            chip_group_option(Chip::Esp32),
            chip_group_option(Chip::Esp32c6),
            option_for_chips("shared", &[], &[Chip::Esp32, Chip::Esp32c6]),
        ];
        // Before applying `set_options`, mimic what `toggle_current` would
        // have done: swap the chip-group pick on the *old* tree. This is the
        // state the main loop sees when it reads `selected_chip()` and
        // triggers the rebuild.
        repository.toggle_current(1);
        assert_eq!(repository.selected_chip(), Some(Chip::Esp32c6));

        repository.set_options(Chip::Esp32c6, rebuilt);

        assert_eq!(repository.config.chip, Chip::Esp32c6);
        assert!(repository.config.is_selected("esp32c6"));
        assert!(!repository.config.is_selected("esp32"));
        assert!(repository.config.is_selected("shared"));
        // Chip-filtered out of the tree entirely; rebuild-by-name drops it.
        assert!(!repository.config.is_selected("only-on-esp32"));
        // And `selected_chip()` now agrees with `config.chip` — no more
        // pending switch.
        assert_eq!(repository.selected_chip(), Some(repository.config.chip));
    }
}

impl App {
    pub fn handle_event(&mut self, event: Event) -> Result<AppResult> {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                use KeyCode::*;

                if self.confirm_quit {
                    match key.code {
                        Char('y') | Char('Y') => return Ok(AppResult::Quit),
                        _ => self.confirm_quit = false,
                    }
                    return Ok(AppResult::Continue);
                }

                match key.code {
                    Char('q') => self.confirm_quit = true,
                    Char('s') | Char('S') => {
                        return Ok(AppResult::Save);
                    }
                    Esc => {
                        if self.state.len() == 1 {
                            self.confirm_quit = true;
                        } else {
                            self.repository.up();
                            self.exit_menu();
                        }
                    }
                    Char('h') | Left => {
                        self.repository.up();
                        self.exit_menu();
                    }
                    Char('l') | Char(' ') | Right | Enter => {
                        let selected = self.selected();

                        // While toolchains are still being scanned, ignore selection inside the
                        // `toolchain` category
                        if self.toolchains_loading && self.repository.is_in_category("toolchain") {
                            return Ok(AppResult::Continue);
                        }

                        if self.repository.is_option(selected) {
                            self.repository.toggle_current(selected);
                        } else if !self.repository.visible_item(selected).options().is_empty() {
                            self.repository.enter_group(self.selected());
                            self.enter_menu();
                        }
                    }
                    Char('j') | Down => self.select_next(),
                    Char('k') | Up => self.select_previous(),
                    _ => {}
                }
            }
        }

        Ok(AppResult::Continue)
    }

    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
        terminal.draw(|f| {
            f.render_widget(self, f.area());
        })?;

        Ok(())
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(self.help_lines(area)),
            Constraint::Length(self.footer_lines(area)),
        ]);
        let [header_area, rest_area, help_area, footer_area] = vertical.areas(area);

        self.render_title(header_area, buf);
        self.render_item(rest_area, buf);
        self.render_help(help_area, buf);
        self.render_footer(footer_area, buf);
    }
}

impl App {
    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        let mut title = String::from("esp-generate");

        if let Some(tc) = self.repository.selected_toolchain() {
            use std::fmt::Write;
            let _ = write!(&mut title, " | Toolchain: {tc}");
        } else {
            use std::fmt::Write;
            let _ = write!(&mut title, " | Toolchain: template defaults");
        }

        Paragraph::new(title)
            .bold()
            .centered()
            .fg(self.colors.text_color)
            .bg(self.colors.app_background)
            .render(area, buf);
    }

    fn render_item(&mut self, area: Rect, buf: &mut Buffer) {
        // We create two blocks, one is for the header (outer) and the other is for the
        // list (inner).
        let outer_block = Block::default()
            .borders(Borders::NONE)
            .fg(self.colors.text_color)
            .bg(self.colors.header_bg)
            .title_alignment(Alignment::Center);
        let inner_block = Block::default()
            .borders(Borders::NONE)
            .fg(self.colors.text_color)
            .bg(self.colors.normal_row_color);

        // We get the inner area from outer_block. We'll use this area later to render
        // the table.
        let outer_area = area;
        let inner_area = outer_block.inner(outer_area);

        // We can render the header in outer_area.
        outer_block.render(outer_area, buf);

        // The hovered index is what ratatui's ListState reports as selected; rows
        // build their "will deselect …" trailer only for this row so the list stays
        // quiet otherwise.
        let hovered = self.state.last().and_then(|s| s.selected());

        let items: Vec<ListItem> = self
            .repository
            .current_level_desc(area.width, &self.ui_elements, hovered)
            .into_iter()
            .map(|(enabled, line)| {
                ListItem::new(line).style(if enabled {
                    Style::default()
                } else {
                    Style::default().fg(self.colors.disabled_style_fg)
                })
            })
            .collect();

        // We can now render the item list
        // (look carefully, we are using StatefulWidget's render.)
        // ratatui::widgets::StatefulWidget::render as stateful_render
        if let Some(current_state) = self.state.last_mut() {
            // Create a List from all list items and highlight the currently selected one

            let current_item_active = if items.is_empty() {
                false
            } else if let Some(idx) = current_state.selected() {
                let row = idx.min(items.len() - 1);
                let current = self.repository.visible_item(row);
                self.repository.current_level_is_active()
                    && self.repository.is_item_actionable(current)
            } else {
                false
            };

            let items = List::new(items)
                .block(inner_block)
                .highlight_style(if current_item_active {
                    self.colors.selected_active_style
                } else {
                    self.colors.selected_inactive_style
                })
                .highlight_spacing(HighlightSpacing::Always);
            StatefulWidget::render(items, inner_area, buf, current_state);
        } else {
            ratatui::restore();
            panic!("menu state not found!")
        }
    }

    fn help_paragraph(&self) -> Option<Paragraph<'_>> {
        let visible_count = self.repository.visible_count();
        if visible_count == 0 {
            return None;
        }
        let selected = self.selected().min(visible_count - 1);
        let option = self.repository.visible_item(selected);

        let relationships = self.repository.config.collect_relationships(option);

        // `disabled_by` used to explain why an option could not be toggled; now that
        // every option with its positive requirements satisfied is selectable (and the
        // right-side row trailer lists what a selection would kick out), surfacing it
        // here would be redundant and confusing.
        let Relationships {
            requires,
            required_by,
            disabled_by: _,
        } = relationships;

        let help_text = option.help();
        let help_text = append_list_as_sentence(help_text, "Requires", &requires);
        let help_text = append_list_as_sentence(&help_text, "Required by", &required_by);

        if help_text.is_empty() {
            return None;
        }

        let help_block = Block::default()
            .borders(Borders::NONE)
            .fg(self.colors.text_color)
            .bg(self.colors.help_row_color);

        Some(
            Paragraph::new(help_text)
                .centered()
                .wrap(Wrap { trim: false })
                .block(help_block),
        )
    }

    fn help_lines(&self, area: Rect) -> u16 {
        if let Some(paragraph) = self.help_paragraph() {
            paragraph.line_count(area.width) as u16
        } else {
            0
        }
    }

    fn render_help(&self, area: Rect, buf: &mut Buffer) {
        if let Some(paragraph) = self.help_paragraph() {
            paragraph.render(area, buf);
        }
    }

    fn footer_paragraph(&self) -> Paragraph<'_> {
        let text = if self.confirm_quit {
            "Are you sure you want to quit? (y/N)"
        } else {
            "Use ↓↑ to move, ESC/← to go up, → to go deeper or change the value, s/S to save and generate, ESC/q to cancel"
        };

        let text = if self.toolchains_loading {
            format!("{text}  |  Scanning installed toolchains…")
        } else {
            text.to_string()
        };

        Paragraph::new(text)
            .centered()
            .fg(self.colors.text_color)
            .bg(self.colors.app_background)
            .wrap(Wrap { trim: false })
    }

    fn footer_lines(&self, area: Rect) -> u16 {
        self.footer_paragraph().line_count(area.width) as u16
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        self.footer_paragraph().render(area, buf);
    }
}
