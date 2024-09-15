// shitty code, need to modularise it

use arboard::Clipboard;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};
use regex::Regex;
use std::default::Default;
use std::fmt;
use std::{
    process::Command,
    time::{Duration, Instant},
};

use crate::db::{
    add_account, get_current_user, get_ssh_key, init_db, list_accounts, remove_account,
    switch_account, Account as DbAccount,
};

pub struct App {
    running: bool,
    accounts: Vec<DbAccount>,
    active_account_index: Option<usize>,
    current_tab: Tab,
    input: String,
    input_mode: InputMode,
    status_message: String,
    status_time: Instant,
    active_account: Option<usize>,
    popup: PopupType,
    new_account_name: String,
    new_account_email: String,
    clipboard: Clipboard,
    add_account_focus: AddAccountField,
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("accounts", &self.accounts)
            .field("active_account_index", &self.active_account_index)
            .field("current_tab", &self.current_tab)
            .field("input", &self.input)
            .field("input_mode", &self.input_mode)
            .field("status_message", &self.status_message)
            .field("status_time", &self.status_time)
            .field("active_account", &self.active_account)
            .field("popup", &self.popup)
            .field("new_account_name", &self.new_account_name)
            .field("new_account_email", &self.new_account_email)
            .field("add_account_focus", &self.add_account_focus)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Tab {
    Accounts,
    Help,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, PartialEq, Clone)]
enum PopupType {
    AddAccount,
    RemoveConfirmation,
    None,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum AddAccountField {
    Name,
    Email,
}

impl Default for Tab {
    fn default() -> Self {
        Tab::Accounts
    }
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::Normal
    }
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: false,
            accounts: Vec::new(),
            active_account_index: None,
            current_tab: Tab::default(),
            input: String::new(),
            input_mode: InputMode::default(),
            status_message: String::new(),
            status_time: Instant::now(),
            active_account: None,
            popup: PopupType::None,
            new_account_name: String::new(),
            new_account_email: String::new(),
            clipboard: Clipboard::new().expect("Failed to initialize clipboard"),
            add_account_focus: AddAccountField::Name,
        }
    }
}

impl App {
    pub fn new() -> Result<Self> {
        init_db()?;
        let accounts = list_accounts().unwrap_or_default();
        let mut app = Self {
            running: true,
            accounts,
            active_account_index: None,
            current_tab: Tab::default(),
            input: String::new(),
            input_mode: InputMode::default(),
            status_message: String::new(),
            status_time: Instant::now(),
            active_account: None,
            popup: PopupType::None,
            new_account_name: String::new(),
            new_account_email: String::new(),
            clipboard: Clipboard::new().expect("Failed to initialize clipboard"),
            add_account_focus: AddAccountField::Name,
        };
        app.status_message = "Welcome to GitSwitch-Tui!".to_string();

        // Set the active account index based on the current Git user
        if let Ok(Some(current_user)) = get_current_user() {
            if let Some(index) = app
                .accounts
                .iter()
                .position(|a| a.email == current_user.email)
            {
                app.active_account_index = Some(index);
                app.active_account = Some(index);
            }
        }

        Ok(app)
    }

