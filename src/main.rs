use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::CrosstermBackend};

use cargo_hawk::{App, Tui, cli, setup_logging, trace_dbg};

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::parse();

    setup_logging(args.verbose)?;
    color_eyre::install()?;

    let mut app = App::new(args)?;
    let mut tui = {
        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend)?;
        Tui::new(terminal, app.event_tx())
    };

    trace_dbg!("starting hawk");
    app.run(&mut tui).await?;

    Ok(())
}
