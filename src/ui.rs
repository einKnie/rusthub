//! UI implementations hub
//!

#[cfg(any(
    all(feature = "egui_gui", feature = "slint_gui"),
    all(feature = "egui_gui", feature = "iced_gui"),
    all(feature = "slint_gui", feature = "iced_gui")
))]
compile_error!("exactly one gui feature must be enabled");

#[cfg(not(any(feature = "egui_gui", feature = "iced_gui", feature = "slint_gui")))]
compile_error!("exactly one gui feature must be enabled");

#[cfg(feature = "egui_gui")]
pub mod hubgui;

#[cfg(feature = "iced_gui")]
pub mod iced_gui;

#[cfg(feature = "slint_gui")]
pub mod slintgui;
