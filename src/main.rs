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
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, ListState, Paragraph, Widget, Wrap},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
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
enum Status {
    NotRun,
    Running,
    Warning,
    Failure,
    Success,
}

#[derive(Debug, Clone, PartialEq)]
enum DiagnosticDisplayMode {
    Summary,
    First,
    Full,
}

impl DiagnosticDisplayMode {
    fn next(&self) -> Self {
        match self {
            DiagnosticDisplayMode::Summary => DiagnosticDisplayMode::First,
            DiagnosticDisplayMode::First => DiagnosticDisplayMode::Full,
            DiagnosticDisplayMode::Full => DiagnosticDisplayMode::Summary,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            DiagnosticDisplayMode::Summary => "Summary",
            DiagnosticDisplayMode::First => "First",
            DiagnosticDisplayMode::Full => "Full",
        }
    }
}

impl Display for DiagnosticDisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())?;
        Ok(())
    }
}

impl Default for DiagnosticDisplayMode {
    fn default() -> Self {
        Self::First
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PlanStep {
    name: String,
    cmd: String,
    status: Status,
}

#[derive(Debug, Clone, Default)]
struct Plan {
    name: String,
    commands: Vec<PlanStep>,
}

impl Plan {
    fn from_string(s: &str) -> Plan {
        let steps: Vec<PlanStep> = s
            .split("\n")
            .into_iter()
            .filter(|step| step.trim() != "")
            .map(|cmd| {
                let name = cmd
                    .strip_prefix("cargo ")
                    .and_then(|s| s.split_whitespace().next())
                    .unwrap_or("custom")
                    .to_string();
                PlanStep {
                    name,
                    cmd: cmd.to_string(),
                    status: Status::NotRun,
                }
            })
            .collect();
        Plan {
            name: "default".to_string(),
            commands: steps,
        }
    }
}

#[derive(Debug)]
struct App {
    plans: Vec<Plan>,
    current_plan: Plan,
    selected: ListState,
    output: Vec<String>,
    last_file_changed: Option<String>,
    running: bool,
    scroll_offset: u16,
    command_inputs: Vec<String>,
    input_cursor: usize,
    running_task: Option<JoinHandle<Result<CommandResult>>>,
    running_child: Arc<Mutex<Option<Child>>>,
    input_focused: bool,
    cargo_messages: Vec<CargoMessage>,
    first_diagnostic_shown: bool,
    auto_mode: bool,
    plan_editing: bool,
    plan_edit_text: Vec<String>,
    plan_edit_cursor_line: usize,
    plan_edit_cursor_col: usize,
    diagnostic_display_mode: DiagnosticDisplayMode,
}

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        // Top line: tabs on left, "Cargo Hawk" on right, with dark background
        let selected_idx = self.selected.selected().unwrap_or(0);
        let selected_color = get_tab_color(selected_idx);

        let mut tab_spans = vec![];

        // Add auto mode indicator if enabled
        if self.auto_mode {
            tab_spans.push(Span::styled(
                " » ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let mut step_idx = 0;
        for plan in &self.plans {
            for step in &plan.commands {
                let status_indicator = get_status_indicator(&step.status);
                let tab_text = if status_indicator.is_empty() {
                    format!(" {} {} ", step_idx + 1, step.name)
                } else {
                    format!(" {} {} {} ", step_idx + 1, step.name, status_indicator)
                };
                let tab_color = get_tab_color(step_idx);
                let style = if step_idx == selected_idx {
                    Style::default()
                        .fg(Color::Black)
                        .bg(tab_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(tab_color).bg(Color::DarkGray)
                };
                tab_spans.push(Span::styled(tab_text, style));
                step_idx += 1;
            }
        }

        let tab_line = Line::from(tab_spans);
        let tabs_widget = Paragraph::new(tab_line).style(Style::default().bg(Color::DarkGray));
        tabs_widget.render(chunks[0], buf);

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
        title.render(title_area, buf);

        // Colored separator line using upper half block character (▀) to touch the tab bar
        let separator_line = "▀".repeat(chunks[1].width as usize);
        let separator = Paragraph::new(separator_line).style(Style::default().fg(selected_color));
        let separator_area = ratatui::layout::Rect {
            x: chunks[1].x,
            y: chunks[0].y + chunks[0].height,
            width: chunks[1].width,
            height: 1,
        };
        separator.render(separator_area, buf);

        // Output panel - positioned right after separator
        let output_area = ratatui::layout::Rect {
            x: chunks[1].x,
            y: chunks[0].y + chunks[0].height + 1,
            width: chunks[1].width,
            height: chunks[1].height.saturating_sub(1),
        };

        // Parse ANSI color codes in the output
        let visible_output: Vec<Line> = self
            .output
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
            .scroll((self.scroll_offset, 0));

        output_panel.render(output_area, buf);

        // Input field with focus indicator
        let input_text = &self.command_inputs[selected_idx];
        let (input_style, input_title) = if self.input_focused {
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
        input.render(chunks[2], buf);

        // Footer with mode-specific shortcuts
        let footer_text = if self.input_focused {
            format!("Esc: Exit edit mode | Enter: Run | ←/→/Home/End: Navigate | Ctrl+C: Quit")
        } else {
            format!(
                "q/Esc: Quit | i: Edit command | p: Edit plan | Enter/r: Run | a: Auto advance | 1-9: Switch tab | j/k/↑/↓: Scroll | s: Diag ({diag})",
                diag = self.diagnostic_display_mode
            )
        };
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        footer.render(chunks[3], buf);

        // Render plan editing modal if active
        if self.plan_editing {
            // Calculate 80% of screen dimensions
            let modal_width = (area.width as f32 * 0.8) as u16;
            let modal_height = (area.height as f32 * 0.8) as u16;
            let modal_x = (area.width - modal_width) / 2;
            let modal_y = (area.height - modal_height) / 2;

            let modal_area = ratatui::layout::Rect {
                x: modal_x,
                y: modal_y,
                width: modal_width,
                height: modal_height,
            };

            // Clear the background area to prevent transparency
            Clear.render(modal_area, buf);

            // Render modal background/border
            let modal_block = Block::default()
                .title("Edit Plan")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::Black));

            // Get inner area for text
            let inner_area = modal_block.inner(modal_area);
            modal_block.render(modal_area, buf);

            // Render text content
            let text_lines: Vec<Line> = self
                .plan_edit_text
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let style = if i == self.plan_edit_cursor_line {
                        Style::default().fg(Color::White).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line.clone(), style))
                })
                .collect();

            let text_widget = Paragraph::new(text_lines)
                .block(Block::default().borders(Borders::NONE))
                .wrap(Wrap { trim: false });

            text_widget.render(inner_area, buf);

            // Render help text at bottom of modal
            let help_text = "Ctrl+S: Save | Esc: Cancel";
            let help_area = ratatui::layout::Rect {
                x: modal_area.x + 2,
                y: modal_area.y + modal_area.height - 2,
                width: modal_area.width - 4,
                height: 1,
            };
            let help_widget = Paragraph::new(help_text).style(Style::default().fg(Color::Gray));
            help_widget.render(help_area, buf);
        }
    }
}

