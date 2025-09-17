mod auth;
mod matrix;

use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame, Terminal,
};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Debug)]
struct JournalEntry {
    title: String,
    path: PathBuf,
    created: DateTime<Local>,
}

#[derive(Debug)]
enum AppMode {
    Normal,
    TitleInput,
}

struct App {
    entries: Vec<JournalEntry>,
    list_state: ListState,
    mode: AppMode,
    title_input: String,
    journal_dir: PathBuf,
}

impl App {
    fn new() -> Result<Self> {
        let home_dir = dirs::home_dir().expect("Could not find home directory");
        let journal_dir = home_dir.join(".journal").join("entries");
        
        if !journal_dir.exists() {
            fs::create_dir_all(&journal_dir)?;
        }
        
        let mut app = App {
            entries: Vec::new(),
            list_state: ListState::default(),
            mode: AppMode::Normal,
            title_input: String::new(),
            journal_dir,
        };
        
        app.load_entries()?;
        // Always select the first item (Create New Entry)
        app.list_state.select(Some(0));
        
        Ok(app)
    }
    
    fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        
        if let Ok(entries) = fs::read_dir(&self.journal_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(created) = metadata.created() {
                            let created: DateTime<Local> = created.into();
                            if let Some(title) = self.read_title_from_file(&path) {
                                self.entries.push(JournalEntry {
                                    title,
                                    path,
                                    created,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        self.entries.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(())
    }
    
    fn read_title_from_file(&self, path: &Path) -> Option<String> {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(title) = line.strip_prefix("# ") {
                    return Some(title.to_string());
                }
            }
        }
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
    
    fn create_new_entry(&mut self) -> Result<()> {
        if self.title_input.trim().is_empty() {
            return Ok(());
        }
        
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.md", timestamp, self.title_input.replace(' ', "_"));
        let filepath = self.journal_dir.join(filename);
        
        let content = format!("# {}\n\n", self.title_input);
        fs::write(&filepath, content)?;
        
        // Suspend raw mode but don't clear screen
        disable_raw_mode()?;
        
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
        Command::new(editor)
            .arg(&filepath)
            .arg("+2")
            .status()?;
        
        // Re-enable raw mode
        enable_raw_mode()?;
        
        self.title_input.clear();
        self.mode = AppMode::Normal;
        self.load_entries()?;
        
        Ok(())
    }
    
    fn open_entry(&mut self) -> Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if selected > 0 && selected <= self.entries.len() {
                let entry = &self.entries[selected - 1];
                
                // Suspend raw mode but don't clear screen
                disable_raw_mode()?;
                
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
                Command::new(editor)
                    .arg(&entry.path)
                    .status()?;
                
                // Re-enable raw mode
                enable_raw_mode()?;
                
                self.load_entries()?;
            }
        }
        Ok(())
    }
    
    fn move_selection_up(&mut self) {
        let current = self.list_state.selected().unwrap_or(0);
        if current > 0 {
            self.list_state.select(Some(current - 1));
        }
    }
    
    fn move_selection_down(&mut self) {
        let current = self.list_state.selected().unwrap_or(0);
        let max = self.entries.len();
        if current < max {
            self.list_state.select(Some(current + 1));
        }
    }
}

fn main() -> Result<()> {
    // Run matrix authentication animation
    let authenticated = matrix::run_matrix_authentication(|| auth::authenticate())?;
    
    if !authenticated {
        println!("Authentication required to access journal");
        return Ok(());
    }
    
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    let app = App::new()?;
    let res = run_app(&mut terminal, app);
    
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    
    if let Err(err) = res {
        eprintln!("Error: {err:?}");
    }
    
    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> Result<()> {
    // Initial draw
    terminal.draw(|f| ui(f, &mut app))?;
    
    loop {
        // Poll for events with a timeout to prevent blocking
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Only process key press events, ignore key release events
                if key.kind != KeyEventKind::Press {
                    continue;
                }
            
            let needs_refresh = match app.mode {
                AppMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.move_selection_down();
                        false
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.move_selection_up();
                        false
                    }
                    KeyCode::Char('g') => {
                        if key.modifiers.contains(KeyModifiers::NONE) {
                            app.list_state.select(Some(0));
                        }
                        false
                    }
                    KeyCode::Char('G') => {
                        let max = app.entries.len();
                        app.list_state.select(Some(max));
                        false
                    }
                    KeyCode::Enter => {
                        if let Some(0) = app.list_state.selected() {
                            app.mode = AppMode::TitleInput;
                            false
                        } else {
                            app.open_entry()?;
                            // Need full refresh after vim
                            true
                        }
                    }
                    _ => false
                },
                AppMode::TitleInput => match key.code {
                    KeyCode::Esc => {
                        app.title_input.clear();
                        app.mode = AppMode::Normal;
                        false
                    }
                    KeyCode::Enter => {
                        app.create_new_entry()?;
                        // Need full refresh after vim
                        true
                    }
                    KeyCode::Backspace => {
                        app.title_input.pop();
                        false
                    }
                    KeyCode::Char(c) => {
                        app.title_input.push(c);
                        false
                    }
                    _ => false
                },
            };
            
            if needs_refresh {
                // Clear and resize terminal after vim
                terminal.clear()?;
            }
            }
        }
        
        // Always redraw
        terminal.draw(|f| ui(f, &mut app))?;
    }
}

