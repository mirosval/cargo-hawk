use ansi_to_tui::IntoText;
use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    io,
    path::PathBuf,
    process::Stdio,
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const CUSTOM_COMMAND_PLACEHOLDER: &str = "<custom command>";

#[derive(Parser, Debug)]
#[command(name = "cargo-hawk")]
#[command(about = "A file watcher with interactive command runner for Rust projects", long_about = None)]
struct Args {
    /// Directory to watch (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// Custom command to run (in addition to built-in commands)
    #[arg(short, long)]
    custom: Option<String>,
}

#[derive(Debug, Clone)]
enum AppEvent {
    FileChanged(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
enum CargoCommand {
    Check,
    Build,
    Run,
    Test,
    Clippy,
    Custom(String),
}

impl CargoCommand {
    fn as_display(&self) -> &str {
        match self {
            CargoCommand::Check => "check",
            CargoCommand::Build => "build",
            CargoCommand::Run => "run",
            CargoCommand::Test => "test",
            CargoCommand::Clippy => "clippy",
            CargoCommand::Custom(cmd) => {
                if cmd.is_empty() {
                    CUSTOM_COMMAND_PLACEHOLDER
                } else {
                    cmd
                }
            }
        }
    }
}

struct App {
    commands: Vec<CargoCommand>,
    selected: ListState,
    output: Vec<String>,
    last_file_changed: Option<String>,
    running: bool,
    scroll_offset: usize,
    command_inputs: Vec<String>,
    input_cursor: usize,
    running_task: Option<JoinHandle<Result<CommandResult>>>,
    running_child: Arc<Mutex<Option<Child>>>,
    input_focused: bool,
}

struct CommandResult {
    stdout: String,
    stderr: String,
    success: bool,
    exit_code: Option<i32>,
}

impl App {
    fn new(custom_command: Option<String>) -> Self {
        let mut commands = vec![
            CargoCommand::Check,
            CargoCommand::Build,
            CargoCommand::Run,
            CargoCommand::Test,
            CargoCommand::Clippy,
            CargoCommand::Custom("".to_string()),
        ];

        if let Some(cmd) = custom_command {
            commands.push(CargoCommand::Custom(cmd));
        }

        let mut selected = ListState::default();
        selected.select(Some(0));

        // Initialize command inputs with default command strings
        let command_inputs: Vec<String> = commands
            .iter()
            .map(|c| format!("cargo {}", c.as_display()))
            .collect();
        let input_cursor = command_inputs[0].len();

        Self {
            commands,
            selected,
            output: vec!["Watching for file changes...".to_string()],
            last_file_changed: None,
            running: false,
            scroll_offset: 0,
            command_inputs,
            input_cursor,
            running_task: None,
            running_child: Arc::new(Mutex::new(None)),
            input_focused: false,
        }
    }

    fn next(&mut self) {
        if self.running {
            return;
        }
        let current = self.selected.selected().unwrap_or(0);
        let i = if current >= self.commands.len() - 1 {
            0
        } else {
            current + 1
        };
        self.selected.select(Some(i));
        self.update_cursor_for_current_input();
        self.start_command();
    }

    fn previous(&mut self) {
        if self.running {
            return;
        }
        let current = self.selected.selected().unwrap_or(0);
        let i = if current == 0 {
            self.commands.len() - 1
        } else {
            current - 1
        };
        self.selected.select(Some(i));
        self.update_cursor_for_current_input();
        self.start_command();
    }

    fn switch_to_tab(&mut self, idx: usize) {
        if !self.running && idx < self.commands.len() {
            self.selected.select(Some(idx));
            self.update_cursor_for_current_input();
            self.start_command();
        }
    }

    fn update_cursor_for_current_input(&mut self) {
        let idx = self.selected.selected().unwrap_or(0);
        self.input_cursor = self.command_inputs[idx].len();
    }

    fn input_insert_char(&mut self, c: char) {
        if self.running {
            return;
        }
        let idx = self.selected.selected().unwrap_or(0);
        self.command_inputs[idx].insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    fn input_delete_char(&mut self) {
        if self.running || self.input_cursor == 0 {
            return;
        }
        let idx = self.selected.selected().unwrap_or(0);
        self.command_inputs[idx].remove(self.input_cursor - 1);
        self.input_cursor -= 1;
    }

    fn input_move_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
        }
    }

    fn input_move_cursor_right(&mut self) {
        let idx = self.selected.selected().unwrap_or(0);
        if self.input_cursor < self.command_inputs[idx].len() {
            self.input_cursor += 1;
        }
    }

    fn input_move_cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    fn input_move_cursor_end(&mut self) {
        let idx = self.selected.selected().unwrap_or(0);
        self.input_cursor = self.command_inputs[idx].len();
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_down(&mut self) {
        self.scroll_offset += 1;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.output.len().saturating_sub(1);
    }

    fn add_output(&mut self, line: String) {
        self.output.push(line);
        // Auto-scroll to bottom when new output arrives
        self.scroll_to_bottom();
    }

    fn clear_output(&mut self) {
        self.output.clear();
        self.scroll_offset = 0;
    }

    fn start_command(&mut self) {
        if self.running {
            return;
        }

        let selected_idx = self.selected.selected().unwrap_or(0);
        let command_str = self.command_inputs[selected_idx].clone();

        self.running = true;
        self.clear_output();
        self.add_output(format!("Running: {}", command_str));
        self.add_output(String::new());

        // Parse the command string into program and args
        let parts: Vec<String> = command_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if parts.is_empty() || command_str.contains(CUSTOM_COMMAND_PLACEHOLDER) {
            self.add_output("Error: Empty command".to_string());
            self.running = false;
            return;
        }

        let program = parts[0].clone();
        let mut args: Vec<String> = parts[1..].to_vec();

        // Inject JSON format for cargo commands
        if program == "cargo" && !args.is_empty() {
            let cargo_subcommand = &args[0];
            let known_commands = ["check", "build", "test", "run", "clippy"];

            if known_commands.contains(&cargo_subcommand.as_str()) {
                // Insert --message-format after the subcommand
                args.insert(1, "--message-format".to_string());
                args.insert(2, "json-diagnostic-rendered-ansi".to_string());
            }
        }

        let child_handle = self.running_child.clone();

        // Spawn the command execution as a background task
        let task = tokio::spawn(async move {
            let child = Command::new(&program)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            // Store the child handle so it can be killed if needed
            {
                let mut guard = child_handle.lock().await;
                *guard = Some(child);
            }

            // Take the child out of the Option to wait for it
            let child_to_wait = {
                let mut guard = child_handle.lock().await;
                guard.take()
            };

            let output = if let Some(child) = child_to_wait {
                child.wait_with_output().await?
            } else {
                return Err(anyhow::anyhow!("Child process was killed"));
            };

            // Child handle is already cleared by the take() above

            // Keep ANSI color codes in the output for colored display
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            Ok(CommandResult {
                stdout,
                stderr,
                success: output.status.success(),
                exit_code: output.status.code(),
            })
        });

        self.running_task = Some(task);
    }

    async fn check_command_completion(&mut self) {
        if let Some(task) = &mut self.running_task {
            if task.is_finished() {
                let task = self.running_task.take().unwrap();

                // Get the result (this won't block since it's finished)
                match task.await {
                    Ok(Ok(result)) => {
                        for line in result.stdout.lines() {
                            self.add_output(line.to_string());
                        }
                        for line in result.stderr.lines() {
                            self.add_output(line.to_string());
                        }
                        self.add_output(String::new());
                        if result.success {
                            self.add_output("✓ Command completed successfully".to_string());
                        } else {
                            self.add_output(format!(
                                "✗ Command failed with exit code: {}",
                                result.exit_code.unwrap_or(-1)
                            ));
                        }
                    }
                    Ok(Err(e)) => {
                        self.add_output(format!("Error: {}", e));
                    }
                    Err(e) => {
                        if e.is_cancelled() {
                            self.add_output("Command was cancelled".to_string());
                        } else {
                            self.add_output(format!("Task error: {}", e));
                        }
                    }
                }

                self.running = false;
            }
        }
    }

    fn cancel_running_command(&mut self) {
        // Kill the child process if it exists
        {
            let mut guard = self.running_child.blocking_lock();
            if let Some(child) = guard.as_mut() {
                let _ = child.start_kill();
            }
            *guard = None;
        }

        // Abort the task
        if let Some(task) = self.running_task.take() {
            task.abort();
        }

        self.running = false;
    }
}

fn get_tab_color(idx: usize) -> Color {
    let colors = [
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Blue,
        Color::Red,
    ];
    colors[idx % colors.len()]
}

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Top line: tabs on left, "Cargo Hawk" on right, with dark background
    let selected_idx = app.selected.selected().unwrap_or(0);
    let selected_color = get_tab_color(selected_idx);

    let mut tab_spans = vec![];
    for (idx, cmd) in app.commands.iter().enumerate() {
        let tab_text = format!(" {} {} ", idx + 1, cmd.as_display());
        let tab_color = get_tab_color(idx);
        let style = if idx == selected_idx {
            Style::default()
                .fg(Color::Black)
                .bg(tab_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(tab_color).bg(Color::DarkGray)
        };
        tab_spans.push(Span::styled(tab_text, style));
    }

    let tab_line = Line::from(tab_spans);
    let tabs_widget = Paragraph::new(tab_line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(tabs_widget, chunks[0]);

    // Render "Cargo Hawk" on the right side of the same line
    let title_text = "Cargo Hawk";
    let title_x = chunks[0].width.saturating_sub(title_text.len() as u16);
    let title_area = ratatui::layout::Rect {
        x: chunks[0].x + title_x,
        y: chunks[0].y,
        width: title_text.len() as u16,
        height: 1,
    };
    let title =
        Paragraph::new(title_text).style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(title, title_area);

    // Colored separator line using upper half block character (▀) to touch the tab bar
    let separator_line = "▀".repeat(chunks[1].width as usize);
    let separator = Paragraph::new(separator_line).style(Style::default().fg(selected_color));
    let separator_area = ratatui::layout::Rect {
        x: chunks[1].x,
        y: chunks[0].y + chunks[0].height,
        width: chunks[1].width,
        height: 1,
    };
    frame.render_widget(separator, separator_area);

    // Output panel - positioned right after separator
    let output_area = ratatui::layout::Rect {
        x: chunks[1].x,
        y: chunks[0].y + chunks[0].height + 1,
        width: chunks[1].width,
        height: chunks[1].height.saturating_sub(1),
    };

    let output_height = output_area.height as usize;
    let start_line = app
        .scroll_offset
        .min(app.output.len().saturating_sub(output_height));
    let end_line = (start_line + output_height).min(app.output.len());

    // Parse ANSI color codes in the output
    let visible_output: Vec<Line> = app.output[start_line..end_line]
        .iter()
        .map(|line| {
            // Parse ANSI codes and convert to styled text
            match line.as_bytes().into_text() {
                Ok(text) => {
                    // Extract the first line from the parsed text
                    if text.lines.is_empty() {
                        Line::from("")
                    } else {
                        text.lines[0].clone()
                    }
                }
                Err(_) => Line::from(line.clone()),
            }
        })
        .collect();

    let output_panel = Paragraph::new(visible_output)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((0, 0));

    frame.render_widget(output_panel, output_area);

    // Input field with focus indicator
    let input_text = &app.command_inputs[selected_idx];
    let (input_style, input_title) = if app.input_focused {
        (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            "Command [EDITING]",
        )
    } else {
        (Style::default().fg(Color::DarkGray), "Command")
    };
    let input = Paragraph::new(input_text.as_str())
        .block(Block::default().borders(Borders::ALL).title(input_title))
        .style(input_style);
    frame.render_widget(input, chunks[2]);

    // Set cursor position in the input field (only visible when focused)
    if app.input_focused {
        frame.set_cursor_position((chunks[2].x + app.input_cursor as u16 + 1, chunks[2].y + 1));
    }

    // Footer with mode-specific shortcuts
    let footer_text = if app.input_focused {
        "Esc: Exit edit mode | Enter: Run | ←/→/Home/End: Navigate | Ctrl+C: Quit"
    } else {
        "q/Esc: Quit | i: Edit command | Enter/r: Run | 1-9: Switch tab | j/k/↑/↓: Navigate | PgUp/PgDn: Scroll"
    };
    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(footer, chunks[3]);
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    event_rx: mpsc::Receiver<AppEvent>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        // Check if running command has completed
        app.check_command_completion().await;

        // Non-blocking event check
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Ctrl+C always quits
                    if key.code == KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        app.cancel_running_command();
                        return Ok(());
                    }

                    if app.input_focused {
                        // FOCUSED MODE - editing command input
                        match key.code {
                            KeyCode::Esc => {
                                app.input_focused = false;
                            }
                            KeyCode::Enter => {
                                app.input_focused = false;
                                app.start_command();
                            }
                            KeyCode::Backspace => app.input_delete_char(),
                            KeyCode::Left => app.input_move_cursor_left(),
                            KeyCode::Right => app.input_move_cursor_right(),
                            KeyCode::Home => app.input_move_cursor_home(),
                            KeyCode::End => app.input_move_cursor_end(),
                            KeyCode::Char(c) => app.input_insert_char(c),
                            _ => {}
                        }
                    } else {
                        // UNFOCUSED MODE - navigation
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                app.cancel_running_command();
                                return Ok(());
                            }
                            KeyCode::Char('i') => {
                                app.input_focused = true;
                            }
                            KeyCode::Down | KeyCode::Char('j') => app.next(),
                            KeyCode::Up | KeyCode::Char('k') => app.previous(),
                            KeyCode::Enter => {
                                app.start_command();
                            }
                            KeyCode::Char('r') => {
                                app.start_command();
                            }
                            KeyCode::Char(c @ '1'..='9') => {
                                let idx = c.to_digit(10).unwrap() as usize - 1;
                                app.switch_to_tab(idx);
                            }
                            KeyCode::PageUp => {
                                for _ in 0..10 {
                                    app.scroll_up();
                                }
                            }
                            KeyCode::PageDown => {
                                for _ in 0..10 {
                                    app.scroll_down();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Check for file change events
        while let Ok(AppEvent::FileChanged(path)) = event_rx.try_recv() {
            app.last_file_changed = Some(path.display().to_string());
            app.add_output(format!("File changed: {}", path.display()));
            app.add_output("Press Enter to run selected command".to_string());
        }
    }
}

fn setup_file_watcher(
    path: PathBuf,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    for path in event.paths {
                        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                            let _ = event_tx.send(AppEvent::FileChanged(path));
                            break;
                        }
                    }
                }
            }
        },
        Config::default(),
    )?;

    watcher.watch(&path, RecursiveMode::Recursive)?;
    Ok(watcher)
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup file watcher
    let (event_tx, event_rx) = mpsc::channel();
    let _watcher = setup_file_watcher(args.path.clone(), event_tx)?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Ensure terminal cleanup on panic or Ctrl+C
    let _cleanup = TerminalCleanup;

    // Create app
    let app = App::new(args.custom);

    // Run app
    let res = run_app(&mut terminal, app, event_rx).await;

    // Explicit cleanup (will also happen via Drop)
    drop(_cleanup);
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}
