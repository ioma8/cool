use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("simulator/sdcard"));
    std::fs::create_dir_all(&root)?;
    simulator::runtime::run(&root, simulator::runtime::browser_page_size(), 1)
}
