//! TODO
//! - close button
//!    - handle mgr disconnect and close gracefully
//! - improve everything (gui-wise; don't do everything in update, have some memeber variables etc...)

use eframe::egui;
use btleplug::api::BDAddr;
use crossbeam_channel::{bounded, Sender, Receiver, TryRecvError};
use tokio::task::JoinHandle;
use crate::peripheral_mgr::peripheral::{PeripheralMgr, HubMsg, EventMsg};
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

struct MeasureApp {
    rx: Receiver<EventMsg>,
    tx: Sender<HubMsg>,
    sensors: Vec<BDAddr>,
    _handle: JoinHandle<u32>,
}

impl MeasureApp {

    pub fn new() -> Self {

        let (gui_tx, thread_rx) = bounded(4);
        let (thread_tx, gui_rx) = bounded(4);

        let mgr_handle = tokio::spawn(peripheral::mgr_run(thread_tx, thread_rx));

        Self {
            tx: gui_tx,
            rx: gui_rx,
            sensors: Vec::<BDAddr>::new(),
            _handle: mgr_handle,
        }
    }

    fn find_sensors(&mut self) {
        log::info!("looking for sensors");
        self.tx.send(HubMsg::FindSensors).unwrap();
    }

    fn blink(&mut self, addr: BDAddr) {
        log::info!("blinking led");
        self.tx.send(HubMsg::Blink(addr)).unwrap();
    }

    // non-blocking to be run inside frame update
    fn run(&mut self) -> i8 {

        match self.rx.try_recv() {
            Ok(val) => {
                dbg!(&val);
                match val {
                    EventMsg::DeviceDiscovered(addr) => {
                        log::info!("Device Discovered: {addr:?}");
                        self.tx.send(HubMsg::Connect(addr)).unwrap();
                    },
                    EventMsg::DeviceConnected(addr) => {
                        log::info!("Device Connected: {addr:?}");
                        self.sensors.push(addr);
                    },
                    EventMsg::DeviceDisconnected(addr) => {
                        // for testing; let's stop the thread if a device disconnects
                        log::info!("Device Disconnected: {addr:?}");
                        let removed = self.sensors.extract_if(.., |x| *x == addr).collect::<Vec<_>>();
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

            if ui.button("Find Sensors").clicked() {
                self.find_sensors();
            }

            // one button for each connected sensor
            for addr in self.sensors.clone() {
                if ui.button(addr.to_string()).clicked() {
                    self.blink(addr);
                }
            }

        });
    }

}