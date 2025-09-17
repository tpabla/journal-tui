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
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
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

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)])
        .split(f.area());
    
    let mut items: Vec<ListItem> = vec![
        ListItem::new("📝 Create New Entry")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    ];
    
    for entry in &app.entries {
        let date_str = entry.created.format("%Y-%m-%d %H:%M").to_string();
        let item = ListItem::new(vec![
            Line::from(vec![
                Span::styled(&entry.title, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled(format!("  {}", date_str), Style::default().fg(Color::Gray)),
            ]),
        ]);
        items.push(item);
    }
    
    let list = List::new(items)
        .block(
            Block::default()
                .title(" Journal Entries ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan))
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        )
        .highlight_symbol("▶ ");
    
    f.render_stateful_widget(list, chunks[0], &mut app.list_state);
    
    if matches!(app.mode, AppMode::TitleInput) {
        let popup_area = centered_rect(60, 20, f.area());
        
        f.render_widget(Clear, popup_area);
        
        let input_block = Block::default()
            .title(" New Entry Title ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow));
        
        let input_area = popup_area.inner(Margin::new(1, 1));
        
        let input = Paragraph::new(app.title_input.as_str())
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false });
        
        f.render_widget(input_block, popup_area);
        f.render_widget(input, input_area);
        
        f.set_cursor_position((
            input_area.x + app.title_input.len() as u16,
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
        x: chunks[0].x,
        y: chunks[0].bottom() - 1,
        width: chunks[0].width,
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