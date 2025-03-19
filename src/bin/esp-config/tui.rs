use std::{error::Error, io};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::*, style::palette::tailwind, widgets::*};
use tui_textarea::{CursorMove, TextArea};

const TODO_HEADER_BG: Color = tailwind::BLUE.c950;
const NORMAL_ROW_COLOR: Color = tailwind::SLATE.c950;
const HELP_ROW_COLOR: Color = tailwind::SLATE.c800;
const TEXT_COLOR: Color = tailwind::SLATE.c200;

const SELECTED_ACTIVE_BACKGROUND: Color = tailwind::BLUE.c950;

const SELECTED_ACTIVE_STYLE: Style = Style::new()
    .add_modifier(Modifier::BOLD)
    .fg(TEXT_COLOR)
    .bg(SELECTED_ACTIVE_BACKGROUND);

const EDIT_VALID_STYLE: Style = Style::new().add_modifier(Modifier::BOLD).fg(TEXT_COLOR);

const EDIT_INVALID_STYLE: Style = Style::new().add_modifier(Modifier::BOLD).fg(Color::Red);

const BORDER_STYLE: Style = Style::new()
    .add_modifier(Modifier::BOLD)
    .fg(Color::LightBlue);

const BORDER_ERROR_STYLE: Style = Style::new().add_modifier(Modifier::BOLD).fg(Color::Red);

type AppResult<T> = Result<T, Box<dyn Error>>;

pub struct Repository {
    configs: Vec<crate::CrateConfig>,
    current_crate: Option<usize>,
}

enum Item {
    TopLevel(String),
    CrateLevel(crate::ConfigOption),
}

impl Item {
    fn title(&self, _width: u16) -> String {
        match self {
            Item::TopLevel(crate_name) => crate_name.clone(),
            Item::CrateLevel(config_option) => {
                format!("{} ({})", config_option.name, config_option.actual_value).to_string()
            }
        }
    }

    fn help_text(&self) -> String {
        match self {
            Item::TopLevel(crate_name) => format!("The `{crate_name}` crate").to_string(),
            Item::CrateLevel(config_option) => config_option.description.clone(),
        }
        .replace("<p>", "")
        .replace("</p>", "\n")
        .to_string()
    }

    fn value(&self) -> crate::Value {
        match self {
            Item::TopLevel(_) => unreachable!(),
            Item::CrateLevel(config_option) => config_option.actual_value.clone(),
        }
    }

    fn constraint(&self) -> crate::Constraint {
        match self {
            Item::TopLevel(_) => unreachable!(),
            Item::CrateLevel(config_option) => config_option
                .constraint
                .clone()
                .unwrap_or(crate::Constraint::Other),
        }
    }
}

impl Repository {
    pub fn new(options: Vec<crate::CrateConfig>) -> Self {
        Self {
            configs: options,
            current_crate: None,
        }
    }

    fn current_level(&self) -> Vec<Item> {
        if self.current_crate.is_none() {
            Vec::from_iter(
                self.configs
                    .iter()
                    .map(|config| Item::TopLevel(config.name.clone())),
            )
        } else {
            Vec::from_iter(
                self.configs[self.current_crate.unwrap()]
                    .options
                    .iter()
                    .map(|option| Item::CrateLevel(option.clone())),
            )
        }
    }

    fn enter_group(&mut self, index: usize) {
        if self.current_crate.is_none() {
            self.current_crate = Some(index);
        }
    }

    fn up(&mut self) {
        if self.current_crate.is_some() {
            self.current_crate = None;
        }
    }

    fn set_current(&mut self, index: usize, new_value: crate::Value) {
        if self.current_crate.is_none() {
            return;
        }

        self.configs[self.current_crate.unwrap()].options[index].actual_value = new_value;
    }

    // true if this is a configurable option
    fn is_option(&self, _index: usize) -> bool {
        self.current_crate.is_some()
    }

