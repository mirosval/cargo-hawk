use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use strip_ansi_escapes::strip;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, ListState, Paragraph, Tabs, Wrap},
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
            CargoCommand::Check => "cargo check",
            CargoCommand::Build => "cargo build",
            CargoCommand::Run => "cargo run",
            CargoCommand::Test => "cargo test",
            CargoCommand::Clippy => "cargo clippy",
            CargoCommand::Custom(cmd) => cmd,
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
        ];

        if let Some(cmd) = custom_command {
            commands.push(CargoCommand::Custom(cmd));
        }

        let mut selected = ListState::default();
        selected.select(Some(0));

        // Initialize command inputs with default command strings
        let command_inputs: Vec<String> = commands.iter().map(|c| c.as_display().to_string()).collect();
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
        let parts: Vec<String> = command_str.split_whitespace().map(|s| s.to_string()).collect();
        if parts.is_empty() {
            self.add_output("Error: Empty command".to_string());
            self.running = false;
            return;
        }

        let program = parts[0].clone();
        let args: Vec<String> = parts[1..].to_vec();
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

            // Strip ANSI escape sequences from output
            let stdout_stripped = strip(&output.stdout);
            let stderr_stripped = strip(&output.stderr);

            let stdout = String::from_utf8_lossy(&stdout_stripped).to_string();
            let stderr = String::from_utf8_lossy(&stderr_stripped).to_string();

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

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // Tab bar showing commands
    let selected_idx = app.selected.selected().unwrap_or(0);
    let tab_titles: Vec<String> = app.commands
        .iter()
        .enumerate()
        .map(|(idx, cmd)| format!(" {} {} ", idx + 1, cmd.as_display()))
        .collect();

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::NONE).title("Cargo Hawk"))
        .select(selected_idx)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow));
    frame.render_widget(tabs, chunks[0]);

    // Output panel (now full width)
    let output_height = chunks[1].height.saturating_sub(2) as usize;
    let start_line = app.scroll_offset.min(app.output.len().saturating_sub(output_height));
    let end_line = (start_line + output_height).min(app.output.len());

    let visible_output: Vec<Line> = app.output[start_line..end_line]
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect();

    let status_indicator = if app.running { " [RUNNING]" } else { "" };

    let output_panel = Paragraph::new(visible_output)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Output{}", status_indicator)),
        )
        .wrap(Wrap { trim: false })
        .scroll((0, 0));

    frame.render_widget(output_panel, chunks[1]);

    // Input field
    let input_text = &app.command_inputs[selected_idx];
    let input = Paragraph::new(input_text.as_str())
        .block(Block::default().borders(Borders::ALL).title("Command"))
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, chunks[2]);

    // Set cursor position in the input field
    frame.set_cursor_position((chunks[2].x + app.input_cursor as u16 + 1, chunks[2].y + 1));

    // Footer
    let footer = Paragraph::new("Ctrl+C: Quit | Enter: Run | Ctrl+R: Re-run | Alt+1-9: Switch tab | PgUp/PgDn: Scroll | ←/→/Home/End: Edit")
        .block(Block::default().borders(Borders::ALL))
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
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                            // Kill running task if any
                            app.cancel_running_command();
                            return Ok(())
                        }
                        KeyCode::Down if key.modifiers.is_empty() => app.next(),
                        KeyCode::Up if key.modifiers.is_empty() => app.previous(),
                        KeyCode::Enter => {
                            app.start_command();
                        }
                        KeyCode::Char('r') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                            app.start_command();
                        }
                        KeyCode::Char(c @ '1'..='9') if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
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
                        KeyCode::Backspace => app.input_delete_char(),
                        KeyCode::Left => app.input_move_cursor_left(),
                        KeyCode::Right => app.input_move_cursor_right(),
                        KeyCode::Home => app.input_move_cursor_home(),
                        KeyCode::End => app.input_move_cursor_end(),
                        KeyCode::Char(c) => app.input_insert_char(c),
                        _ => {}
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
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
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
