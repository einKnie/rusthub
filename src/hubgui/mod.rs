

use eframe::egui;
use eframe::egui::widgets::Button;
//use crate::bt::ble_mgr::{Ble, ConnectionMgr};

pub fn run_gui() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|_cc| {
            // This gives us image support:
            //egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<MyApp>::default())
        }),
    )
}

struct MyApp {
    //bt: Ble,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            //bt: ConnectionMgr::new()
        }
    }
}

impl MyApp {

    fn connect_sensor(&mut self) {
        log::info!("connecting to sensor");
        // let connector = std::thread::spawn(async move || {
        //     self.bt.init().await;
        //     self.bt.connect_sensor().await;
        // });

        // connector.join();
    }

    fn toggle_led(&mut self) {
        log::info!("blinking led");
        // // read bluetooth to get current state of led,
        // // then set the other state
        // let blinker = std::thread::spawn(async move || {
        //     self.bt.blinky().await;
        // });

        // blinker.join();
    }

}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Soil Measurement Sensor Hub");
            //ui.label(format!("Sensor status: {}", match self.bt.connected { true => "connected", false => "not connected" }));

            if ui.button("Connect to Sensor").clicked() {
                self.connect_sensor();
            }
            if ui.button("blink LED").clicked() {
                self.toggle_led();
            }
        });
    }

}