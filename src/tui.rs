use anyhow::Result;
use std::io;

use esp_generate::{
    append_list_as_sentence,
    config::{ActiveConfiguration, Relationships},
    template::GeneratorOptionItem,
};
use esp_metadata::Chip;
use ratatui::crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, style::palette::tailwind, widgets::*};

pub struct Repository<'app> {
    config: ActiveConfiguration<'app>,
    path: Vec<usize>,
}

impl<'app> Repository<'app> {
    pub fn new(chip: Chip, options: &'app [GeneratorOptionItem], selected: &[String]) -> Self {
        Self {
            config: ActiveConfiguration {
                chip,
                selected: Vec::from(selected),
                options,
            },
            path: Vec::new(),
        }
    }

    fn current_level(&self) -> &[GeneratorOptionItem] {
        let mut current = self.config.options;

        for &index in &self.path {
            current = match &current[index] {
                GeneratorOptionItem::Category(category) => category.options.as_slice(),
                GeneratorOptionItem::Option(_) => unreachable!(),
            }
        }

        current
    }

    fn current_level_is_active(&self) -> bool {
        let mut current = self.config.options;

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

    fn enter_group(&mut self, index: usize) {
        self.path.push(index);
    }

    fn toggle_current(&mut self, index: usize) {
        if !self.current_level_is_active() {
            return;
        }
        if !self.config.is_active(&self.current_level()[index]) {
            return;
        }

        let GeneratorOptionItem::Option(ref option) = self.current_level()[index] else {
            ratatui::restore();
            unreachable!();
        };

        if let Some(i) = self.config.selected_index(&option.name) {
            if self.config.can_be_disabled(&option.name) {
                self.config.selected.swap_remove(i);
            }
        } else {
            self.config.select(option.name.clone());
        }
    }

    fn is_option(&self, index: usize) -> bool {
        matches!(self.current_level()[index], GeneratorOptionItem::Option(_))
    }

    fn up(&mut self) {
        self.path.pop();
    }

    fn current_level_desc(&self, width: u16, style: &UiElements) -> Vec<(bool, String)> {
        let level = self.current_level();
        let level_active = self.current_level_is_active();

        level
            .iter()
            .map(|v| {
                let name = if let GeneratorOptionItem::Option(_) = v {
                    v.name()
                } else {
                    ""
                };
                let indicator =
                    if self.config.selected.iter().any(|o| o == v.name()) && level_active {
                        style.selected
                    } else if v.is_category() {
                        style.category
                    } else {
                        style.unselected
                    };
                // reserve indicator spacing; saturating_sub keeps padding non-negative so narrow widths don't overflow
                let padding = (width as usize).saturating_sub(v.title().len() + 4);
                (
                    level_active && self.config.is_active(v),
                    format!(
                        " {} {}{:>padding$}",
                        indicator,
                        v.title(),
                        name,
                        padding = padding,
                    ),
                )
            })
            .collect()
    }
}

pub fn init_terminal() -> Result<Terminal<impl Backend>> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

struct UiElements {
    selected: &'static str,
    unselected: &'static str,
    category: &'static str,
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
    };
    const FALLBACK: Self = Self {
        selected: "*",
        unselected: " ",
        category: ">",
    };
}

pub struct App<'app> {
    state: Vec<ListState>,
    repository: Repository<'app>,
    confirm_quit: bool,
    ui_elements: UiElements,
    colors: Colors,
}

impl<'app> App<'app> {
    pub fn new(repository: Repository<'app>) -> Self {
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
}

impl App<'_> {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> Result<Option<Vec<String>>> {
        loop {
            self.draw(&mut terminal)?;

            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    use KeyCode::*;

                    if self.confirm_quit {
                        match key.code {
                            Char('y') | Char('Y') => return Ok(None),
                            _ => self.confirm_quit = false,
                        }
                        continue;
                    }

                    match key.code {
                        Char('q') => self.confirm_quit = true,
                        Char('s') | Char('S') => {
                            return Ok(Some(self.repository.config.selected.clone()));
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
                            if self.repository.is_option(selected) {
                                self.repository.toggle_current(selected);
                            } else {
                                self.repository.enter_group(self.selected());
                                self.enter_menu();
                            }
                        }
                        Char('j') | Down => {
                            self.select_next();
                        }
                        Char('k') | Up => {
                            self.select_previous();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<()> {
        terminal.draw(|f| {
            f.render_widget(self, f.area());
        })?;

        Ok(())
    }
}

impl Widget for &mut App<'_> {
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

impl App<'_> {
    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("esp-generate")
            .bold()
            .centered()
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

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
            .repository
            .current_level_desc(area.width, &self.ui_elements)
            .into_iter()
            .map(|(enabled, value)| {
                ListItem::new(value).style(if enabled {
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
                    && self.repository.config.is_active(current)
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

        let Relationships {
            requires,
            required_by,
            disabled_by,
        } = self.repository.config.collect_relationships(option);

        let help_text = option.help();
        let help_text = append_list_as_sentence(help_text, "Requires", &requires);
        let help_text = append_list_as_sentence(&help_text, "Required by", &required_by);
        let help_text = append_list_as_sentence(&help_text, "Disabled by", &disabled_by);

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

        Paragraph::new(text)
            .centered()
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
