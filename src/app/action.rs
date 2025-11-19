#[derive(Debug, Clone)]
pub enum AppAction {
    StartCommand,
    CancelCommand,
    EditPlan,
    ScrollUp,
    ScrollDown,
    ToggleAuto,
    SwitchTab(usize),
    CycleDiagnosticMode,
    EnterCommandEditMode,
    ExitCommandEditMode,
    CommandEditModeBackspace,
    CommandEditModeLeft,
    CommandEditModeRight,
    CommandEditModeChar(char),
}
