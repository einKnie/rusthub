//! TODO
//! - close button
//!    - handle mgr disconnect and close gracefully
//! - improve everything (gui-wise; don't do everything in update, have some memeber variables etc...) => actually, apparently this is not how egui is supposed to work (see "immediate mode gui" vs "retained mode gui")

use crate::peripheral_mgr::peripheral;
use crate::peripheral_mgr::peripheral::{EventMsg, HubMsg};
use btleplug::api::BDAddr;
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use eframe::egui;
use tokio::task::JoinHandle;

/// Run the measurent GUI
///
/// This is basically the only thing we run from main at this point
pub fn run_gui() -> u32 {

    // determine path for storage
    let storage_path = match std::env::home_dir() {
        Some(mut home_dir) => {
            home_dir.push("MeasureHub");
            Some(home_dir)
        },
        None => None,
    };

    dbg!(&storage_path);

    // eframe options
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        persistence_path: storage_path,
        ..Default::default()
    };
    if eframe::run_native(
        "Measurement Hub",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<MeasureApp>::new(MeasureApp::new(cc)))
        }),
    )
    .is_err()
    {
        log::error!("GUI ended with error");
    }
    0
}

/// SensorName
///
/// Represents a connected sensor's name and allows in-flight changes
/// as well as change cancellation. In-flight changes can be made to .next
/// and applied to be permanent with .update()
///
/// @todo make this more generic? for general use?
#[derive(Clone, Debug)]
struct SensorName {
    value: String,
    next: String
}

impl SensorName {
    fn new(val: String) -> Self {
        Self {
            value: val.clone(),
            next: val.clone(),
        }
    }

    /// Update value to next-value
    fn update(&mut self) {
        self.value = self.next.clone();
    }

    /// Reset next-value to value
    fn reset(&mut self) {
        self.next = self.value.clone();
    }
}

/// SensorValue
///
/// Helper class to allow seamless records on value range.
/// Could be updated to retain more info in the furture, e.g. for charts (though info should be kept on disk not in memory)
#[derive(Clone, Debug)]
struct SensorValue {
    value: u32,

    min: u32,
    max: u32,
}

impl SensorValue {
    fn new(val: u32) -> Self {
        Self {
            value: val,
            min: u32::MAX,
            max: 0,
        }
    }

    /// Update value to next-value
    /// and update min and max if necessary
    fn update(&mut self, next: u32) {
        if next < self.min {
            self.min = next;
        }
        if next > self.max {
            self.max = next;
        }

        self.value = next;
    }
}

/// Connected Sensor
///
/// Represents one connected sensor with device address and name
#[derive(Clone, Debug)]
struct ConnectedSensor {
    addr: BDAddr,
    name: SensorName,
    value: SensorValue,
    subscribed: bool,
}

impl PartialEq for ConnectedSensor {
    fn eq(&self, other: &Self) -> bool {
        self.addr() == other.addr()
    }
}

impl ConnectedSensor {
    /// Get current sensor name
    pub fn name(&self) -> String {
        self.name.value.clone()
    }

    /// Get current sensor value
    pub fn value(&self) -> u32 {
        self.value.value
    }

    /// Get the sensor's address
    pub fn addr(&self) -> BDAddr {
        self.addr
    }
}

/// UiAction
///
/// enum denoting currently running actions
/// mostly used for displaying spinner
#[derive(Clone, Debug, PartialEq)]
enum UiAction {
    NoAction,
    Searching,
    Connecting(BDAddr),
}

/// App state
#[derive(Clone, Debug)]
struct MeasureAppState {
    sensors: Vec<ConnectedSensor>,
    action: UiAction,
}

/// Measurement GUI
struct MeasureApp {
    rx: Receiver<EventMsg>,
    tx: Sender<HubMsg>,
    _handle: JoinHandle<u32>,

    state: MeasureAppState,
}

