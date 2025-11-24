use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug, Clone)]
pub enum AppEvent {
    Init,
    FileChanged,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    FocusLost,
    FocusGained,
    Paste(String),
    Error,
    Tick,
    Render,
}