fn render_preview_pane(f: &mut Frame, app: &App, area: Rect) {
    let selected = app.list_state.selected().unwrap_or(0);
    
    // ASCII art header for preview
    let preview_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Header
            Constraint::Min(0),     // Content
        ])
        .split(area);
    
    // Render preview header
    let header = vec![
        Line::from(vec![Span::styled("╔═══════════════════════════════╗", Style::default().fg(Color::Cyan))]),
        Line::from(vec![Span::styled("║  ░▒▓ MEMORY  PREVIEW ▓▒░     ║", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("║  ░▒▓ DATA    STREAM  ▓▒░     ║", Style::default().fg(Color::LightGreen))]),
        Line::from(vec![Span::styled("╚═══════════════════════════════╝", Style::default().fg(Color::Cyan))]),
    ];
    let header_widget = Paragraph::new(header)
        .alignment(Alignment::Center)
        .style(Style::default().bg(Color::Rgb(0, 0, 0)));
    f.render_widget(header_widget, preview_layout[0]);
    
    // Render preview content
    let content = if selected == 0 {
        vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("▓▒░ ", Style::default().fg(Color::LightGreen)),
                Span::styled("READY TO INITIALIZE", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("► ", Style::default().fg(Color::LightGreen)),
                Span::styled("Press ENTER to begin memory capture", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("► ", Style::default().fg(Color::LightGreen)),
                Span::styled("System will launch neural interface", Style::default().fg(Color::Gray)),
            ]),
            Line::from(vec![
                Span::styled("► ", Style::default().fg(Color::LightGreen)),
                Span::styled("Memory will be encrypted and stored", Style::default().fg(Color::Gray)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("[SYSTEM] ", Style::default().fg(Color::DarkGray)),
                Span::styled("Awaiting input...", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ]),
        ]
    } else if selected > 0 && selected <= app.entries.len() {
        let entry = &app.entries[selected - 1];
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("▓▒░ MEMORY BLOCK #", Style::default().fg(Color::LightGreen)),
                Span::styled(format!("{:04}", selected), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
        ];
        
        // Try to read the file content
        if let Ok(content) = fs::read_to_string(&entry.path) {
            let preview_lines: Vec<&str> = content.lines().skip(2).take(20).collect();
            
            if preview_lines.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("[EMPTY] ", Style::default().fg(Color::DarkGray)),
                    Span::styled("No data recorded", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            } else {
                for line in preview_lines {
                    if line.len() > 60 {
                        let truncated = format!("{}...", &line[..57]);
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(truncated, Style::default().fg(Color::Green)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(line.to_string(), Style::default().fg(Color::Green)),
                        ]));
                    }
                }
                
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("[EOF] ", Style::default().fg(Color::DarkGray)),
                    Span::styled("Press ENTER to access full memory", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled("[ERROR] ", Style::default().fg(Color::Red)),
                Span::styled("Failed to decode memory block", Style::default().fg(Color::Red).add_modifier(Modifier::ITALIC)),
            ]));
        }
        
        lines
    } else {
        vec![Line::from("")]
    };
    
    let preview = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Cyan).bg(Color::Rgb(0, 0, 0)))
                .style(Style::default().bg(Color::Rgb(0, 0, 0)))
        )
        .style(Style::default().fg(Color::Green).bg(Color::Rgb(0, 0, 0)))
        .wrap(Wrap { trim: false });
    
    f.render_widget(preview, preview_layout[1]);
}