    // What to show in the list
    fn current_level_desc(&self, width: u16) -> Vec<String> {
        let level = self.current_level();

        level.iter().map(|v| v.title(width)).collect()
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

pub struct App<'a> {
    repository: Repository,

    state: Vec<ListState>,

    confirm_quit: bool,

    editing: bool,
    textarea: TextArea<'a>,
    editing_constraints: Option<crate::Constraint>,
    input_valid: bool,

    showing_selection_popup: bool,
    list_popup: List<'a>,
    list_popup_state: ListState,

    show_initial_message: bool,
    initial_message: Option<String>,
}

impl App<'_> {
    pub fn new(errors_to_show: Option<String>, repository: Repository) -> Self {
        let mut initial_state = ListState::default();
        initial_state.select(Some(0));

        Self {
            repository,
            state: vec![initial_state],
            confirm_quit: false,
            editing: false,
            textarea: TextArea::default(),
            editing_constraints: None,
            input_valid: true,
            showing_selection_popup: false,
            list_popup: List::default(),
            list_popup_state: ListState::default(),
            show_initial_message: errors_to_show.is_some(),
            initial_message: errors_to_show,
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
    pub fn run(
        &mut self,
        mut terminal: Terminal<impl Backend>,
    ) -> AppResult<Option<Vec<crate::CrateConfig>>> {
        loop {
            self.draw(&mut terminal)?;

            if let Event::Key(key) = event::read()? {
                if self.editing {
                    match key.code {
                        KeyCode::Enter if key.kind == KeyEventKind::Press => {
                            if !self.input_valid {
                                continue;
                            }

                            let selected = self.selected();
                            if self.repository.is_option(selected) {
                                let current = self.repository.current_level()[selected].value();
                                let text = self.textarea.lines().join("");

                                self.repository.set_current(
                                    selected,
                                    match current {
                                        crate::Value::Bool(_) => {
                                            crate::Value::Bool(text.parse().unwrap())
                                        }
                                        crate::Value::Integer(_) => {
                                            crate::Value::Integer(text.parse().unwrap())
                                        }
                                        crate::Value::String(_) => crate::Value::String(text),
                                    },
                                );
                            }

                            self.editing = false;
                        }
                        KeyCode::Esc => {
                            self.editing = false;
                        }
                        _ => {
                            if self.textarea.input(key) {
                                if let Some(constraint) = &self.editing_constraints {
                                    let text = self.textarea.lines().join("");

                                    let invalid = match constraint {
                                        crate::Constraint::NegativeInteger => {
                                            let val = text.parse::<i128>().unwrap_or(i128::MAX);
                                            val >= 0
                                        }
                                        crate::Constraint::NonNegativeInteger => {
                                            let val = text.parse::<i128>().unwrap_or(i128::MIN);
                                            val < 0
                                        }
                                        crate::Constraint::PositiveInteger => {
                                            let val = text.parse::<i128>().unwrap_or(0);
                                            val < 1
                                        }
                                        crate::Constraint::IntegerInRange(range) => {
                                            let val = text.parse::<i128>().unwrap_or(i128::MIN);
                                            !range.contains(&val)
                                        }
                                        _ => false,
                                    };

                                    self.textarea.set_style(if invalid {
                                        EDIT_INVALID_STYLE
                                    } else {
                                        EDIT_VALID_STYLE
                                    });
                                    self.input_valid = !invalid;
                                }
                            }
                        }
                    }
                } else if self.showing_selection_popup {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            self.showing_selection_popup = false;
                        }
                        KeyCode::Down | KeyCode::Char('j') => self.list_popup_state.select_next(),
                        KeyCode::Up | KeyCode::Char('k') => self.list_popup_state.select_previous(),
                        KeyCode::Enter if key.kind == KeyEventKind::Press => {
                            let selected = self.selected();
                            if let Some(crate::Constraint::Enumeration(items)) =
                                &self.repository.configs[self.repository.current_crate.unwrap()]
                                    .options[selected]
                                    .constraint
                            {
                                self.repository.set_current(
                                    selected,
                                    crate::Value::String(
                                        items[self.list_popup_state.selected().unwrap()].clone(),
                                    ),
                                );
                            }
                            self.showing_selection_popup = false;
                        }
                        _ => (),
                    }
                } else if self.show_initial_message {
                    match key.code {
                        KeyCode::Enter if key.kind == KeyEventKind::Press => {
                            self.show_initial_message = false;
                        }
                        _ => (),
                    }
                } else if key.kind == KeyEventKind::Press {
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
                        Char('s') | Char('S') => return Ok(Some(self.repository.configs.clone())),
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
                                let current = self.repository.current_level()[selected].value();
                                let constraint =
                                    self.repository.current_level()[selected].constraint();

                                match current {
                                    crate::Value::Bool(value) => self
                                        .repository
                                        .set_current(selected, crate::Value::Bool(!value)),
                                    crate::Value::Integer(value) => {
                                        self.textarea = make_text_area(&format!("{value}"));
                                        self.editing_constraints = Some(constraint);
                                        self.editing = true;
                                    }
                                    crate::Value::String(s) => match constraint {
                                        crate::Constraint::Enumeration(items) => {
                                            let selected_option =
                                                items.iter().position(|v| *v == s);
                                            self.list_popup = make_popup(items);
                                            self.list_popup_state = ListState::default();
                                            self.list_popup_state.select(selected_option);
                                            self.showing_selection_popup = true;
                                        }
                                        _ => {
                                            self.textarea = make_text_area(&s);
                                            self.editing_constraints = None;
                                            self.editing = true;
                                        }
                                    },
                                }
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

fn make_text_area<'a>(s: &str) -> TextArea<'a> {
    let mut text_area = TextArea::new(vec![s.to_string()]);
    text_area.set_block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(BORDER_STYLE)
            .title("Input"),
    );
    text_area.set_style(EDIT_VALID_STYLE);
    text_area.set_cursor_line_style(Style::default());
    text_area.move_cursor(CursorMove::End);
    text_area
}

fn make_popup<'a>(items: Vec<String>) -> List<'a> {
    List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(BORDER_STYLE)
                .title("Choose"),
        )
        .highlight_style(SELECTED_ACTIVE_STYLE)
        .highlight_symbol("▶️ ")
        .repeat_highlight_symbol(true)
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

        if self.editing {
            let area = Rect {
                x: 5,
                y: area.height / 2 - 2,
                width: area.width - 10,
                height: 3,
            };

            ratatui::widgets::Clear.render(area, buf);
            self.textarea.render(area, buf);
        }

        if self.showing_selection_popup {
            let area = Rect {
                x: 5,
                y: area.height / 2 - 3,
                width: area.width - 10,
                height: 6,
            };

            ratatui::widgets::Clear.render(area, buf);
            StatefulWidget::render(&self.list_popup, area, buf, &mut self.list_popup_state);
        }

        if self.show_initial_message {
            let area = Rect {
                x: 5,
                y: area.height / 2 - 5,
                width: area.width - 10,
                height: 5,
            };

            let block = Paragraph::new(self.initial_message.as_ref().unwrap().clone())
                .style(EDIT_INVALID_STYLE)
                .block(
                    Block::bordered()
                        .title("The project generated errors")
                        .style(BORDER_ERROR_STYLE)
                        .padding(Padding::uniform(1)),
                );
            block.render(area, buf);
        }
    }
}

impl App<'_> {
    fn render_title(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("esp-config")
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
            .current_level_desc(area.width)
            .into_iter()
            .map(|value| ListItem::new(value).style(Style::default()))
            .collect();

        // We can now render the item list
        // (look carefully, we are using StatefulWidget's render.)
        // ratatui::widgets::StatefulWidget::render as stateful_render
        if let Some(current_state) = self.state.last_mut() {
            // Create a List from all list items and highlight the currently selected one
            let items = List::new(items)
                .block(inner_block)
                .highlight_style(SELECTED_ACTIVE_STYLE)
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
        let help_text = option.help_text();
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
        } else if self.editing {
            "ENTER to confirm, ESC to cancel"
        } else if self.showing_selection_popup {
            "Use ↓↑ to move, ENTER to confirm, ESC to cancel"
        } else if self.show_initial_message {
            "ENTER to confirm"
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
