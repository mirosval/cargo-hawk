use clap::Parser;
use color_eyre::eyre::Result;
use ratatui::{backend::CrosstermBackend, Terminal};

use cargo_hawk::{App, Args, Tui};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut app = App::new(args)?;
    let mut tui = {
        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend)?;
        Tui::new(terminal, app.event_tx())
    };
    app.run(&mut tui).await?;

    Ok(())
}
