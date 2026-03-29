pub mod cmdmgr;
pub mod hubgui;
pub mod peripheral_mgr;

use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    log::info!("Hello world!");

    hubgui::run_gui().await;

    log::info!("Done!");
    Ok(())
}