#[derive(Debug)]
struct CommandResult {
    stdout: String,
    stderr: String,
    success: bool,
    exit_code: Option<i32>,
}

// Cargo JSON message format models
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum CargoMessage {
    CompilerMessage {
        message: DiagnosticMessage,
        #[serde(default)]
        target: Option<Target>,
    },
    CompilerArtifact {
        #[serde(default)]
        target: Option<Target>,
        #[serde(default)]
        filenames: Vec<String>,
        #[serde(default)]
        fresh: bool,
    },
    BuildScriptExecuted {
        #[serde(default)]
        package_id: String,
    },
    BuildFinished {
        success: bool,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DiagnosticMessage {
    /// The primary message (e.g., "unused variable: `x`")
    message: String,
    /// The level: "error", "warning", "note", "help", etc.
    level: String,
    /// ANSI-formatted rendered output
    #[serde(default)]
    rendered: Option<String>,
    /// Spans showing where in the code the issue is
    #[serde(default)]
    spans: Vec<DiagnosticSpan>,
    /// Child diagnostics (notes, help messages, etc.)
    #[serde(default)]
    children: Vec<DiagnosticMessage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DiagnosticSpan {
    /// File path where the diagnostic points
    #[serde(default)]
    file_name: String,
    /// Line number (1-indexed)
    #[serde(default)]
    line_start: usize,
    /// Column number (1-indexed)
    #[serde(default)]
    column_start: usize,
    /// Whether this is the primary span
    #[serde(default)]
    is_primary: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Target {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: Vec<String>,
}

impl App {
    fn new(custom_command: Option<String>) -> Self {
        let current_plan = Plan {
            name: "default".to_string(),
            commands: vec![
                PlanStep {
                    name: "check".to_string(),
                    cmd: "cargo check".to_string(),
                    status: Status::NotRun,
                },
                PlanStep {
                    name: "test".to_string(),
                    cmd: "cargo test".to_string(),
                    status: Status::NotRun,
                },
                PlanStep {
                    name: "clippy".to_string(),
                    cmd: "cargo clippy".to_string(),
                    status: Status::NotRun,
                },
            ],
        };
        let mut plans = vec![current_plan.clone()];

        if let Some(cmd) = custom_command {
            // Extract name from command (word after "cargo" if present)
            let name = cmd
                .strip_prefix("cargo ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("custom")
                .to_string();

            plans.push(Plan {
                name: "custom".to_string(),
                commands: vec![PlanStep {
                    name,
                    cmd,
                    status: Status::NotRun,
                }],
            });
        }

        let mut selected = ListState::default();
        selected.select(Some(0));

        // Initialize command inputs from all plan steps (flattened)
        let command_inputs: Vec<String> = plans
            .iter()
            .flat_map(|p| p.commands.iter().map(|step| step.cmd.clone()))
            .collect();
        let input_cursor = if !command_inputs.is_empty() {
            command_inputs[0].len()
        } else {
            0
        };

        Self {
            plans,
            current_plan,
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
            cargo_messages: Vec::new(),
            first_diagnostic_shown: false,
            auto_mode: true,
            plan_editing: false,
            plan_edit_text: Vec::new(),
            plan_edit_cursor_line: 0,
            plan_edit_cursor_col: 0,
            diagnostic_display_mode: DiagnosticDisplayMode::First,
        }
    }

    fn total_steps(&self) -> usize {
        self.plans.iter().map(|p| p.commands.len()).sum()
    }

    fn get_selected_step_mut(&mut self) -> Option<&mut PlanStep> {
        let selected_idx = self.selected.selected()?;
        let mut current_idx = 0;
        for plan in &mut self.plans {
            for step in &mut plan.commands {
                if current_idx == selected_idx {
                    return Some(step);
                }
                current_idx += 1;
            }
        }
        None
    }

    fn reset_all_steps(&mut self) {
        for plan in &mut self.plans {
            for step in &mut plan.commands {
                step.status = Status::NotRun;
            }
        }
    }

    fn get_plan_and_step_index(&self, global_idx: usize) -> Option<(usize, usize)> {
        let mut current_idx = 0;
        for (plan_idx, plan) in self.plans.iter().enumerate() {
            for step_idx in 0..plan.commands.len() {
                if current_idx == global_idx {
                    return Some((plan_idx, step_idx));
                }
                current_idx += 1;
            }
        }
        None
    }

    fn get_next_step_in_plan(&self) -> Option<usize> {
        let selected_idx = self.selected.selected()?;
        let (plan_idx, step_idx) = self.get_plan_and_step_index(selected_idx)?;

        // Check if there's a next step in the same plan
        if step_idx + 1 < self.plans[plan_idx].commands.len() {
            // Calculate the global index of the next step
            let mut global_idx = 0;
            for (p_idx, plan) in self.plans.iter().enumerate() {
                if p_idx < plan_idx {
                    global_idx += plan.commands.len();
                } else if p_idx == plan_idx {
                    global_idx += step_idx + 1;
                    break;
                }
            }
            Some(global_idx)
        } else {
            None
        }
    }

    fn get_first_non_successful_step_in_plan(&self) -> Option<usize> {
        let selected_idx = self.selected.selected()?;
        let (plan_idx, _) = self.get_plan_and_step_index(selected_idx)?;

        // Find the first step in the current plan that is not successful
        let plan = &self.plans[plan_idx];
        for (step_idx, step) in plan.commands.iter().enumerate() {
            if step.status != Status::Success {
                // Calculate the global index of this step
                let mut global_idx = 0;
                for (p_idx, p) in self.plans.iter().enumerate() {
                    if p_idx < plan_idx {
                        global_idx += p.commands.len();
                    } else if p_idx == plan_idx {
                        global_idx += step_idx;
                        break;
                    }
                }
                return Some(global_idx);
            }
        }
        None
    }

    fn get_first_step_in_plan(&self) -> Option<usize> {
        let selected_idx = self.selected.selected()?;
        let (plan_idx, _) = self.get_plan_and_step_index(selected_idx)?;

        // Calculate the global index of the first step in the current plan
        let mut global_idx = 0;
        for (p_idx, p) in self.plans.iter().enumerate() {
            if p_idx < plan_idx {
                global_idx += p.commands.len();
            } else if p_idx == plan_idx {
                break;
            }
        }
        Some(global_idx)
    }

    fn switch_to_tab(&mut self, idx: usize) {
        if !self.running && idx < self.total_steps() {
            self.selected.select(Some(idx));
            self.update_cursor_for_current_input();
            self.start_command();
        }
    }

    fn start_plan_editing<B: Backend>(&mut self, terminal: &mut Terminal<B>) {
        crossterm::execute!(std::io::stdout(), LeaveAlternateScreen);

        disable_raw_mode().expect("disable_raw_mode");
        let plan_text = self
            .current_plan
            .commands
            .iter()
            .map(|step| step.cmd.to_string())
            .fold(String::new(), |acc, s| acc + "\n" + &s);

        let edited_plan = edit::edit(plan_text.trim()).unwrap();
        let new_plan = Plan::from_string(&edited_plan);
        crossterm::execute!(std::io::stdout(), EnterAlternateScreen);
        enable_raw_mode().unwrap();
        terminal.clear();
        self.current_plan = new_plan.clone();
        self.plans = vec![new_plan];
        // Rebuild command_inputs for all plans
        self.command_inputs = self
            .plans
            .iter()
            .flat_map(|p| p.commands.iter().map(|step| step.cmd.clone()))
            .collect();
        if let Some(first_step_idx) = self.get_first_step_in_plan() {
            self.selected.select(Some(first_step_idx));
            self.update_cursor_for_current_input();

            if self.auto_mode {
                self.start_command();
            }
        }
    }

    fn save_plan_edits(&mut self) {
        let selected_idx = self.selected.selected().unwrap_or(0);
        if let Some((plan_idx, _)) = self.get_plan_and_step_index(selected_idx) {
            // Parse edited text into new PlanSteps
            let new_steps: Vec<PlanStep> = self
                .plan_edit_text
                .iter()
                .filter(|line| !line.trim().is_empty())
                .map(|cmd| {
                    // Extract name from command (word after "cargo" if present)
                    let name = cmd
                        .strip_prefix("cargo ")
                        .and_then(|s| s.split_whitespace().next())
                        .unwrap_or("custom")
                        .to_string();

                    PlanStep {
                        name,
                        cmd: cmd.clone(),
                        status: Status::NotRun,
                    }
                })
                .collect();

            // Update the plan
            if !new_steps.is_empty() {
                self.plans[plan_idx].commands = new_steps;

                // Rebuild command_inputs for all plans
                self.command_inputs = self
                    .plans
                    .iter()
                    .flat_map(|p| p.commands.iter().map(|step| step.cmd.clone()))
                    .collect();

                // Reset selection to first step of this plan
                if let Some(first_step_idx) = self.get_first_step_in_plan() {
                    self.selected.select(Some(first_step_idx));
                    self.update_cursor_for_current_input();

                    // If auto mode is on, start running from the first step
                    if self.auto_mode {
                        self.plan_editing = false;
                        self.start_command();
                        return;
                    }
                }
            }
        }
        self.plan_editing = false;
    }

    fn cancel_plan_editing(&mut self) {
        self.plan_editing = false;
        self.plan_edit_text.clear();
    }

    fn plan_edit_insert_char(&mut self, c: char) {
        if self.plan_edit_text.is_empty() {
            self.plan_edit_text.push(String::new());
        }
        let line = &mut self.plan_edit_text[self.plan_edit_cursor_line];
        line.insert(self.plan_edit_cursor_col, c);
        self.plan_edit_cursor_col += 1;
    }

    fn plan_edit_delete_char(&mut self) {
        if self.plan_edit_cursor_col > 0 {
            let line = &mut self.plan_edit_text[self.plan_edit_cursor_line];
            line.remove(self.plan_edit_cursor_col - 1);
            self.plan_edit_cursor_col -= 1;
        } else if self.plan_edit_cursor_line > 0 {
            // Join with previous line
            let current_line = self.plan_edit_text.remove(self.plan_edit_cursor_line);
            self.plan_edit_cursor_line -= 1;
            self.plan_edit_cursor_col = self.plan_edit_text[self.plan_edit_cursor_line].len();
            self.plan_edit_text[self.plan_edit_cursor_line].push_str(&current_line);
        }
    }

    fn plan_edit_newline(&mut self) {
        let current_line = &self.plan_edit_text[self.plan_edit_cursor_line];
        let after_cursor = current_line[self.plan_edit_cursor_col..].to_string();
        self.plan_edit_text[self.plan_edit_cursor_line].truncate(self.plan_edit_cursor_col);
        self.plan_edit_cursor_line += 1;
        self.plan_edit_text
            .insert(self.plan_edit_cursor_line, after_cursor);
        self.plan_edit_cursor_col = 0;
    }

    fn plan_edit_move_up(&mut self) {
        if self.plan_edit_cursor_line > 0 {
            self.plan_edit_cursor_line -= 1;
            let line_len = self.plan_edit_text[self.plan_edit_cursor_line].len();
            self.plan_edit_cursor_col = self.plan_edit_cursor_col.min(line_len);
        }
    }

    fn plan_edit_move_down(&mut self) {
        if self.plan_edit_cursor_line + 1 < self.plan_edit_text.len() {
            self.plan_edit_cursor_line += 1;
            let line_len = self.plan_edit_text[self.plan_edit_cursor_line].len();
            self.plan_edit_cursor_col = self.plan_edit_cursor_col.min(line_len);
        }
    }

    fn plan_edit_move_left(&mut self) {
        if self.plan_edit_cursor_col > 0 {
            self.plan_edit_cursor_col -= 1;
        }
    }

    fn plan_edit_move_right(&mut self) {
        let line_len = self.plan_edit_text[self.plan_edit_cursor_line].len();
        if self.plan_edit_cursor_col < line_len {
            self.plan_edit_cursor_col += 1;
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

    fn add_output(&mut self, line: String) {
        self.output.push(line);
    }

    fn clear_output(&mut self) {
        self.output.clear();
        self.cargo_messages.clear();
        self.scroll_offset = 0;
        self.first_diagnostic_shown = false;
    }

    fn render_cargo_message(
        msg: &CargoMessage,
        mode: &DiagnosticDisplayMode,
        is_first: bool,
    ) -> Vec<String> {
        match msg {
            CargoMessage::CompilerMessage { message, target } => {
                let mut output = Vec::new();

                // Use the rendered field if available, otherwise format manually
                if let Some(rendered) = &message.rendered {
                    // The rendered field contains the full ANSI-formatted diagnostic
                    match mode {
                        DiagnosticDisplayMode::Summary => {
                            // Show only first line and file location
                            if let Some(first_line) = rendered.lines().next() {
                                output.push(first_line.to_string());
                            }
                            // Add file location from primary span
                            if let Some(span) = message.spans.iter().find(|s| s.is_primary) {
                                output.push(format!(
                                    "  --> {}:{}:{}",
                                    span.file_name, span.line_start, span.column_start
                                ));
                            }
                        }
                        DiagnosticDisplayMode::First => {
                            // Show full for first diagnostic, summary for rest
                            if is_first {
                                for line in rendered.lines() {
                                    output.push(line.to_string());
                                }
                            } else {
                                if let Some(first_line) = rendered.lines().next() {
                                    output.push(first_line.to_string());
                                }
                            }
                        }
                        DiagnosticDisplayMode::Full => {
                            // Show all lines for all diagnostics
                            for line in rendered.lines() {
                                output.push(line.to_string());
                            }
                        }
                    }
                } else {
                    // Fallback: manually format the message
                    let level_prefix = match message.level.as_str() {
                        "error" => "error",
                        "warning" => "warning",
                        "note" => "note",
                        "help" => "help",
                        _ => &message.level,
                    };

                    if let Some(target) = target {
                        output.push(format!(
                            "[{}] {}: {}",
                            target.name, level_prefix, message.message
                        ));
                    } else {
                        output.push(format!("{}: {}", level_prefix, message.message));
                    }

                    // Add file location in Summary mode
                    if matches!(mode, DiagnosticDisplayMode::Summary) {
                        if let Some(span) = message.spans.iter().find(|s| s.is_primary) {
                            output.push(format!(
                                "  --> {}:{}:{}",
                                span.file_name, span.line_start, span.column_start
                            ));
                        }
                    }
                }

                output
            }
            CargoMessage::CompilerArtifact { .. } => {
                vec![] // Skip artifact messages
            }
            CargoMessage::BuildScriptExecuted { .. } => {
                vec![] // Skip build script messages
            }
            CargoMessage::BuildFinished { success } => {
                if *success {
                    vec!["   Build finished successfully".to_string()]
                } else {
                    vec!["   Build failed".to_string()]
                }
            }
            CargoMessage::Unknown => {
                vec![] // Skip unknown messages
            }
        }
    }

    fn process_output_line(&mut self, line: &str) {
        // Try to parse as JSON cargo message
        if line.trim().starts_with('{') {
            if let Ok(msg) = serde_json::from_str::<CargoMessage>(line) {
                // Successfully parsed as cargo message
                self.cargo_messages.push(msg.clone());

                // Determine if we should show full diagnostic
                let show_full = if matches!(msg, CargoMessage::CompilerMessage { .. }) {
                    // Show full for first diagnostic, summary only for subsequent ones
                    let show = !self.first_diagnostic_shown;
                    self.first_diagnostic_shown = true;
                    show
                } else {
                    // Non-diagnostic messages always show in full (if they show at all)
                    true
                };

                // Render the message instead of showing raw JSON
                let rendered_lines =
                    Self::render_cargo_message(&msg, &self.diagnostic_display_mode, show_full);
                for rendered_line in rendered_lines {
                    self.add_output(rendered_line);
                }
                return;
            }
        }

        // Not a JSON message, add as-is
        self.add_output(line.to_string());
    }

    fn start_command(&mut self) {
        if self.running {
            return;
        }

        let selected_idx = self.selected.selected().unwrap_or(0);
        let command_str = self.command_inputs[selected_idx].clone();

        self.running = true;
        self.clear_output();

        // Set status to Running
        if let Some(step) = self.get_selected_step_mut() {
            step.status = Status::Running;
        }

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
                let final_status = match task.await {
                    Ok(Ok(result)) => {
                        for line in result.stdout.lines() {
                            self.process_output_line(line);
                        }
                        for line in result.stderr.lines() {
                            self.process_output_line(line);
                        }

                        // Determine status based on cargo messages
                        let has_error = self.cargo_messages.iter().any(|msg| {
                            if let CargoMessage::CompilerMessage { message, .. } = msg {
                                message.level == "error"
                            } else {
                                false
                            }
                        });

                        let has_warning = self.cargo_messages.iter().any(|msg| {
                            if let CargoMessage::CompilerMessage { message, .. } = msg {
                                message.level == "warning"
                            } else {
                                false
                            }
                        });

                        if has_error || !result.success {
                            Status::Failure
                        } else if has_warning {
                            Status::Warning
                        } else {
                            Status::Success
                        }
                    }
                    Ok(Err(e)) => {
                        self.add_output(format!("Error: {}", e));
                        Status::Failure
                    }
                    Err(e) => {
                        if e.is_cancelled() {
                            self.add_output("Command was cancelled".to_string());
                        } else {
                            self.add_output(format!("Task error: {}", e));
                        }
                        Status::Failure
                    }
                };

                // Update the status of the selected step
                if let Some(step) = self.get_selected_step_mut() {
                    step.status = final_status.clone();
                }

                self.running = false;

                // Auto-advance to next step if in auto mode and current step succeeded or has warnings
                if self.auto_mode && matches!(final_status, Status::Success | Status::Warning) {
                    if let Some(next_step_idx) = self.get_next_step_in_plan() {
                        self.selected.select(Some(next_step_idx));
                        self.update_cursor_for_current_input();
                        self.start_command();
                    }
                    // Note: auto mode stays on even when reaching end of plan
                }
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

fn get_status_indicator(status: &Status) -> &str {
    match status {
        Status::NotRun => "",
        Status::Running => "...",
        Status::Warning => "!",
        Status::Failure => "✗",
        Status::Success => "✓",
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    // Render the main app widget
    frame.render_widget(&*app, frame.area());

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
    if app.input_focused {
        frame.set_cursor_position((chunks[2].x + app.input_cursor as u16 + 1, chunks[2].y + 1));
    }

    // Set cursor position for plan editing modal
    if app.plan_editing {
        let area = frame.area();
        let modal_width = (area.width as f32 * 0.8) as u16;
        let modal_height = (area.height as f32 * 0.8) as u16;
        let modal_x = (area.width - modal_width) / 2;
        let modal_y = (area.height - modal_height) / 2;

        let modal_area = ratatui::layout::Rect {
            x: modal_x,
            y: modal_y,
            width: modal_width,
            height: modal_height,
        };

        let modal_block = Block::default()
            .title("Edit Plan")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black));

        let inner_area = modal_block.inner(modal_area);
        let cursor_x = inner_area.x + app.plan_edit_cursor_col as u16;
        let cursor_y = inner_area.y + app.plan_edit_cursor_line as u16;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
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

                    if app.plan_editing {
                        // PLAN EDITING MODE
                        match key.code {
                            KeyCode::Esc => {
                                app.cancel_plan_editing();
                            }
                            KeyCode::Char('s')
                                if key
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
                            {
                                app.save_plan_edits();
                            }
                            KeyCode::Enter => {
                                app.plan_edit_newline();
                            }
                            KeyCode::Backspace => {
                                app.plan_edit_delete_char();
                            }
                            KeyCode::Up => {
                                app.plan_edit_move_up();
                            }
                            KeyCode::Down => {
                                app.plan_edit_move_down();
                            }
                            KeyCode::Left => {
                                app.plan_edit_move_left();
                            }
                            KeyCode::Right => {
                                app.plan_edit_move_right();
                            }
                            KeyCode::Char(c) => {
                                app.plan_edit_insert_char(c);
                            }
                            _ => {}
                        }
                    } else if app.input_focused {
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
                            KeyCode::Char('a') => {
                                app.auto_mode = !app.auto_mode;
                                // If turning on auto mode, start from first non-successful step
                                if app.auto_mode {
                                    if let Some(first_step_idx) =
                                        app.get_first_non_successful_step_in_plan()
                                    {
                                        app.selected.select(Some(first_step_idx));
                                        app.update_cursor_for_current_input();
                                        app.start_command();
                                    }
                                    // Note: auto mode stays on even if all steps are already successful
                                }
                            }
                            KeyCode::Char('p') => {
                                app.start_plan_editing(terminal);
                            }
                            KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
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
                            KeyCode::Char('s') => {
                                app.diagnostic_display_mode = app.diagnostic_display_mode.next();
                                app.start_command();
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
            app.reset_all_steps();

            // If auto mode is on, start from the first step in the plan
            if app.auto_mode {
                if let Some(first_step_idx) = app.get_first_step_in_plan() {
                    app.selected.select(Some(first_step_idx));
                    app.update_cursor_for_current_input();
                }
            }

            app.start_command();
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
    let mut app = App::new(args.custom);

    // Auto-run the selected tab's command on startup
    app.start_command();

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

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::backend::TestBackend;
    use testresult::TestResult;

    use super::*;

    #[test]
    fn test_first_screen() -> TestResult {
        let app = App::new(None);
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }

    #[test]
    fn test_auto_disabled() -> TestResult {
        let mut app = App::new(None);
        app.auto_mode = false;
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }
}
