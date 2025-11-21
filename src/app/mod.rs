use action::AppAction;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use model::DiagnosticDisplayMode;
use model::Plan;
use model::PlanStep;
use model::cargo::CargoMessage;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    prelude::Backend,
    widgets::ListState,
};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    process::Command,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tracing::Level;

use crate::app::model::OutputLine;
use crate::app::model::PlanStepExecution;
use crate::app::model::cargo::DiagnosticLevel;
use crate::trace_dbg;
use crate::{Tui, cli::Args};

mod action;
mod event;
mod model;
mod widgets;

pub use event::AppEvent;

fn setup_file_watcher(
    path: PathBuf,
    event_tx: UnboundedSender<AppEvent>,
) -> Result<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res
                && (event.kind.is_modify() || event.kind.is_create())
            {
                for path in event.paths {
                    if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                        let _ = event_tx.send(AppEvent::FileChanged(path));
                        break;
                    }
                }
            }
        },
        Config::default(),
    )?;

    watcher.watch(&path, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[derive(Debug)]
pub struct CommandResult {
    stdout: String,
    stderr: String,
    success: bool,
}

#[derive(Debug)]
pub struct App {
    event_rx: UnboundedReceiver<AppEvent>,
    event_tx: UnboundedSender<AppEvent>,
    _file_watcher: RecommendedWatcher,
    plan: Plan,
    selected: ListState,
    last_file_changed: Option<String>,
    scroll_offset: u16,
    input_cursor: usize,
    input_focused: bool,
    cargo_messages: Vec<CargoMessage>,
    first_diagnostic_shown: bool,
    auto_mode: bool,
    diagnostic_display_mode: DiagnosticDisplayMode,
    should_quit: bool,
}

impl App {
    pub fn new(args: Args) -> Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        let plan = Plan {
            commands: vec![
                PlanStep {
                    name: "check".to_string(),
                    cmd: "cargo check".to_string(),
                    exec: PlanStepExecution::NotRun,
                },
                PlanStep {
                    name: "test".to_string(),
                    cmd: "cargo test".to_string(),
                    exec: PlanStepExecution::NotRun,
                },
                PlanStep {
                    name: "clippy".to_string(),
                    cmd: "cargo clippy".to_string(),
                    exec: PlanStepExecution::NotRun,
                },
            ],
        };

        let mut selected = ListState::default();
        selected.select(Some(0));

        let file_watcher = setup_file_watcher(args.path.clone(), event_tx.clone())?;
        Ok(Self {
            event_rx,
            event_tx,
            _file_watcher: file_watcher,
            plan,
            selected,
            last_file_changed: None,
            scroll_offset: 0,
            input_cursor: 0,
            input_focused: false,
            cargo_messages: Vec::new(),
            first_diagnostic_shown: false,
            auto_mode: true,
            diagnostic_display_mode: DiagnosticDisplayMode::First,
            should_quit: false,
        })
    }

    pub fn event_tx(&self) -> UnboundedSender<AppEvent> {
        self.event_tx.clone()
    }

    pub async fn run<B: Backend>(&mut self, tui: &mut Tui<B>) -> Result<()> {
        tui.enter()?;
        tui.start()?;

        loop {
            tui.draw(|f| {
                self.ui(f);
            })?;

            if let Some(event) = self.event_rx.recv().await {
                let mut maybe_action = self.handle_event(event).await;
                while let Some(action) = maybe_action {
                    maybe_action = self.update(action).await;
                    if let Some(AppAction::EditPlan) = maybe_action {
                        self.start_plan_editing(tui)?;
                        maybe_action = None;
                    }
                }
            };
            if self.should_quit {
                break;
            }
        }

        tui.stop()?;
        tui.exit()?;
        Ok(())
    }

    fn ui(&mut self, frame: &mut Frame) {
        // Render the main app widget
        frame.render_widget(&*self, frame.area());

        // Handle cursor positioning (can't be done in Widget trait)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(frame.area());

        // Set cursor position in the input field (only visible when focused)
        if self.input_focused {
            frame
                .set_cursor_position((chunks[2].x + self.input_cursor as u16 + 1, chunks[2].y + 1));
        }
    }

    async fn handle_event(&mut self, event: AppEvent) -> Option<AppAction> {
        // Check if running command has completed
        self.check_command_completion().await;
        match event {
            AppEvent::Key(key) => {
                // Ctrl+C always quits
                if key.code == KeyCode::Char('c')
                    && key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                {
                    Some(AppAction::CancelCommand)
                } else if self.input_focused {
                    // FOCUSED MODE - editing command input
                    match key.code {
                        KeyCode::Esc => Some(AppAction::ExitCommandEditMode),
                        KeyCode::Enter => Some(AppAction::EnterCommandEditMode),
                        KeyCode::Backspace => Some(AppAction::CommandEditModeBackspace),
                        KeyCode::Left => Some(AppAction::CommandEditModeLeft),
                        KeyCode::Right => Some(AppAction::CommandEditModeRight),
                        KeyCode::Char(c) => Some(AppAction::CommandEditModeChar(c)),
                        _ => None,
                    }
                } else {
                    // UNFOCUSED MODE - navigation
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            self.cancel_running_command();
                            self.should_quit = true;
                            None
                        }
                        KeyCode::Char('i') => {
                            self.input_focused = true;
                            None
                        }
                        KeyCode::Char('a') => Some(AppAction::ToggleAuto),
                        KeyCode::Char('p') => Some(AppAction::EditPlan),
                        KeyCode::Down | KeyCode::Char('j') => Some(AppAction::ScrollUp),
                        KeyCode::Up | KeyCode::Char('k') => Some(AppAction::ScrollUp),
                        KeyCode::Enter | KeyCode::Char('r') => Some(AppAction::ToggleAuto),
                        KeyCode::Char(c @ '1'..='9') => {
                            let idx = c.to_digit(10).unwrap() as usize - 1;
                            Some(AppAction::SwitchTab(idx))
                        }
                        KeyCode::Char('s') => Some(AppAction::CycleDiagnosticMode),
                        _ => None,
                    }
                }
            }
            AppEvent::Init => Some(AppAction::StartCommand),
            AppEvent::Mouse(_) => None,
            AppEvent::Resize(_, _) => None,
            AppEvent::FocusLost => None,
            AppEvent::FocusGained => None,
            AppEvent::Paste(_) => None,
            AppEvent::Error => None,
            AppEvent::Tick => None,
            AppEvent::Render => None,
            AppEvent::FileChanged(path) => {
                self.last_file_changed = Some(path.display().to_string());
                self.reset_all_steps();

                // If auto mode is on, start from the first step in the plan
                if self.auto_mode
                    && let Some(first_step_idx) = self.get_first_step_in_plan()
                {
                    self.selected.select(Some(first_step_idx));
                }

                self.start_command();
                None
            }
        }
    }

    async fn update(&mut self, action: AppAction) -> Option<AppAction> {
        match action {
            AppAction::CancelCommand => {
                self.cancel_running_command();
                self.should_quit = true;
                None
            }
            AppAction::EditPlan => Some(AppAction::EditPlan),
            AppAction::StartCommand => self.start_command(),
            AppAction::ScrollUp => self.scroll_up(),
            AppAction::ScrollDown => self.scroll_down(),
            AppAction::ToggleAuto => self.toggle_auto(),
            AppAction::SwitchTab(idx) => self.switch_to_tab(idx),
            AppAction::CycleDiagnosticMode => self.cycle_diagnostic_mode(),
            AppAction::EnterCommandEditMode => {
                self.update_cursor_for_current_input();
                self.input_focused = true;
                None
            }
            AppAction::ExitCommandEditMode => {
                self.input_focused = false;
                Some(AppAction::StartCommand)
            }
            AppAction::CommandEditModeBackspace => {
                self.input_delete_char();
                None
            }
            AppAction::CommandEditModeLeft => {
                self.input_move_cursor_left();
                None
            }
            AppAction::CommandEditModeRight => {
                self.input_move_cursor_right();
                None
            }
            AppAction::CommandEditModeChar(c) => {
                self.input_insert_char(c);
                None
            }
        }
    }

    fn total_steps(&self) -> usize {
        self.plan.commands.len()
    }

    fn get_selected_step_mut(&mut self) -> Option<&mut PlanStep> {
        let selected_idx = self.selected.selected()?;
        self.plan.commands.get_mut(selected_idx)
    }

    fn reset_all_steps(&mut self) {
        for step in &mut self.plan.commands {
            step.exec = PlanStepExecution::NotRun;
        }
    }

    fn get_next_step(&self) -> Option<usize> {
        let next = self.selected.selected()? + 1;
        self.plan.commands.get(next).map(|_| next)
    }

    fn get_first_non_successful_step(&self) -> Option<usize> {
        for (step_idx, step) in self.plan.commands.iter().enumerate() {
            if !matches!(step.exec, PlanStepExecution::Success { .. }) {
                return Some(step_idx);
            }
        }
        None
    }

    fn get_first_step_in_plan(&self) -> Option<usize> {
        if !self.plan.commands.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    fn switch_to_tab(&mut self, idx: usize) -> Option<AppAction> {
        if idx < self.total_steps() {
            self.selected.select(Some(idx));
            Some(AppAction::StartCommand)
        } else {
            None
        }
    }

    fn cycle_diagnostic_mode(&mut self) -> Option<AppAction> {
        self.diagnostic_display_mode = self.diagnostic_display_mode.next();
        Some(AppAction::StartCommand)
    }

    fn start_plan_editing<B: Backend>(&mut self, tui: &mut Tui<B>) -> Result<()> {
        tui.stop()?;
        tui.exit()?;

        let plan_text = self
            .plan
            .commands
            .iter()
            .map(|step| step.cmd.to_string())
            .fold(String::new(), |acc, s| acc + "\n" + &s);

        let edited_plan = edit::edit(plan_text.trim()).unwrap();

        self.plan = Plan::from_string(&edited_plan);
        if let Some(first_step_idx) = self.get_first_step_in_plan() {
            self.selected.select(Some(first_step_idx));

            if self.auto_mode {
                self.start_command();
            }
        }

        tui.enter()?;
        tui.clear()?;
        tui.start()?;

        Ok(())
    }

    fn update_cursor_for_current_input(&mut self) {
        let idx = self.selected.selected().unwrap_or(0);
        self.input_cursor = self.plan.commands[idx].cmd.len();
    }

    fn input_insert_char(&mut self, c: char) {
        let idx = self.selected.selected().unwrap_or(0);
        self.plan.commands[idx].cmd.insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    fn input_delete_char(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let idx = self.selected.selected().unwrap_or(0);
        self.plan.commands[idx].cmd.remove(self.input_cursor - 1);
        self.input_cursor -= 1;
    }

    fn input_move_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
        }
    }

    fn input_move_cursor_right(&mut self) {
        let idx = self.selected.selected().unwrap_or(0);
        if self.input_cursor < self.plan.commands[idx].cmd.len() {
            self.input_cursor += 1;
        }
    }

    fn scroll_up(&mut self) -> Option<AppAction> {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
        None
    }

    fn scroll_down(&mut self) -> Option<AppAction> {
        self.scroll_offset += 1;
        None
    }

    fn toggle_auto(&mut self) -> Option<AppAction> {
        self.auto_mode = !self.auto_mode;
        // If turning on auto mode, start from first non-successful step
        if self.auto_mode
            && let Some(first_step_idx) = self.get_first_non_successful_step()
        {
            self.selected.select(Some(first_step_idx));
            self.start_command();
            // Note: auto mode stays on even if all steps are already successful
        }
        None
    }

    fn clear_output(&mut self) {
        self.cargo_messages.clear();
        self.scroll_offset = 0;
        self.first_diagnostic_shown = false;
    }

    fn parse_output_line(line: &str) -> OutputLine {
        // Try to parse as JSON cargo message
        if let Some(msg) = CargoMessage::parse(line) {
            OutputLine::Cargo(msg)
        } else {
            OutputLine::Other(line.to_string())
        }
    }

    pub fn start_command(&mut self) -> Option<AppAction> {
        trace_dbg!("start command");
        let selected_idx = self.selected.selected().unwrap_or(0);
        let command_str = self.plan.commands[selected_idx].cmd.clone();

        self.clear_output();

        // Parse the command string into program and args
        let parts: Vec<String> = command_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

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

        // Spawn the command execution as a background task
        let task = tokio::spawn(async move {
            let child = Command::new(&program)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;

            let output = child.wait_with_output().await?;

            // Keep ANSI color codes in the output for colored display
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            Ok(CommandResult {
                stdout,
                stderr,
                success: output.status.success(),
            })
        });

        if let Some(step) = self.get_selected_step_mut() {
            step.exec = PlanStepExecution::Running { task: Some(task) }
        }
        None
    }

    async fn check_command_completion(&mut self) {
        let task = self.plan.commands.iter_mut().find_map(|step| {
            if let PlanStepExecution::Running { task } = &mut step.exec
                && let Some(handle) = task
                && handle.is_finished()
            {
                task.take()
            } else {
                None
            }
        });

        if let Some(task) = task {
            // Get the result (this won't block since it's finished)
            let final_status = match task.await {
                Ok(Ok(result)) => {
                    trace_dbg!("command completed");
                    let lines: Vec<OutputLine> = result
                        .stdout
                        .lines()
                        .map(Self::parse_output_line)
                        .chain(result.stderr.lines().map(Self::parse_output_line))
                        .collect();

                    let n_warnings = lines
                        .iter()
                        .map(|line| match line {
                            OutputLine::Cargo(cargo_message) => match cargo_message {
                                CargoMessage::CompilerMessage { message, target: _ }
                                    if message.level == DiagnosticLevel::Warning =>
                                {
                                    1
                                }
                                _ => 0,
                            },
                            OutputLine::Other(_) => 0,
                        })
                        .sum();

                    let n_errors = lines
                        .iter()
                        .map(|line| match line {
                            OutputLine::Cargo(cargo_message) => match cargo_message {
                                CargoMessage::CompilerMessage { message, target: _ }
                                    if message.level == DiagnosticLevel::Error =>
                                {
                                    1
                                }
                                _ => 0,
                            },
                            OutputLine::Other(_) => 0,
                        })
                        .sum();

                    match (n_warnings, n_errors, result.success) {
                        (0, 0, true) => PlanStepExecution::Success { output: lines },
                        (0, 0, false) => PlanStepExecution::Error { output: lines },
                        (1.., 0, _) => PlanStepExecution::Warning {
                            warnings: n_warnings,
                            output: lines,
                        },
                        (_, 1.., _) => PlanStepExecution::Failure {
                            warnings: n_warnings,
                            failures: n_errors,
                            output: lines,
                        },
                    }
                }
                Ok(Err(e)) => {
                    trace_dbg!(level: Level::ERROR, &e);
                    let output = vec![OutputLine::Other(format!("Error: {}", e))];
                    PlanStepExecution::Error { output }
                }
                Err(e) => {
                    trace_dbg!(level: Level::ERROR, &e);
                    let output = if e.is_cancelled() {
                        vec![OutputLine::Other("Command was cancelled".to_string())]
                    } else {
                        vec![OutputLine::Other(format!("Task error: {}", e))]
                    };
                    PlanStepExecution::Error { output }
                }
            };

            let should_continue = matches!(
                final_status,
                PlanStepExecution::Success { .. } | PlanStepExecution::Warning { .. }
            );

            // Update the status of the selected step
            if let Some(step) = self.get_selected_step_mut() {
                step.exec = final_status;
            }

            // Auto-advance to next step if in auto mode and current step succeeded or has warnings
            if self.auto_mode && should_continue {
                if let Some(next_step_idx) = self.get_next_step() {
                    self.selected.select(Some(next_step_idx));
                    self.start_command();
                } else if let Some(next_step_idx) = self.get_first_non_successful_step() {
                    // Note: when we reached the end and there are still warnings, switch to them
                    self.selected.select(Some(next_step_idx))
                }
            }
        }
    }

    fn cancel_running_command(&mut self) {
        self.plan.commands.iter_mut().for_each(|step| {
            if let PlanStepExecution::Running { task } = &mut step.exec
                && let Some(handle) = task
            {
                handle.abort();
                step.exec = PlanStepExecution::NotRun;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};
    use testresult::TestResult;

    use super::*;

    #[test]
    fn test_first_screen() -> TestResult {
        let app = App::new(Args {
            path: PathBuf::from("."),
            verbose: false,
        })?;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }

    #[test]
    fn test_auto_disabled() -> TestResult {
        let mut app = App::new(Args {
            path: PathBuf::from("."),
            verbose: false,
        })?;
        app.auto_mode = false;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }
}
