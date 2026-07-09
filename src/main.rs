use std::error::Error;

use spsa_tui::app::App;

fn main() -> Result<(), Box<dyn Error>> {
    let mut app = App::new();
    ratatui::run(|terminal| app.run(terminal))
}
