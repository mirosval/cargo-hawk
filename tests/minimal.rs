use cargo_hawk::{App, cli::Args};
use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use testresult::TestResult;

#[test]
fn test_minimal() -> TestResult {
    let app = App::new(Args::default())?;
    let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
    terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
    assert_snapshot!(terminal.backend());
    Ok(())
}