    pub fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<impl ratatui::backend::Backend>,
    ) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.draw(frame))?;

            if let Event::Key(key) = event::read()? {
                self.handle_key_event(key);
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ]
                .as_ref(),
            )
            .split(frame.size());

        self.draw_tabs(frame, chunks[0]);
        self.draw_content(frame, chunks[1]);
        self.draw_status_bar(frame, chunks[2]);

        // Draw popup if there is one
        if self.popup != PopupType::None {
            self.draw_popup(frame);
        }
    }

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let titles = vec!["Accounts", "Help"];
        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("GitSwitch-Tui"),
            )
            .select(self.current_tab as usize)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, area);
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect) {
        match self.current_tab {
            Tab::Accounts => self.draw_accounts(frame, area),
            Tab::Help => self.draw_help(frame, area),
        }
    }

    fn draw_accounts(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(area);

        if self.accounts.is_empty() {
            let message = vec![
                Line::from(Span::styled(
                    "No accounts",
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press 'a' to add a new account",
                    Style::default().fg(Color::Green),
                )),
            ];
            let paragraph = Paragraph::new(message)
                .block(Block::default().borders(Borders::ALL).title("Accounts"))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(paragraph, chunks[0]);
        } else {
            let items: Vec<ListItem> = self
                .accounts
                .iter()
                .enumerate()
                .map(|(index, account)| {
                    let style = if Some(index) == self.active_account {
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(vec![
                        Line::from(Span::styled(&account.name, style)),
                        Line::from(Span::styled(&account.email, style.fg(Color::DarkGray))),
                    ])
                })
                .collect();

            let accounts = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Accounts"))
                .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
                .highlight_symbol(">> ");

            let mut list_state = ListState::default();
            list_state.select(self.active_account_index);
            frame.render_stateful_widget(accounts, chunks[0], &mut list_state);
        }

        if let Some(index) = self.active_account_index {
            let account = &self.accounts[index];
            let info = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(&account.name, Style::default().fg(Color::LightGreen)),
                ]),
                Line::from(vec![
                    Span::styled("Email: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(&account.email, Style::default().fg(Color::LightBlue)),
                ]),
                Line::from(Span::styled(
                    if Some(index) == self.active_account {
                        "● Active"
                    } else {
                        "○ Inactive"
                    },
                    Style::default().fg(if Some(index) == self.active_account {
                        Color::LightGreen
                    } else {
                        Color::Gray
                    }),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Enter: Copy SSH key | Space: Set active",
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(Span::styled(
                    "a: Add new account | r: Remove account",
                    Style::default().fg(Color::Cyan),
                )),
            ])
            .block(Block::default().borders(Borders::ALL).title("Account Info"));
            frame.render_widget(info, chunks[1]);
        } else {
            let info = Paragraph::new(vec![
                Line::from(Span::styled(
                    "No account selected",
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from("Select an account from the list"),
                Line::from("or add a new one with 'a'"),
            ])
            .block(Block::default().borders(Borders::ALL).title("Account Info"))
            .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(info, chunks[1]);
        }
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(Span::styled(
                "Keyboard Shortcuts",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw(": Switch between tabs"),
            ]),
            Line::from(vec![
                Span::styled("↑/↓", Style::default().fg(Color::LightGreen)),
                Span::raw(": Navigate accounts"),
            ]),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::LightYellow)),
                Span::raw(": Copy SSH key to clipboard"),
            ]),
            Line::from(vec![
                Span::styled("Space", Style::default().fg(Color::LightYellow)),
                Span::raw(": Set active account"),
            ]),
            Line::from(vec![
                Span::styled("a", Style::default().fg(Color::Cyan)),
                Span::raw(": Add new account"),
            ]),
            Line::from(vec![
                Span::styled("r", Style::default().fg(Color::Red)),
                Span::raw(": Remove selected account"),
            ]),
            Line::from(vec![
                Span::styled("q", Style::default().fg(Color::LightRed)),
                Span::raw(": Quit"),
            ]),
        ];
        let help = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(help, area);
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status = Paragraph::new(self.status_message.clone())
            .style(Style::default().fg(Color::LightCyan));
        frame.render_widget(status, area);
    }

    fn draw_popup(&self, frame: &mut Frame) {
        let area = centered_rect(60, 20, frame.size());
        frame.render_widget(Clear, area); // Clear the background

        match &self.popup {
            PopupType::AddAccount => {
                let name_style = if self.add_account_focus == AddAccountField::Name {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let email_style = if self.add_account_focus == AddAccountField::Email {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let popup = Paragraph::new(vec![
                    Line::from(Span::styled(
                        "Add New Account",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Name: ", name_style),
                        Span::styled(&self.new_account_name, name_style),
                    ]),
                    Line::from(vec![
                        Span::styled("Email: ", email_style),
                        Span::styled(&self.new_account_email, email_style),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "↑/↓: Switch fields | Enter: Submit | Esc: Cancel",
                        Style::default().fg(Color::Magenta),
                    )),
                ])
                .block(
                    Block::default()
                        .title("Add Account")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::LightBlue)),
                )
                .style(Style::default().fg(Color::White));
                frame.render_widget(popup, area);
            }
            PopupType::RemoveConfirmation => {
                if let Some(index) = self.active_account_index {
                    if let Some(account) = self.accounts.get(index) {
                        let popup = Paragraph::new(vec![
                            Line::from(Span::styled(
                                "Remove Account",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            )),
                            Line::from(""),
                            Line::from(vec![Span::raw(
                                "Are you sure you want to remove this account?",
                            )]),
                            Line::from(""),
                            Line::from(vec![
                                Span::styled(
                                    "Name: ",
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(&account.name, Style::default().fg(Color::Yellow)),
                            ]),
                            Line::from(vec![
                                Span::styled(
                                    "Email: ",
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(&account.email, Style::default().fg(Color::Cyan)),
                            ]),
                            Line::from(""),
                            Line::from(Span::styled(
                                "Press 'y' to confirm",
                                Style::default().fg(Color::Green),
                            )),
                            Line::from(Span::styled(
                                "Press 'n' to cancel",
                                Style::default().fg(Color::Red),
                            )),
                        ])
                        .block(
                            Block::default()
                                .title("Confirm Removal")
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(Color::LightRed)),
                        )
                        .style(Style::default().fg(Color::White));
                        frame.render_widget(popup, area);
                    } else {
                        // Handle the case where the account doesn't exist
                        let error_popup = Paragraph::new("Error: Account not found")
                            .block(Block::default().title("Error").borders(Borders::ALL))
                            .style(Style::default().fg(Color::Red));
                        frame.render_widget(error_popup, area);
                    }
                } else {
                    // Handle the case where no account is selected
                    let error_popup = Paragraph::new("Error: No account selected")
                        .block(Block::default().title("Error").borders(Borders::ALL))
                        .style(Style::default().fg(Color::Red));
                    frame.render_widget(error_popup, area);
                }
            }
            PopupType::None => {}
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        match &self.popup {
            PopupType::None => match (self.input_mode, key.code) {
                (InputMode::Normal, KeyCode::Char('q')) => self.quit(),
                (InputMode::Normal, KeyCode::Char('c'))
                    if key.modifiers == KeyModifiers::CONTROL =>
                {
                    self.quit()
                }
                (InputMode::Normal, KeyCode::Tab) => self.next_tab(),
                (InputMode::Normal, KeyCode::BackTab) => self.previous_tab(),
                (InputMode::Normal, KeyCode::Char('h')) => self.switch_to_tab(Tab::Help),
                (InputMode::Normal, KeyCode::Char('1')) => self.switch_to_tab(Tab::Accounts),
                (InputMode::Normal, KeyCode::Char('a')) => {
                    self.popup = PopupType::AddAccount;
                    self.new_account_name.clear();
                    self.new_account_email.clear();
                    self.input_mode = InputMode::Editing;
                }
                (InputMode::Normal, KeyCode::Char('r')) => {
                    if self.active_account_index.is_some() {
                        self.popup = PopupType::RemoveConfirmation;
                    }
                }
                (InputMode::Normal, KeyCode::Up) => self.select_previous_account(),
                (InputMode::Normal, KeyCode::Down) => self.select_next_account(),
                (InputMode::Normal, KeyCode::Enter) => {
                    if let Some(index) = self.active_account_index {
                        self.copy_ssh_key_to_clipboard(index);
                    }
                }
                (InputMode::Normal, KeyCode::Char(' ')) => {
                    if let Some(index) = self.active_account_index {
                        self.set_active_account(index);
                    }
                }
                _ => {}
            },
            PopupType::AddAccount => match key.code {
                KeyCode::Enter => {
                    if !self.new_account_name.is_empty() && !self.new_account_email.is_empty() {
                        self.submit_new_account();
                    } else {
                        self.set_status("Please fill both name and email");
                    }
                }
                KeyCode::Esc => {
                    self.popup = PopupType::None;
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => {
                    self.add_account_focus = match self.add_account_focus {
                        AddAccountField::Name => AddAccountField::Email,
                        AddAccountField::Email => AddAccountField::Name,
                    };
                }
                KeyCode::Char(c) => match self.add_account_focus {
                    AddAccountField::Name => self.new_account_name.push(c),
                    AddAccountField::Email => self.new_account_email.push(c),
                },
                KeyCode::Backspace => match self.add_account_focus {
                    AddAccountField::Name => {
                        self.new_account_name.pop();
                    }
                    AddAccountField::Email => {
                        self.new_account_email.pop();
                    }
                },
                _ => {}
            },
            PopupType::RemoveConfirmation => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.remove_account();
                    self.popup = PopupType::None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.popup = PopupType::None;
                }
                _ => {}
            },
        }
    }

    fn set_status(&mut self, message: &str) {
        self.status_message = message.to_string();
        self.status_time = Instant::now();
    }

    fn quit(&mut self) {
        self.running = false;
    }

    fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Accounts => Tab::Help,
            Tab::Help => Tab::Accounts,
        };
        self.set_status(&format!("Switched to {} tab", self.current_tab_name()));
    }

    fn previous_tab(&mut self) {
        self.next_tab(); // Since we only have two tabs, previous is the same as next
    }

    fn current_tab_name(&self) -> &'static str {
        match self.current_tab {
            Tab::Accounts => "Accounts",
            Tab::Help => "Help",
        }
    }

    fn select_previous_account(&mut self) {
        if !self.accounts.is_empty() {
            self.active_account_index = Some(self.active_account_index.map_or(0, |i| {
                if i == 0 {
                    self.accounts.len() - 1
                } else {
                    i - 1
                }
            }));
            self.set_status("Selected previous account");
        }
    }

    fn select_next_account(&mut self) {
        if !self.accounts.is_empty() {
            self.active_account_index = Some(self.active_account_index.map_or(0, |i| {
                if i == self.accounts.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }));
            self.set_status("Selected next account");
        }
    }

    fn submit_new_account(&mut self) {
        if !self.new_account_name.is_empty() && !self.new_account_email.is_empty() {
            if self.is_valid_email(&self.new_account_email) {
                match add_account(&self.new_account_name, &self.new_account_email) {
                    Ok(public_key) => {
                        self.set_status("New account added successfully!");
                        self.accounts = list_accounts().unwrap_or_default();
                        self.new_account_name.clear();
                        self.new_account_email.clear();
                        self.popup = PopupType::None;
                        self.input_mode = InputMode::Normal;
                    }
                    Err(e) => self.set_status(&format!("Failed to add account: {}", e)),
                }
            } else {
                self.set_status("Invalid email format. Please enter a valid email.");
            }
        } else {
            self.set_status("Invalid input. Name and email are required.");
        }
    }

    fn is_valid_email(&self, email: &str) -> bool {
        let email_regex = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
        email_regex.is_match(email)
    }

    fn remove_account(&mut self) {
        if let Some(index) = self.active_account_index {
            if index < self.accounts.len() {
                let account = &self.accounts[index];
                if let Err(e) = remove_account(&account.email) {
                    self.set_status(&format!("Failed to remove account: {}", e));
                } else {
                    self.set_status(&format!("Removed account: {}", account.name));
                    self.accounts = list_accounts().unwrap_or_default();
                    if self.accounts.is_empty() {
                        self.active_account_index = None;
                    } else {
                        self.active_account_index = Some(index.min(self.accounts.len() - 1));
                    }
                }
            }
        }
    }

    fn switch_to_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.set_status(&format!("Switched to {} tab", self.current_tab_name()));
    }

    fn set_active_account(&mut self, index: usize) {
        if index < self.accounts.len() {
            let email = self.accounts[index].email.clone();
            if let Err(e) = switch_account(&email) {
                self.set_status(&format!("Failed to switch account: {}", e));
            } else {
                // Update the active status for all accounts
                for acc in &mut self.accounts {
                    acc.is_active = acc.email == email;
                }
                self.active_account = Some(index);
                self.active_account_index = Some(index);
                self.set_status(&format!("Activated account: {}", self.accounts[index].name));
            }
        }
    }

    fn copy_ssh_key_to_clipboard(&mut self, index: usize) {
        if let Some(account) = self.accounts.get(index) {
            match get_ssh_key(&account.email) {
                Ok(ssh_key) => {
                    if let Err(e) = self.clipboard.set_text(&ssh_key) {
                        self.set_status(&format!("Failed to copy SSH key: {}", e));
                    } else {
                        self.set_status("SSH key copied to clipboard");
                    }
                }
                Err(e) => self.set_status(&format!("Failed to get SSH key: {}", e)),
            }
        }
    }
}
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
