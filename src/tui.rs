use std::{error::Error, io};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use esp_metadata::Chip;
use ratatui::{prelude::*, style::palette::tailwind, widgets::*};

use super::{GeneratorOption, GeneratorOptionItem};

const TODO_HEADER_BG: Color = tailwind::BLUE.c950;
const NORMAL_ROW_COLOR: Color = tailwind::SLATE.c950;
const SELECTED_STYLE_FG: Color = tailwind::BLUE.c300;
const DISABLED_STYLE_FG: Color = tailwind::GRAY.c600;
const TEXT_COLOR: Color = tailwind::SLATE.c200;

type AppResult<T> = Result<T, Box<dyn Error>>;

pub struct Repository {
    chip: Chip,
    options: &'static [GeneratorOptionItem],
    path: Vec<usize>,
    selected: Vec<String>,
}

impl Repository {
    pub fn new(chip: Chip, options: &'static [GeneratorOptionItem], selected: &[String]) -> Self {
        Self {
            chip,
            options,
            path: Vec::new(),
            selected: Vec::from(selected),
        }
    }

    fn current_level(&self) -> Vec<GeneratorOptionItem> {
        let mut current = self.options;

        for &index in &self.path {
            current = match current[index] {
                GeneratorOptionItem::Category(category) => category.options,
                GeneratorOptionItem::Option(_) => unreachable!(),
            }
        }

        Vec::from(current)
    }

    fn select(&mut self, index: usize) {
        self.path.push(index);
    }

    fn toggle_current(&mut self, index: usize) {
        let current = self.current_level()[index];
        match current {
            GeneratorOptionItem::Category(_) => unreachable!(),
            GeneratorOptionItem::Option(option) => {
                if !option.chips.is_empty() && !option.chips.contains(&self.chip) {
                    return;
                }

                if let Some(i) = self.selected.iter().position(|v| v == option.name) {
                    self.selected.remove(i);
                } else {
                    self.selected.push(option.name.to_string());
                }

                let currently_selected = self.selected.clone();
                for option in currently_selected {
                    let option = find_option(option, self.options).unwrap();
                    for enable in option.enables {
                        if !self.selected.contains(&enable.to_string()) {
                            self.selected.push(enable.to_string());
                        }
                    }
                    for disable in option.disables {
                        if self.selected.contains(&disable.to_string()) {
                            let idx = self.selected.iter().position(|v| v == disable).unwrap();
                            self.selected.remove(idx);
                        }
                    }
                }
            }
        }
    }

    fn is_option(&self, index: usize) -> bool {
        matches!(self.current_level()[index], GeneratorOptionItem::Option(_))
    }

    fn up(&mut self) {
        self.path.pop();
    }

    fn count(&self) -> usize {
        self.current_level().len()
    }

    fn current_level_desc(&self) -> Vec<(bool, String)> {
        self.current_level()
            .iter()
            .map(|v| {
                (
                    v.chips().is_empty() || v.chips().contains(&self.chip),
                    format!(
                        " {} {}",
                        if self.selected.contains(&v.name()) {
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

fn find_option(
    option: String,
    options: &'static [GeneratorOptionItem],
) -> Option<&'static GeneratorOption> {
    for item in options {
        match item {
            GeneratorOptionItem::Category(category) => {
                return find_option(option, category.options)
            }
            GeneratorOptionItem::Option(item) => {
                if item.name == option {
                    return Some(item);
                }
            }
        }
    }
    None
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
    state: ListState,
    repository: Repository,
}

impl App {
    pub fn new(repository: Repository) -> Self {
        let mut initial_state = ListState::default();
        initial_state.select(Some(0));

        Self {
            repository,
            state: initial_state,
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

                    match key.code {
                        Char('q') | Esc => return Ok(None),
                        Char('s') => return Ok(Some(self.repository.selected.clone())),
                        Char('h') | Left => {
                            self.repository.up();
                            self.state.select(Some(0));
                        }
                        Char('l') | Right | Enter => {
                            let selected = self.state.selected().unwrap_or_default();
                            if self.repository.is_option(selected) {
                                self.repository.toggle_current(selected);
                            } else {
                                self.repository
                                    .select(self.state.selected().unwrap_or_default());
                                self.state.select(Some(0));
                            }
                        }
                        Char('j') | Down => {
                            if self.state.selected().unwrap_or_default()
                                < self.repository.count() - 1
                            {
                                self.state
                                    .select(Some(self.state.selected().unwrap_or_default() + 1));
                            }
                        }
                        Char('k') | Up => {
                            if self.state.selected().unwrap_or_default() > 0 {
                                self.state
                                    .select(Some(self.state.selected().unwrap_or_default() - 1));
                            }
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
        // Create a space for header, todo list and the footer.
        let vertical = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(2),
        ]);
        let [header_area, rest_area, footer_area] = vertical.areas(area);

        // Create two chunks with equal vertical screen space. One for the list and the
        // other for the info block.
        let vertical = Layout::vertical([Constraint::Percentage(100)]);
        let [upper_item_list_area] = vertical.areas(rest_area);

        self.render_title(header_area, buf);
        self.render_item(upper_item_list_area, buf);
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
            .map(|v| {
                ListItem::new(v.1).style(if v.0 {
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
        StatefulWidget::render(items, inner_area, buf, &mut self.state);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(
            "\nUse ↓↑ to move, ← to go up, → to go deeper or change the value, s/S to save and generate, ESC/q to cancel"
        ).centered()
        .render(area, buf);
    }
}
