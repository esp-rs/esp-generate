use std::{error::Error, io};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use esp_generate::{
    append_list_as_sentence,
    config::{find_option, ActiveConfiguration},
    template::GeneratorOptionItem,
};
use esp_metadata::Chip;
use ratatui::{prelude::*, style::palette::tailwind, widgets::*};

const TODO_HEADER_BG: Color = tailwind::BLUE.c950;
const NORMAL_ROW_COLOR: Color = tailwind::SLATE.c950;
const HELP_ROW_COLOR: Color = tailwind::SLATE.c800;
const SELECTED_STYLE_FG: Color = tailwind::BLUE.c300;
const DISABLED_STYLE_FG: Color = tailwind::GRAY.c600;
const TEXT_COLOR: Color = tailwind::SLATE.c200;

type AppResult<T> = Result<T, Box<dyn Error>>;

pub struct Repository {
    config: ActiveConfiguration<'static>,
    path: Vec<usize>,
}

impl Repository {
    pub fn new(chip: Chip, options: &'static [GeneratorOptionItem], selected: &[String]) -> Self {
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

    fn enter_group(&mut self, index: usize) {
        self.path.push(index);
    }

    fn toggle_current(&mut self, index: usize) {
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
        } else if self.config.requirements_met(option) {
            self.config.select(option.name.clone());
        }
    }

    fn is_option(&self, index: usize) -> bool {
        matches!(self.current_level()[index], GeneratorOptionItem::Option(_))
    }

    fn up(&mut self) {
        self.path.pop();
    }

    fn current_level_desc(&self) -> Vec<(bool, String)> {
        let level = self.current_level();

        level
            .iter()
            .map(|v| {
                (
                    self.config.is_active(v),
                    format!(
                        " {} {}",
                        if self.config.selected.iter().any(|o| o == v.name()) {
                            "✅"
                        } else if v.is_category() {
                            "▶️"
                        } else {
                            "  "
                        },
                        v.title(),
                    ),
                )
            })
            .collect()
    }
}

pub fn init_terminal() -> AppResult<Terminal<impl Backend>> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal() -> AppResult<()> {
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

pub struct App {
    state: Vec<ListState>,
    repository: Repository,
    confirm_quit: bool,
}

impl App {
    pub fn new(repository: Repository) -> Self {
        let mut initial_state = ListState::default();
        initial_state.select(Some(0));

        Self {
            repository,
            state: vec![initial_state],
            confirm_quit: false,
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

impl App {
    pub fn run(&mut self, mut terminal: Terminal<impl Backend>) -> AppResult<Option<Vec<String>>> {
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
                            return Ok(Some(self.repository.config.selected.clone()))
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

    fn draw(&mut self, terminal: &mut Terminal<impl Backend>) -> AppResult<()> {
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
        Paragraph::new("esp-generate")
            .bold()
            .centered()
            .render(area, buf);
    }

    fn render_item(&mut self, area: Rect, buf: &mut Buffer) {
        // We create two blocks, one is for the header (outer) and the other is for the
        // list (inner).
        let outer_block = Block::default()
            .borders(Borders::NONE)
            .fg(TEXT_COLOR)
            .bg(TODO_HEADER_BG)
            .title_alignment(Alignment::Center);
        let inner_block = Block::default()
            .borders(Borders::NONE)
            .fg(TEXT_COLOR)
            .bg(NORMAL_ROW_COLOR);

        // We get the inner area from outer_block. We'll use this area later to render
        // the table.
        let outer_area = area;
        let inner_area = outer_block.inner(outer_area);

        // We can render the header in outer_area.
        outer_block.render(outer_area, buf);

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
            .repository
            .current_level_desc()
            .into_iter()
            .map(|(enabled, value)| {
                ListItem::new(value).style(if enabled {
                    Style::default()
                } else {
                    Style::default().fg(DISABLED_STYLE_FG)
                })
            })
            .collect();

        // Create a List from all list items and highlight the currently selected one
        let items = List::new(items)
            .block(inner_block)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED)
                    .fg(SELECTED_STYLE_FG),
            )
            .highlight_spacing(HighlightSpacing::Always);

        // We can now render the item list
        // (look carefully, we are using StatefulWidget's render.)
        // ratatui::widgets::StatefulWidget::render as stateful_render
        if let Some(current_state) = self.state.last_mut() {
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

        let mut requires = Vec::new();
        let mut required_by = Vec::new();
        let mut disabled_by = Vec::new();

        self.repository.config.selected.iter().for_each(|opt| {
            let opt = find_option(opt.as_str(), self.repository.config.options).unwrap();
            for o in opt.requires.iter() {
                if let Some(disables) = o.strip_prefix("!") {
                    if disables == option.name() {
                        disabled_by.push(opt.name.as_str());
                    }
                } else if o == option.name() {
                    required_by.push(o.as_str());
                }
            }
        });
        for req in option.requires() {
            if let Some(disables) = req.strip_prefix("!") {
                if self.repository.config.is_selected(disables) {
                    disabled_by.push(disables);
                }
            } else {
                requires.push(req);
            }
        }

        let help_text = option.help();
        let help_text = append_list_as_sentence(help_text, "Requires", &requires);
        let help_text = append_list_as_sentence(&help_text, "Required by", &required_by);
        let help_text = append_list_as_sentence(&help_text, "Disabled by", &disabled_by);

        if help_text.is_empty() {
            return None;
        }

        let help_block = Block::default()
            .borders(Borders::NONE)
            .fg(TEXT_COLOR)
            .bg(HELP_ROW_COLOR);

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

        Paragraph::new(text).centered().wrap(Wrap { trim: false })
    }

    fn footer_lines(&self, area: Rect) -> u16 {
        self.footer_paragraph().line_count(area.width) as u16
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        self.footer_paragraph().render(area, buf);
    }
}
