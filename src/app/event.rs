use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Init,
    FileChanged(PathBuf),
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
