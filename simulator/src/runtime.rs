use core::mem::size_of;
use std::path::Path;

use crate::{input::pressed_buttons, storage::HostStorage, window::SimulatorWindow};
use xteink_app::{AppStorage, Session};
use xteink_memory::{
    DEVICE_PERSISTENT_BUDGET_BYTES, DEVICE_STACK_RESERVE_BYTES, DEVICE_TOTAL_RAM_BYTES,
    DEVICE_TRANSIENT_HEADROOM_BYTES, DeviceMemoryFootprint,
};
use xteink_render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, EPUB_RENDER_WORKSPACE_BYTES};

pub fn bootstrap_session<S: AppStorage>(
    storage: S,
    page_size: usize,
) -> Result<Session<S>, S::Error> {
    let mut session = Session::new(storage, page_size);
    session.bootstrap()?;
    Ok(session)
}

pub fn simulator_device_memory_footprint(scale: usize) -> DeviceMemoryFootprint {
    let device_bytes = size_of::<Session<HostStorage>>() + EPUB_RENDER_WORKSPACE_BYTES;
    let host_only_bytes =
        usize::from(DISPLAY_WIDTH) * usize::from(DISPLAY_HEIGHT) * scale * scale * size_of::<u32>();
    DeviceMemoryFootprint::with_host_overhead(device_bytes, host_only_bytes)
}

fn print_simulator_memory_report(footprint: DeviceMemoryFootprint, scale: usize) {
    let used_permille = footprint.used_device_permille();
    println!(
        "simulator memory: device={}B/{DEVICE_PERSISTENT_BUDGET_BYTES}B ({}.{}%), remaining={}B, host_window={}B, scale={scale}, total_ram={}B, stack_reserve={}B, transient_headroom={}B",
        footprint.device_bytes,
        used_permille / 10,
        used_permille % 10,
        footprint.remaining_device_bytes(),
        footprint.host_only_bytes,
        DEVICE_TOTAL_RAM_BYTES,
        DEVICE_STACK_RESERVE_BYTES,
        DEVICE_TRANSIENT_HEADROOM_BYTES,
    );
}

pub fn run(root: &Path, page_size: usize, scale: usize) -> Result<(), Box<dyn std::error::Error>> {
    let footprint = simulator_device_memory_footprint(scale);
    print_simulator_memory_report(footprint, scale);
    if !footprint.fits_device_budget() {
        return Err(std::io::Error::other(format!(
            "simulated device memory footprint {} exceeds budget {}",
            footprint.device_bytes, DEVICE_PERSISTENT_BUDGET_BYTES
        ))
        .into());
    }
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
