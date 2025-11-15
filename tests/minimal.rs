use insta::assert_snapshot;
use ratatui::{backend::TestBackend, Terminal};
use testresult::TestResult;

#[test]
fn test_minimal() -> TestResult {
    // let app = App::new(None);
    // let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
    // terminal.draw(|frame| frame.render_widget(&app, frame.area()))?;
    // assert_snapshot!(terminal.backend());
    Ok(())
}