impl MeasureApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (gui_tx, thread_rx) = bounded(4);
        let (thread_tx, gui_rx) = bounded(4);

        let mgr_handle = tokio::spawn(peripheral::mgr_run(thread_tx, thread_rx));

        Self {
            tx: gui_tx,
            rx: gui_rx,
            _handle: mgr_handle,
            state: MeasureAppState {sensors: Vec::<ConnectedSensor>::new(), action: UiAction::NoAction},
        }
    }

    fn find_sensors(&mut self) {
        log::info!("looking for sensors");
        self.state.action = UiAction::Searching;
        self.tx.send(HubMsg::FindSensors).unwrap();
    }

    fn ping(&self) {
        log::debug!("GUI managing sensors:");
        dbg!(&self.state.sensors);
        log::info!("pinging manager");
        self.tx.send(HubMsg::Ping).unwrap();
    }

    fn blink(&self, addr: BDAddr) {
        log::info!("blinking led");
        self.tx.send(HubMsg::Blink(addr)).unwrap();
    }

    fn blink_all(&self) {
        log::info!("blinking led");
        self.tx.send(HubMsg::BlinkAll).unwrap();
    }

    fn read_sensor(&self, addr: BDAddr) {
        log::info!("reading from sensor");
        self.tx.send(HubMsg::ReadFrom(addr)).unwrap();
    }

    fn subscribe(&self, addr: BDAddr) {
        log::info!("subscribing to data from sensor");
        self.tx.send(HubMsg::Subscribe(addr)).unwrap();
    }

    fn unsubscribe(&self, addr: BDAddr) {
        log::info!("unsubscribing to data from sensor");
        self.tx.send(HubMsg::Unsubscribe(addr)).unwrap();
    }

    fn disconnect_all(&self) {
        for p in self.state.sensors.iter() {
            self.tx.send(HubMsg::Disconnect(p.addr)).unwrap();
        }
    }

    fn connect_all(&self) {
        self.tx.send(HubMsg::ConnectAll).unwrap();
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
                        self.state.action = UiAction::Connecting(addr);
                    }
                    EventMsg::SearchFailed => {
                        log::info!("Failed to find any sensors");
                        self.state.action = UiAction::NoAction;
                    }
                    EventMsg::NewData(addr, data) => {
                        log::info!("received new sensor data for {addr:?}");
                        if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                            p.value.update(data);
                        }
                    }
                    EventMsg::DeviceConnected(addr) => {
                        log::info!("Device Connected: {addr:?}");
                        self.state.sensors.push(ConnectedSensor {
                            addr,
                            name: SensorName::new(format!("Sensor {:?}", self.state.sensors.len() + 1)),
                            value: SensorValue::new(0),
                            subscribed: false,
                        });
                        self.state.action = UiAction::NoAction;
                    }
                    EventMsg::DeviceDisconnected(addr) => {
                        log::info!("Device Disconnected: {addr:?}");
                        let removed = self
                            .state
                            .sensors
                            .extract_if(.., |x| x.addr() == addr)
                            .collect::<Vec<_>>();
                        log::info!("Removed from UI: {removed:?}");
                    }
                    EventMsg::ServiceDiscovered(addr) => {
                        log::info!("Found Moisture LED service: {addr:?}")
                    } // not really needed; and sensor currently does not advrertise this
                };
            }
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                log::warn!("disconnected from thread!");
                return -1;
            }
        }
        0
    }
}

// todo: statusbar with current messages (i.e. search failed)
impl eframe::App for MeasureApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Soil Measurement Sensor Hub");
            if self.run() < 0 {
                log::warn!("disconnected from thread, should stop");
            }

            // enable button only when no search is in progress
            if ui.add_enabled(self.state.action != UiAction::Searching, egui::Button::new("Find Sensors"))
                .clicked()
            {
                self.find_sensors();
            }
            // enable button only when no search is in progress
            if ui.add_enabled(self.state.action == UiAction::NoAction, egui::Button::new("Connect"))
                .clicked()
            {
                self.connect_all();
            }

            // show spinner while action in progress
            if self.state.action != UiAction::NoAction {
                let label = match self.state.action {
                    UiAction::Searching => String::from("Searching"),
                    UiAction::Connecting(addr) => format!("Connecting ({addr:?})"),
                    _ => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(label);
                });
            }

            ui.add(egui::Separator::default());

            // TODO: I have to take this ugly route to be able to iterate through the sensors mutably
            // with self.sensors.iter_mut() the whole self becomes mutably borrowed which messes things up here;
            // find out if there is a better (canonical) way to do this
            let mut sensors = self.state.sensors.clone();
            // one button for each connected sensor
            for s in sensors.iter_mut() {

                // idea: have a 'box' per peripheral, with several buttons (read, blink, disconnect)
                ui.add(egui::Label::new(format!("{0} ({1})", s.name(), s.value())));
                if ui.button("Read").clicked() {
                    self.read_sensor(s.addr);
                }
                if ui.button("Blink").clicked() {
                    self.blink(s.addr);
                }

                if s.subscribed {
                    if ui.button("Unsubscribe").clicked() {
                        self.unsubscribe(s.addr);
                        s.subscribed = false;
                    }
                } else if ui.button("Subscribe").clicked() {
                    self.subscribe(s.addr);
                    s.subscribed = true;
                }

                // allow sensor name change
                // TODO: reset name.next to name if apply button is not clicked? e.g. on show changed?
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.name.next));

                    if ui.button("Change Name").clicked() {
                        log::debug!("name change requested");
                        s.name.update();
                    }
                });

                ui.add(egui::Separator::default());
            }

            // update sensors (in case smth was changed)
            self.state.sensors = sensors.clone();

            // general interface (independent from connected sensors)
            ui.horizontal(|ui| {
                if ui.button("Ping").clicked() {
                    self.ping();
                }

                if ui.button("Blink all").clicked() {
                    self.blink_all();
                }

                if ui.button("Disconnect all").clicked() {
                    self.disconnect_all();
                }

                // exit
                if ui.button("Close App").clicked() {
                    log::info!("Closing app. Byebye!");
                    self.cleanup_and_exit(ctx.clone());
                }
            });

            // yes, this updates the ui all the time, but this (no na) also causes cpu usage to go up
            // but we actually need this. the entire msg handling loop is also run in here.
            ui.ctx().request_repaint();
        });
    }
}
