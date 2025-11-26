use color_eyre::eyre::Result;
use ratatui::{Terminal, backend::CrosstermBackend};

use cargo_hawk::{App, Tui, cli, setup_logging};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::parse();

    setup_logging(args.log_file.clone())?;
    color_eyre::install()?;

    let mut app = App::new(args)?;
    let mut tui = {
        let backend = CrosstermBackend::new(std::io::stdout());
        let terminal = Terminal::new(backend)?;
        Tui::new(terminal, app.event_tx())
    };

    info!("--------------------------------------------------------------------------------");
    info!("starting hawk");
    app.run(&mut tui).await?;

    Ok(())
}
