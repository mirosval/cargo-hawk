use color_eyre::eyre::Result;
use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyEventKind,
    },
    terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{FutureExt, StreamExt};
use ratatui::{prelude::Backend, Terminal};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::app::AppEvent;

#[derive(Debug)]
pub struct Tui<B: Backend> {
    terminal: Terminal<B>,
    task: JoinHandle<()>,
    event_tx: UnboundedSender<AppEvent>,
    cancellation_token: CancellationToken,
    frame_rate: f64,
    tick_rate: f64,
    mouse: bool,
    paste: bool,
}

impl<B: Backend> Tui<B> {
    pub fn new(terminal: Terminal<B>, event_tx: UnboundedSender<AppEvent>) -> Tui<B> {
        let tick_rate = 4.0;
        let frame_rate = 60.0;
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});
        let mouse = false;
        let paste = false;
        Self {
            tick_rate,
            frame_rate,
            event_tx,
            terminal,
            cancellation_token,
            task,
            mouse,
            paste,
        }
    }

    pub fn start(&mut self) {
        let tick_delay = Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let _cancellation_token = self.cancellation_token.clone();
        let _event_tx = self.event_tx.clone();
        self.task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);
            _event_tx
                .send(AppEvent::Init)
                .expect("Sending AppEvent via mpsc Sender");
            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                    _ = _cancellation_token.cancelled() => {
                        break;
                    }
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    Event::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            _event_tx.send(AppEvent::Key(key)).expect("Sending AppEvent");
                                        }
                                    }
                                    Event::Mouse(mouse) => {
                                        _event_tx.send(AppEvent::Mouse(mouse)).expect("Sending AppEvent");
                                    }
                                    Event::Resize(x, y) => {
                                        _event_tx.send(AppEvent::Resize(x, y)).expect("Sending AppEvent");
                                    }
                                    Event::FocusLost => {
                                        _event_tx.send(AppEvent::FocusLost).expect("Sending AppEvent");
                                    }
                                    Event::FocusGained => {
                                        _event_tx.send(AppEvent::FocusGained).expect("Sending AppEvent");
                                    }
                                    Event::Paste(paste) => {
                                        _event_tx.send(AppEvent::Paste(paste)).expect("Sending AppEvent");
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                // TODO: Error handling
                                eprintln!("{:?}", e);
                                _event_tx.send(AppEvent::Error).expect("Sending AppEvent");
                            }
                            None => {},
                        }
                    }
                    _ = tick_delay => {
                        _event_tx.send(AppEvent::Tick).expect("Sending AppEvent");
                    }
                    _ = render_delay => {
                        _event_tx.send(AppEvent::Render).expect("Sending AppEvent");
                    }
                }
            }
        });
    }

    pub fn stop(&self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.task.abort();
            }
            if counter > 100 {
                // TODO: Error handling
                eprintln!("failed to abort task in 100ms for an unknown reason");
                break;
            }
        }
        Ok(())
    }

    pub fn enter(&mut self) -> Result<()> {
        enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(std::io::stderr(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(std::io::stderr(), EnableBracketedPaste)?;
        }
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        if crossterm::terminal::is_raw_mode_enabled()? {
            if self.paste {
                crossterm::execute!(std::io::stderr(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(std::io::stderr(), DisableMouseCapture)?;
            }
            crossterm::execute!(std::io::stderr(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }
}

impl<B: Backend> Deref for Tui<B> {
    type Target = Terminal<B>;
    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl<B: Backend> DerefMut for Tui<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl<B: Backend> Drop for Tui<B> {
    fn drop(&mut self) {
        self.exit().expect("Drop")
    }
}
