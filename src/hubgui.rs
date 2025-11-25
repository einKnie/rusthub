//! TODO
//! - close button
//!    - handle mgr disconnect and close gracefully
//! - improve everything (gui-wise; don't do everything in update, have some memeber variables etc...) => actually, apparently this is not how egui is supposed to work (see "immediate mode gui" vs "retained mode gui")

use eframe::egui;
use btleplug::api::BDAddr;
use crossbeam_channel::{bounded, Sender, Receiver, TryRecvError};
use tokio::task::JoinHandle;
use crate::peripheral_mgr::peripheral::{HubMsg, EventMsg};
use crate::peripheral_mgr::peripheral;

/// Run the measurent GUI
/// 
/// This is basically the only thing we run from main at this point
pub fn run_gui() -> u32 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    if eframe::run_native(
        "Measurement Hub",
        options,
        Box::new(|_cc| {
            // This gives us image support:
            //egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<MeasureApp>::new(MeasureApp::new()))
        }),
    ).is_err() {
        log::error!("GUI ended with error");
    }
    0
}

// todo:
// i want to have an actual snesor name on the button that is changable
// but also keep the current use of BDAddr since that is easy

/// Connected Sensor
///
/// Represents one connected sensor with device address and name
#[derive(Clone, Debug)]
struct ConnectedSensor {
    addr: BDAddr,
    name: String,
}

/// compare by name, could be useful when allowing the user to rename sensors
/// (although, tbh, the name is representative only, so it does not matter and mutiple sensors *could* have the same name)
impl PartialEq for ConnectedSensor {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl ConnectedSensor {

    /// Get and/or set the name
    /// If a new name is provided, it is set for the sensor
    /// Either way, the current name is returned
    pub fn name(&mut self, new: Option<String>) -> String {
        match new {
            None => (),
            Some(new_name) => {
                self.name = new_name
            }
        };
        self.name.clone()
    }

    /// Get the sensor's address
    pub fn addr(&self) -> BDAddr {
        self.addr
    }
}

/// Measurement GUI
struct MeasureApp {
    rx: Receiver<EventMsg>,
    tx: Sender<HubMsg>,
    sensors: Vec<ConnectedSensor>,
    _handle: JoinHandle<u32>,

    searching: bool,
}

impl MeasureApp {

    pub fn new() -> Self {

        let (gui_tx, thread_rx) = bounded(4);
        let (thread_tx, gui_rx) = bounded(4);

        let mgr_handle = tokio::spawn(peripheral::mgr_run(thread_tx, thread_rx));

        Self {
            tx: gui_tx,
            rx: gui_rx,
            sensors: Vec::<ConnectedSensor>::new(),
            _handle: mgr_handle,
            searching: false,
        }
    }

    fn find_sensors(&mut self) {
        log::info!("looking for sensors");
        self.searching = true;
        self.tx.send(HubMsg::FindSensors).unwrap();
    }

    fn blink(&mut self, addr: BDAddr) {
        log::info!("blinking led");
        self.tx.send(HubMsg::Blink(addr)).unwrap();
    }

    fn cleanup_and_exit(&self, ctx: egui::Context) {
        // fire-and-forget, since we can't await the handle here
        // and this is good enough for now (sorry)
        self.tx.send(HubMsg::StopThread).unwrap();

        // works (https://github.com/emilk/egui/discussions/4103#discussioncomment-9225022)
        std::thread::spawn(move || {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        });
    }

    // non-blocking to be run inside frame update
    // @todo: is there another way i could do this? or is this fine?
    fn run(&mut self) -> i8 {

        match self.rx.try_recv() {
            Ok(val) => {
                dbg!(&val);
                match val {
                    EventMsg::DeviceDiscovered(addr) => {
                        log::info!("Device Discovered: {addr:?}");
                        self.tx.send(HubMsg::Connect(addr)).unwrap();
                    },
                    EventMsg::SearchFailed => {
                        log::info!("Failed to find any sensors");
                        self.searching = false;
                    }
                    EventMsg::DeviceConnected(addr) => {
                        log::info!("Device Connected: {addr:?}");
                        self.sensors.push(ConnectedSensor {addr: addr, name: format!("Sensor {:?}", self.sensors.len()+1)});
                        self.searching = false;
                    },
                    EventMsg::DeviceDisconnected(addr) => {
                        log::info!("Device Disconnected: {addr:?}");
                        let removed = self.sensors.extract_if(.., |x| x.addr() == addr).collect::<Vec<_>>();
                        log::info!("Removed from UI: {removed:?}");
                    },
                    EventMsg::ServiceDiscovered(addr) => log::info!("Found Moisture LED service: {addr:?}"), // does nothing: i think b/c the sensor does not advertise; i guess i have to add that lol
                };
            },
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                log::warn!("disconnected from thread!");
                return -1
            }
        }
        0
    }
}

impl eframe::App for MeasureApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Soil Measurement Sensor Hub");
            if self.run() < 0 {
                log::warn!("disconnected from thread, should stop");
            }

            // TODO: feedback to user (search running, not running etc)
            // this means, that peripheral_mgr must report back after searching
            if !self.searching {
                if ui.add(egui::Button::new("Find Sensors")).clicked() {
                    self.find_sensors();
                }
            } else {
                if ui.add_enabled(false, egui::Button::new("Find Sensors")).clicked() {
                    unreachable!();
                }
            }

            // one button for each connected sensor
            for mut s in self.sensors.clone() {
                if ui.button(s.name(None)).clicked() {
                    self.blink(s.addr);
                }
            }

            // exit
            if ui.button("Close App").clicked() {
                log::info!("Closing app. Byebye!");
                self.cleanup_and_exit(ctx.clone());
            }

        });
    }

}