fn ui(f: &mut Frame, app: &mut App) {
    // Set black background for entire frame
    let area = f.area();
    f.buffer_mut().set_style(area, Style::default().bg(Color::Rgb(0, 0, 0)));
    
    // Create layout with preview pane
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),  // Entry list
            Constraint::Percentage(60),  // Preview pane
        ])
        .split(area);
    
    // ASCII art header for the list
    let list_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Header
            Constraint::Min(0),     // List
        ])
        .split(main_layout[0]);
    
    // Render ASCII header
    let header = vec![
        Line::from(vec![Span::styled("╔═══════════════════════════════╗", Style::default().fg(Color::LightGreen))]),
        Line::from(vec![Span::styled("║  ░▒▓ NEURAL  JOURNAL ▓▒░     ║", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled("║  ░▒▓ MEMORY  ARCHIVE ▓▒░     ║", Style::default().fg(Color::Cyan))]),
        Line::from(vec![Span::styled("╚═══════════════════════════════╝", Style::default().fg(Color::LightGreen))]),
    ];
    let header_widget = Paragraph::new(header)
        .alignment(Alignment::Center)
        .style(Style::default().bg(Color::Rgb(0, 0, 0)));
    f.render_widget(header_widget, list_layout[0]);
    
    // Create list items with larger text
    let mut items: Vec<ListItem> = vec![
        ListItem::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("[+] ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                Span::styled("CREATE NEW ENTRY", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("    └─> ", Style::default().fg(Color::DarkGray)),
                Span::styled("Initialize new memory block", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ]),
            Line::from(""),
        ])
    ];
    
    for (i, entry) in app.entries.iter().enumerate() {
        let date_str = entry.created.format("%Y-%m-%d %H:%M").to_string();
        let item = ListItem::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("[{}] ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.title, Style::default().fg(Color::LightGreen)),
            ]),
            Line::from(vec![
                Span::styled("    ├─> ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("Timestamp: {}", date_str), Style::default().fg(Color::Gray)),
            ]),
            Line::from(""),
        ]);
        items.push(item);
    }
    
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::LightGreen).bg(Color::Rgb(0, 0, 0)))
                .style(Style::default().bg(Color::Rgb(0, 0, 0)))
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(0, 40, 0))
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        )
        .highlight_symbol("█▓▒░ ");
    
    f.render_stateful_widget(list, list_layout[1], &mut app.list_state);
    
    // Render preview pane
    render_preview_pane(f, app, main_layout[1]);
    
    if matches!(app.mode, AppMode::TitleInput) {
        let popup_area = centered_rect(60, 20, f.area());
        
        // Fill popup area with black
        let buf = f.buffer_mut();
        for y in popup_area.top()..popup_area.bottom() {
            for x in popup_area.left()..popup_area.right() {
                let cell = &mut buf[(x, y)];
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(Color::Rgb(0, 0, 0)));
            }
        }
        
        let input_block = Block::default()
            .title("╔═ INITIALIZE MEMORY BLOCK ═╗")
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::LightGreen).bg(Color::Rgb(0, 0, 0)));
        
        let input_area = popup_area.inner(Margin::new(1, 1));
        
        let cursor = "█";
        let input = Paragraph::new(format!("> {}{}", app.title_input, cursor))
            .style(Style::default().fg(Color::LightGreen).bg(Color::Rgb(0, 0, 0)))
            .wrap(Wrap { trim: false });
        
        f.render_widget(input_block, popup_area);
        f.render_widget(input, input_area);
        
        f.set_cursor_position((
            input_area.x + 2 + app.title_input.len() as u16,  // +2 for "> " prefix
            input_area.y,
        ));
    }
    
    let help_text = if matches!(app.mode, AppMode::Normal) {
        " j/k: navigate | Enter: select | q: quit "
    } else {
        " Enter: create | Esc: cancel "
    };
    
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    
    let help_area = Rect {
        x: area.x,
        y: area.bottom() - 1,
        width: area.width,
        height: 1,
    };
    
    f.render_widget(help, help_area);
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