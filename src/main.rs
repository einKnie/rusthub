#![warn(missing_docs)]

//! RustHub
//!
//! Gui for sensor peripheral management

pub mod cmdmgr;
pub mod database_mgr;
pub mod peripheral_mgr;
pub mod ui;

// run old hubgui
#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("Hello world with egui!");

    ui::hubgui::run_gui().await;

    log::info!("Done!");
}
