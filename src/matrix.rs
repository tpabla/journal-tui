use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use rand::Rng;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::{
    io,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct MatrixColumn {
    chars: Vec<char>,
    position: f32,
    speed: f32,
    length: usize,
    brightness: Vec<f32>,
}

impl MatrixColumn {
    fn new(height: usize) -> Self {
        let mut rng = rand::thread_rng();
        let chars: Vec<char> = (0..height)
            .map(|_| {
                let chars = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*(){}[]|\\:;<>?,./";
                let chars_vec: Vec<char> = chars.chars().collect();
                chars_vec[rng.gen_range(0..chars_vec.len())]
            })
            .collect();
        
        Self {
            chars,
            position: rng.gen_range(-20.0..0.0),
            speed: rng.gen_range(0.3..1.5),
            length: rng.gen_range(5..20),
            brightness: vec![0.0; height],
        }
    }
    
    fn update(&mut self) {
        self.position += self.speed;
        let mut rng = rand::thread_rng();
        
        // Update brightness
        for i in 0..self.brightness.len() {
            let relative_pos = i as f32 - self.position;
            if relative_pos >= 0.0 && relative_pos < self.length as f32 {
                let fade = 1.0 - (relative_pos / self.length as f32);
                self.brightness[i] = fade;
            } else {
                self.brightness[i] *= 0.95;
            }
        }
        
        // Randomly change some characters
        if rng.gen_bool(0.1) {
            let chars = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*(){}[]|\\:;<>?,./";
            let chars_vec: Vec<char> = chars.chars().collect();
            for c in &mut self.chars {
                if rng.gen_bool(0.02) {
                    *c = chars_vec[rng.gen_range(0..chars_vec.len())];
                }
            }
        }
        
        // Reset column when it goes off screen
        if self.position > self.chars.len() as f32 + self.length as f32 {
            self.position = rng.gen_range(-30.0..-10.0);
            self.speed = rng.gen_range(0.3..1.5);
            self.length = rng.gen_range(5..20);
        }
    }
}

pub struct MatrixAnimation {
    columns: Vec<MatrixColumn>,
    phase: AnimationPhase,
    start_time: Instant,
    message: String,
    decoded_chars: usize,
    decode_complete_time: Option<Instant>,
}

#[derive(Clone, PartialEq)]
enum AnimationPhase {
    MatrixRain,
    Authenticating,
    Decoding,
    Success,
    Failed,
}

impl MatrixAnimation {
    pub fn new(width: u16, height: u16) -> Self {
        let columns: Vec<MatrixColumn> = (0..width)
            .map(|_| MatrixColumn::new(height as usize))
            .collect();
        
        Self {
            columns,
            phase: AnimationPhase::MatrixRain,
            start_time: Instant::now(),
            message: String::new(),
            decoded_chars: 0,
            decode_complete_time: None,
        }
    }
    
    pub fn start_authentication(&mut self) {
        self.phase = AnimationPhase::Authenticating;
        self.message = "BIOMETRIC SCAN INITIATED...".to_string();
    }
    
    pub fn authentication_success(&mut self) {
        self.phase = AnimationPhase::Decoding;
        self.message = "ACCESS GRANTED - DECRYPTING JOURNAL".to_string();
        self.decoded_chars = 0;
    }
    
    pub fn authentication_failed(&mut self) {
        self.phase = AnimationPhase::Failed;
        self.message = "ACCESS DENIED".to_string();
    }
    
    pub fn update(&mut self) {
        for col in &mut self.columns {
            col.update();
        }
        
        if self.phase == AnimationPhase::Decoding {
            // Type out the message character by character
            if self.decoded_chars < self.message.len() {
                self.decoded_chars = (self.decoded_chars + 1).min(self.message.len());
            } else if self.decode_complete_time.is_none() {
                // Mark when typing is complete
                self.decode_complete_time = Some(Instant::now());
            }
            
            // Wait 3 seconds after typing is complete before transitioning to journal
            if let Some(complete_time) = self.decode_complete_time {
                if complete_time.elapsed() > Duration::from_secs(3) {
                    self.phase = AnimationPhase::Success;
                }
            }
        }
    }
}

pub fn run_matrix_authentication<F>(auth_fn: F) -> Result<bool>
where
    F: FnOnce() -> Result<bool> + Send + 'static,
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout, 
        EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb{r: 0, g: 0, b: 0}),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    let (width, height) = terminal.size().map(|r| (r.width, r.height))?;
    let mut animation = MatrixAnimation::new(width, height);
    
    // Show authentication message immediately
    animation.start_authentication();
    
    // Run authentication in background with 3 second delay
    let auth_result = thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        auth_fn()
    });
    
    // Continue showing matrix rain with auth message for 3 seconds
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        animation.update();
        terminal.draw(|f| draw_matrix(f, &animation))?;
        thread::sleep(Duration::from_millis(50));
    }
    
    loop {
        animation.update();
        
        terminal.draw(|f| draw_matrix(f, &animation))?;
        
        // Check for auth result
        if auth_result.is_finished() {
            match auth_result.join().unwrap() {
                Ok(true) => {
                    animation.authentication_success();
                    
                    // Keep running until the animation completes (typing + 5 second wait)
                    while animation.phase != AnimationPhase::Success {
                        animation.update();
                        terminal.draw(|f| draw_matrix(f, &animation))?;
                        thread::sleep(Duration::from_millis(50));
                    }
                    
                    disable_raw_mode()?;
                    execute!(
                        terminal.backend_mut(), 
                        crossterm::cursor::Show,
                        LeaveAlternateScreen
                    )?;
                    return Ok(true);
                }
                _ => {
                    animation.authentication_failed();
                    
                    // Show failure for a moment
                    let fail_start = Instant::now();
                    while fail_start.elapsed() < Duration::from_secs(2) {
                        animation.update();
                        terminal.draw(|f| draw_matrix(f, &animation))?;
                        thread::sleep(Duration::from_millis(50));
                    }
                    
                    disable_raw_mode()?;
                    execute!(
                        terminal.backend_mut(),
                        crossterm::cursor::Show, 
                        LeaveAlternateScreen
                    )?;
                    return Ok(false);
                }
            }
        }
        
        // Check for ESC key
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Esc {
                    disable_raw_mode()?;
                    execute!(
                        terminal.backend_mut(),
                        crossterm::cursor::Show, 
                        LeaveAlternateScreen
                    )?;
                    return Ok(false);
                }
            }
        }
        
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn run_matrix_encrypting_animation() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout, 
        EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb{r: 0, g: 0, b: 0}),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    let (width, height) = terminal.size().map(|r| (r.width, r.height))?;
    let mut animation = MatrixAnimation::new(width, height);
    
    // Set up for encrypting message
    animation.phase = AnimationPhase::Decoding;
    animation.message = "ENCRYPTING VAULT - SECURING MEMORIES".to_string();
    animation.decoded_chars = 0;
    
    let start = Instant::now();
    
    // Show the typing animation for 2 seconds
    while start.elapsed() < Duration::from_secs(2) {
        animation.update();
        
        // Type out the message
        if animation.decoded_chars < animation.message.len() {
            animation.decoded_chars = (animation.decoded_chars + 1).min(animation.message.len());
        }
        
        terminal.draw(|f| draw_matrix(f, &animation))?;
        thread::sleep(Duration::from_millis(50));
        
        // Check for ESC key to skip
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }
    
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::cursor::Show, 
        LeaveAlternateScreen,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;
    
    Ok(())
}

