use anyhow::Result;
use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use env_logger::{Builder, Env, Logger};
use esp_generate::{
    append_list_as_sentence,
    config::{ActiveConfiguration, Relationships, flatten_options},
    template::GeneratorOptionItem,
};
use esp_metadata::Chip;
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

    fn enter_group(&mut self, index: usize) {
        self.path.push(index);
    }

    fn toggle_current(&mut self, index: usize) {
        if !self.current_level_is_active() {
            return;
        }

        let GeneratorOptionItem::Option(ref option) = self.current_level()[index] else {
            ratatui::restore();
            unreachable!();
        };

        let option_name = option.name.clone();

        if let Some(i) = self
            .config
            .selected
            .iter()
            .position(|s| self.config.flat_options[*s].name == option_name)
        {
            let idx = self.config.selected[i];
            self.config.deselect_idx(idx);
        } else if self.config.is_option_toggleable(option) {
            self.config.select(&option_name);
        }
    }

    fn is_option(&self, index: usize) -> bool {
        matches!(self.current_level()[index], GeneratorOptionItem::Option(_))
    }

    fn up(&mut self) {
        self.path.pop();
    }

    /// Builds the visible list rows.
    ///
    /// `hovered` is the index of the currently highlighted row (what the cursor is
    /// on). Only that row gets the "will disable: …" warning trailer (styled yellow)
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

        level
            .iter()
            .enumerate()
            .map(|(idx, v)| {
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
                // hovered row additionally appends a yellow " - will disable: …"
                // clause when selecting it would force-deselect something. When the
                // detailed list wouldn't fit, the clause degrades to
                // " - will disable N" (count) before any actual truncation kicks in.
                let name_part: String = match v {
                    GeneratorOptionItem::Option(_) => v.name().to_string(),
                    GeneratorOptionItem::Category(_) => String::new(),
                };

                // reserve indicator spacing; saturating_sub keeps padding non-negative so narrow widths don't overflow
                let padding = (width as usize).saturating_sub(v.title().len() + 4);

                let is_hovered = hovered == Some(idx);
                // Symmetric: whether the row is selected or not, toggling it may
                // force-deselect others — always ask the config what would happen.
                let warning_part: Option<String> = match v {
                    GeneratorOptionItem::Option(option) if is_hovered && level_active => {
                        let evicted = self.config.would_force_deselect(option);
                        if evicted.is_empty() {
                            None
                        } else {
                            let detailed = format!(" - will disable: {}", evicted.join(", "));
                            let budget = padding.saturating_sub(name_part.len());
                            if detailed.len() <= budget {
                                Some(detailed)
                            } else {
                                Some(format!(" - will disable {}", evicted.len()))
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

                (
                    level_active && self.is_item_actionable(v),
                    Line::from(spans),
                )
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
    /// Style applied to the " - will disable …" hover clause.
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
    use esp_generate::template::{GeneratorOption, GeneratorOptionItem};
    use esp_metadata::Chip;

    fn option(name: &str, requires: &[&str]) -> GeneratorOptionItem {
        GeneratorOptionItem::Option(GeneratorOption {
            name: name.to_string(),
            display_name: name.to_string(),
            selection_group: String::new(),
            help: String::new(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            chips: Vec::new(),
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
        // The "will disable …" trailer must:
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
                && method_hover.contains("- will disable: dependent"),
            "expected deselect-side warning on selected hovered row, got: {method_hover:?}"
        );

        // Select-side: hovering unselected `method-unselected-a` warns that
        // selecting it would kick out `method` (and, via cascade, `dependent`).
        let unselected_hover = row_text(1, Some(1));
        assert!(
            unselected_hover.contains("method-unselected-a")
                && unselected_hover.contains("- will disable:")
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
            .find(|s| s.content.contains("will disable"))
            .expect("warning span");
        assert!(!warning_span.content.contains("method-unselected-a "));

        // Hovering a row whose toggle is non-destructive means nothing on any
        // row can carry the warning.
        for idx in 0..3 {
            let text = row_text(idx, Some(2 /* `dependent` — non-destructive */));
            assert!(
                !text.contains("will disable"),
                "no row must show the warning when hover is non-destructive, got: {text:?}"
            );
        }
    }

    #[test]
    fn force_deselect_trailer_collapses_to_count_when_too_wide() {
        // With several long-named options being evicted and a narrow row, the
        // detailed list must collapse to "will disable N" rather than overflow /
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
            narrow_text.contains("will disable 3"),
            "expected count fallback, got: {narrow_text:?}"
        );
        assert!(
            !narrow_text.contains("will disable:"),
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
            wide_text.contains("will disable: loooooong-one, loooooong-two, loooooong-three"),
            "expected detailed trailer, got: {wide_text:?}"
        );
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
                        } else if !self.repository.current_level()[selected]
                            .options()
                            .is_empty()
                        {
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
        // build their "will disable …" trailer only for this row so the list stays
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

            let current_item_active = if let Some(current) =
                current_state.selected().and_then(|idx| {
                    self.repository
                        .current_level()
                        .get(idx.min(items.len() - 1))
                }) {
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
        let selected = self
            .selected()
            .min(self.repository.current_level().len() - 1);
        let option = &self.repository.current_level()[selected];

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
