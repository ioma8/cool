use std::path::Path;

use crate::{input::pressed_buttons, storage::HostStorage, window::SimulatorWindow};
use xteink_app::{AppStorage, Session};

pub fn bootstrap_session<S: AppStorage>(
    storage: S,
    page_size: usize,
) -> Result<Session<S>, S::Error> {
    let mut session = Session::new(storage, page_size);
    session.bootstrap()?;
    Ok(session)
}

pub fn run(root: &Path, page_size: usize, scale: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = bootstrap_session(HostStorage::new(root), page_size)?;
    let mut window = SimulatorWindow::new("Xteink Simulator", scale)?;

    while window.is_open() {
        for button in pressed_buttons(window.window()) {
            let _ = session.handle_button(button);
        }
        window.update(session.framebuffer())?;
    }

    Ok(())
}
