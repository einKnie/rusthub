#![warn(missing_docs)]

//! RustHub
//!
//! Gui for sensor peripheral management

pub mod cmdmgr;
pub mod database_mgr;
pub mod peripheral_mgr;
pub mod ui;

#[cfg(any(feature = "slint_gui", feature = "egui_gui"))]
#[tokio::main]
#[cfg(any(feature = "slint_gui", feature = "egui_gui"))]
async fn main() {
    env_logger::init();
    log::info!("Hello world!");

    #[cfg(feature = "egui_gui")]
    ui::hubgui::run_gui().await;

    #[cfg(feature = "slint_gui")]
    ui::slintgui::run_gui().await;

    log::info!("Done!");
}

// run new iced gui
#[cfg(feature = "iced_gui")]
fn main() {
    env_logger::init();
    log::info!("Hello world with iced!");

    ui::iced_gui::run_gui();

    log::info!("Done!");
}
