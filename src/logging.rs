use color_eyre::eyre::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn setup_logging(maybe_log_file: Option<String>) -> Result<()> {
    let log_file_path = if let Some(path) = maybe_log_file {
        path
    } else {
        return Ok(());
    };

    let log_file = std::fs::File::create(log_file_path)?;

    let file_subscriber = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file)
        .with_target(false)
        .with_ansi(true)
        .with_filter(tracing_subscriber::filter::EnvFilter::from_default_env());

    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .init();
    Ok(())
}
