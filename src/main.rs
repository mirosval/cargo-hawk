use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::CrosstermBackend};

use cargo_hawk::{App, Tui, cli};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = cli::parse();

    let mut app = App::new(args)?;
    let mut tui = {
        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend)?;
        Tui::new(terminal, app.event_tx())
    };
    app.run(&mut tui).await?;

    Ok(())
}
