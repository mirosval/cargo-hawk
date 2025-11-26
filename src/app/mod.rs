use crate::{Tui, cli::Args};
use action::AppAction;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::MouseEventKind;
use model::DiagnosticDisplayMode;
use model::Plan;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::DebouncedEvent;
use notify_debouncer_mini::Debouncer;
use notify_debouncer_mini::new_debouncer_opt;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    prelude::Backend,
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Instant;
use tracing::debug;
use tracing::warn;
use tracing::{error, info};

mod action;
mod event;
mod model;
mod widgets;

pub use event::AppEvent;

type FileWatcher = Debouncer<RecommendedWatcher>;

fn setup_file_watcher(
    path: PathBuf,
    event_tx: UnboundedSender<AppEvent>,
    extensions: Vec<String>,
) -> Result<FileWatcher> {
    let notify_config = notify::Config::default();
    let config = notify_debouncer_mini::Config::default()
        .with_timeout(Duration::from_secs(1))
        .with_notify_config(notify_config);
    let mut debounced_watcher = new_debouncer_opt::<_, RecommendedWatcher>(
        config,
        move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| match res {
            Err(err) => {
                error!(?err, "error from file watcher");
            }
            Ok(events) => {
                if events.iter().any(|event| {
                    if let Some(ext) = event.path.extension() {
                        extensions.contains(&ext.to_string_lossy().to_string())
                    } else {
                        false
                    }
                }) {
                    debug!(num_events = events.len(), "files changed");
                    if let Err(err) = event_tx.send(AppEvent::FileChanged) {
                        error!(?err, "error sending FileChanged event");
                    }
                }
            }
        },
    )?;

    info!(?path, "watching path");
    let watcher = debounced_watcher.watcher();
    watcher.watch(&path, RecursiveMode::Recursive)?;
    Ok(debounced_watcher)
}

#[derive(Debug)]
pub struct App {
    event_rx: UnboundedReceiver<AppEvent>,
    event_tx: UnboundedSender<AppEvent>,
    _file_watcher: FileWatcher,
    plan: Plan,
    scroll_offset: u16,
    input_cursor: usize,
    input_focused: bool,
    auto_mode: bool,
    diagnostic_display_mode: DiagnosticDisplayMode,
    should_quit: bool,
}

impl App {
    pub fn new(args: Args) -> Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        let plan = Plan::from_string(
            r#"cargo check
            cargo test
            cargo clippy"#,
        );

        // TODO: Decouple this
        let file_watcher =
            setup_file_watcher(args.path.clone(), event_tx.clone(), vec!["rs".to_string()])?;
        Ok(Self {
            event_rx,
            event_tx,
            _file_watcher: file_watcher,
            plan,
            scroll_offset: 0,
            input_cursor: 0,
            input_focused: false,
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

            let start = Instant::now();
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
            let event_handling_duration = Instant::now() - start;
            if event_handling_duration > Duration::from_millis(50) {
                warn!(?event_handling_duration, "event loop slow");
            }
            if self.should_quit {
                break;
            }
        }

        tui.stop()?;
        tui.exit()?;
        Ok(())
    }

    fn ui(&mut self, frame: &mut Frame) {
        if self.auto_mode {
            self.plan.select_best_step_to_present_idx();
        }

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
        match event {
            AppEvent::Key(key) => {
                debug!(?key, "key pressed");
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
                            self.should_quit = true;
                            None
                        }
                        KeyCode::Char('i') => {
                            self.input_focused = true;
                            None
                        }
                        KeyCode::Char('a') => Some(AppAction::ToggleAuto),
                        KeyCode::Char('p') => Some(AppAction::EditPlan),
                        KeyCode::Down | KeyCode::Char('j') => Some(AppAction::ScrollDown),
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
            AppEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => Some(AppAction::ScrollDown),
                MouseEventKind::ScrollUp => Some(AppAction::ScrollUp),
                _ => None,
            },
            AppEvent::Resize(_, _) => None,
            AppEvent::FocusLost => None,
            AppEvent::FocusGained => None,
            AppEvent::Paste(_) => None,
            AppEvent::Error => None,
            AppEvent::Tick => {
                // Check if running command has completed
                let start = Instant::now();
                self.check_command_completion().await;
                let check_command_duration = Instant::now() - start;
                if check_command_duration > Duration::from_millis(30) {
                    debug!(?check_command_duration, "check command");
                }
                None
            }
            AppEvent::Render => None,
            AppEvent::FileChanged => {
                self.plan.reset();
                self.start_command();
                None
            }
        }
    }

    async fn update(&mut self, action: AppAction) -> Option<AppAction> {
        debug!(?action, "update action");
        match action {
            AppAction::CancelCommand => {
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
        self.plan.len()
    }

    fn switch_to_tab(&mut self, idx: usize) -> Option<AppAction> {
        self.plan.reset();
        if idx < self.total_steps() {
            self.plan.select(idx);
            Some(AppAction::StartCommand)
        } else {
            None
        }
    }

    fn cycle_diagnostic_mode(&mut self) -> Option<AppAction> {
        self.diagnostic_display_mode = self.diagnostic_display_mode.next();
        None
    }

    fn start_plan_editing<B: Backend>(&mut self, tui: &mut Tui<B>) -> Result<()> {
        tui.stop()?;
        tui.exit()?;

        let edited_plan = edit::edit(self.plan.as_text().trim()).unwrap();

        self.plan = Plan::from_string(&edited_plan);

        self.start_command();

        tui.enter()?;
        tui.clear()?;
        tui.start()?;

        Ok(())
    }

    fn update_cursor_for_current_input(&mut self) {
        self.input_cursor = self.plan.current_step_len();
    }

    fn input_insert_char(&mut self, c: char) {
        let mut current_cmd = self.plan.selected().cmd.to_string();
        current_cmd.insert(self.input_cursor, c);
        self.plan.set_current_command(current_cmd);
        self.input_cursor += 1;
    }

    fn input_delete_char(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let mut current_cmd = self.plan.selected().cmd.to_string();
        current_cmd.remove(self.input_cursor - 1);
        self.plan.set_current_command(current_cmd);
        self.input_cursor -= 1;
    }

    fn input_move_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
        }
    }

    fn input_move_cursor_right(&mut self) {
        if self.input_cursor < self.plan.current_step_len() {
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
        self.start_command();
        None
    }

    fn clear_output(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn start_command(&mut self) -> Option<AppAction> {
        self.clear_output();
        self.plan.start_selected();
        None
    }

    async fn check_command_completion(&mut self) {
        self.plan.check().await;
        if self.auto_mode && !self.plan.is_running() {
            self.plan.advance();
            if !self.plan.is_finished() {
                self.start_command();
            }
        }
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
        let app = App::new(Args::default())?;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }

    #[test]
    fn test_auto_disabled() -> TestResult {
        let mut app = App::new(Args::default())?;
        app.auto_mode = false;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }
}