fn draw_matrix(f: &mut Frame, animation: &MatrixAnimation) {
    let area = f.area();
    
    // Explicitly set every cell to have black background with content
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");  // Set a space character
            cell.set_style(Style::new().bg(Color::Rgb(0, 0, 0)));
        }
    }
    
    // Draw matrix rain
    for (x, col) in animation.columns.iter().enumerate() {
        for (y, &brightness) in col.brightness.iter().enumerate() {
            if brightness > 0.01 && y < area.height as usize {
                let color = if brightness > 0.8 {
                    Color::White
                } else if brightness > 0.4 {
                    Color::LightGreen
                } else {
                    Color::Green
                };
                
                let style = if brightness > 0.9 {
                    Style::new().fg(color).bg(Color::Rgb(0, 0, 0)).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(color).bg(Color::Rgb(0, 0, 0))
                };
                
                let char_idx = y.min(col.chars.len().saturating_sub(1));
                let text = Span::styled(col.chars[char_idx].to_string(), style);
                
                if x < area.width as usize && y < area.height as usize {
                    let rect = Rect::new(x as u16, y as u16, 1, 1);
                    f.render_widget(Paragraph::new(text), rect);
                }
            }
        }
    }
    
    // Draw center message based on phase
    let center = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(7),
            Constraint::Percentage(50),
        ])
        .split(area);
    
    let message_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(center[1])[1];
    
    let (message, style) = match animation.phase {
        AnimationPhase::MatrixRain => ("".to_string(), Style::default()),
        AnimationPhase::Authenticating => {
            let dots = ".".repeat((animation.start_time.elapsed().as_millis() / 500 % 4) as usize);
            (
                format!("🔐 BIOMETRIC SCAN IN PROGRESS{}", dots),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            )
        }
        AnimationPhase::Decoding => {
            // Show typed message with blinking cursor
            let typed_message = &animation.message[..animation.decoded_chars];
            let show_cursor = animation.start_time.elapsed().as_millis() / 500 % 2 == 0;
            let cursor = if animation.decoded_chars < animation.message.len() && show_cursor {
                "█"
            } else if animation.decoded_chars >= animation.message.len() && show_cursor {
                "█"
            } else {
                ""
            };
            
            (
                format!("> {}{}", typed_message, cursor),
                Style::default().fg(Color::LightGreen)
            )
        }
        AnimationPhase::Success => (
            "🚀 ENTERING THE MATRIX...".to_string(),
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        ),
        AnimationPhase::Failed => (
            "❌ ACCESS DENIED - AUTHENTICATION FAILED".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        ),
    };
    
    // Explicitly set every cell in message area to black with content
    let buf = f.buffer_mut();
    for y in message_area.top()..message_area.bottom() {
        for x in message_area.left()..message_area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");  // Set a space character
            cell.set_style(Style::new().bg(Color::Rgb(0, 0, 0)));
        }
    }
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(match animation.phase {
            AnimationPhase::Failed => Style::default().fg(Color::Red),
            AnimationPhase::Success | AnimationPhase::Decoding => Style::default().fg(Color::LightGreen),
            _ => Style::default().fg(Color::Cyan),
        })
        .style(Style::new().bg(Color::Rgb(0, 0, 0)));
    
    let paragraph = Paragraph::new(message)
        .style(style.bg(Color::Rgb(0, 0, 0)))  // Ensure message text also has black background
        .block(block)
        .alignment(Alignment::Center);
    
    f.render_widget(paragraph, message_area);
}