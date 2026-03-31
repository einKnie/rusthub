#![warn(missing_docs)]

//! RustHub
//!
//! Gui for sensor peripheral management

pub mod cmdmgr;
pub mod database_mgr;
pub mod hubgui;
pub mod peripheral_mgr;

#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("Hello world!");

    hubgui::run_gui().await;

    log::info!("Done!");
}
