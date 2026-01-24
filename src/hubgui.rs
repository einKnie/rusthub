use crate::peripheral_mgr::message::{
    CmdMgr, HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg,
};
use crate::peripheral_mgr::peripheral;
use btleplug::api::BDAddr;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use eframe::egui::global_theme_preference_switch;

/// Run the measurent GUI
///
/// This is basically the only thing we run from main at this point
pub async fn run_gui() -> u32 {
    let (gui_tx, thread_rx) = unbounded_channel();
    let (thread_tx, gui_rx) = unbounded_channel();

    let mgr_handle = tokio::spawn(peripheral::mgr_run(thread_tx, thread_rx));

    // determine path for storage
    let storage_path = match std::env::home_dir() {
        Some(mut home_dir) => {
            home_dir.push("MeasureHub");
            Some(home_dir)
        }
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

            Ok(Box::<MeasureApp>::new(MeasureApp::new(cc, gui_tx, gui_rx)))
        }),
    )
    .is_err()
    {
        log::error!("GUI ended with error");
    }

    // wait for manager to join
    log::debug!("waiting for manager to join");
    mgr_handle.await.expect("PeripheralMgr thread has panicked");

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
    next: String,
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
    show: bool,
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
#[allow(unused)]
#[derive(Clone, Debug, PartialEq)]
enum GuiAction {
    Search,
    Connect,
    Read(BDAddr),
    Blink(BDAddr),
    Subscribe(BDAddr),
}

/// App state
#[derive(Clone, Debug)]
struct MeasureAppState {
    sensors: Vec<ConnectedSensor>,
    pending: CmdMgr,
}

/// Measurement GUI
struct MeasureApp {
    rx: UnboundedReceiver<PeripheralMsg>,
    tx: UnboundedSender<PeripheralCmd>,

    state: MeasureAppState,
}

impl MeasureApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        tx: UnboundedSender<PeripheralCmd>,
        rx: UnboundedReceiver<PeripheralMsg>,
    ) -> Self {
        let pending = CmdMgr::default();
        pending.start_handler();

        Self {
            tx,
            rx,
            state: MeasureAppState {
                sensors: Vec::<ConnectedSensor>::new(),
                pending,
            },
        }
    }

    fn find_sensors(&mut self) {
        log::info!("looking for sensors");

        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::FindSensors));
        self.tx.send(cmd).unwrap();
    }

    fn ping(&mut self) {
        log::debug!("GUI managing sensors:");
        dbg!(&self.state.sensors);
        log::debug!("GUI pending commands:");
        dbg!(&self.state.pending);

        log::info!("pinging manager");

        let cmd = self.state.pending.add(PeripheralCmd::new(HubCmd::Ping));
        self.tx.send(cmd).unwrap();
    }

    fn blink(&mut self, addr: BDAddr) {
        log::info!("blinking led");

        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::Blink(addr)));
        self.tx.send(cmd).unwrap();
    }

    fn connect(&mut self, addr: BDAddr) {
        log::info!("connecting to sensor");

        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::Connect(addr)));
        self.tx.send(cmd).unwrap();
    }

    fn blink_all(&mut self) {
        log::info!("blinking led");
        let cmd = self.state.pending.add(PeripheralCmd::new(HubCmd::BlinkAll));
        self.tx.send(cmd).unwrap();
    }

    fn read_sensor(&mut self, addr: BDAddr) {
        log::info!("reading from sensor");
        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::ReadFrom(addr)));
        self.tx.send(cmd).unwrap();
    }

    fn subscribe(&mut self, addr: BDAddr) {
        log::info!("subscribing to data from sensor");
        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::Subscribe(addr)));
        self.tx.send(cmd).unwrap();
    }

    fn unsubscribe(&mut self, addr: BDAddr) {
        log::info!("unsubscribing to data from sensor");
        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::Unsubscribe(addr)));
        self.tx.send(cmd).unwrap();
    }

    fn disconnect_all(&mut self) {
        for p in self.state.sensors.iter() {
            let cmd = self
                .state
                .pending
                .add(PeripheralCmd::new(HubCmd::Disconnect(p.addr)));
            self.tx.send(cmd).unwrap();
        }
    }

    fn connect_all(&mut self) {
        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::ConnectAll));
        self.tx.send(cmd).unwrap();
    }

    fn cleanup_and_exit(&mut self, ctx: egui::Context) {
        // tell PeripheralMgr to stop
        // handle is awaited in the runner
        let cmd = self
            .state
            .pending
            .add(PeripheralCmd::new(HubCmd::StopThread));
        self.tx.send(cmd).unwrap();

        // works (https://github.com/emilk/egui/discussions/4103#discussioncomment-9225022)
        std::thread::spawn(move || {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        });
    }

    // non-blocking to be run inside frame update
    // @todo: is there another way i could do this? or is this fine?
    fn run(&mut self) -> i8 {
        match self.rx.try_recv() {
            // handle Event message
            Ok(PeripheralMsg::Event(val)) => {
                log::debug!("Received event message");
                dbg!(&val);

                match val {
                    HubEvent::DeviceDiscovered(addr) => {
                        log::info!("Device Discovered: {addr:?}");
                        self.connect(addr);
                    }
                    HubEvent::NewData(addr, data) => {
                        log::info!("Async received new sensor data for {addr:?}");
                        if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                            p.value.update(data);
                        }
                    }
                    HubEvent::DeviceConnected(addr) => {
                        log::info!("Async Device Connected: {addr:?}");
                        self.state.sensors.push(ConnectedSensor {
                            addr,
                            name: SensorName::new(format!(
                                "Sensor {:?}",
                                self.state.sensors.len() + 1
                            )),
                            value: SensorValue::new(0),
                            subscribed: false,
                            show: false,
                        });
                    }
                    HubEvent::DeviceDisconnected(addr) => {
                        log::info!("Async Device Disconnected: {addr:?}");
                        let removed = self
                            .state
                            .sensors
                            .extract_if(.., |x| x.addr() == addr)
                            .collect::<Vec<_>>();
                        log::info!("Removed from UI: {removed:?}");
                    }
                };
            }

            // handle Response message
            Ok(PeripheralMsg::Response(id, val)) => {
                log::debug!("Received response message");
                dbg!(&val);

                if let Some(cmd) = self.state.pending.pop(id) {
                    // at least for now, let's ignore the possibility of multiple cmds found
                    log::debug!("Found matching command id in pending list for {val:?}!");

                    if cmd.validate_response(&val) {
                        log::debug!(
                            "received matching response type for command: {cmd:?} => {val:?}"
                        );
                    } else {
                        log::warn!(
                            "received invalid response type for command: {cmd:?} => {val:?}"
                        );
                        return 1;
                    }
                } else {
                    log::warn!(
                        "Received unexpected response, no matching command found: {id:?},{val:?}"
                    );
                    // issue if there is a pending command that is not resolved due to a wrong response id
                    //  e.g. cannot click connect, b/c the last connect command was never resolved
                    //  @todo add command timeout
                    return 1;
                }

                // handle the response
                match val {
                    HubResp::Failed => {
                        log::debug!("Command failed");
                    }
                    HubResp::Success => {}
                    HubResp::ReadData(addr, data) => {
                        log::info!("received read sensor data for {addr:?}");
                        if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                            p.value.update(data);
                        }
                    }
                };
            }

            // handle (or ignore) errors
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                return -1;
            }
        }
        0
    }

    /// Any Pending Commands
    ///
    /// Check if any commands to PeripheralMgr are open
    /// Response depends on the `action`,
    /// if None, any pending command returns true
    /// if Some, only pending commands falling in the command category return true
    fn any_pending(&self, action: Option<GuiAction>) -> bool {
        match action {
            None => !self.state.pending.is_empty(),
            Some(kind) => {
                let pending: Vec<_> = self
                    .state
                    .pending
                    .get_current()
                    .iter()
                    .map(|cmd| cmd.msg)
                    .collect();

                match kind {
                    GuiAction::Search => pending.iter().any(|c| matches!(c, HubCmd::FindSensors)),
                    GuiAction::Connect => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::ConnectAll | HubCmd::Connect(_))),
                    GuiAction::Read(addr) => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::ReadFrom(a) if *a == addr)),
                    GuiAction::Blink(addr) => pending.iter().any(|c| match c {
                        HubCmd::Blink(a) if *a == addr => true,
                        HubCmd::BlinkAll => true,
                        _ => false,
                    }),
                    GuiAction::Subscribe(addr) => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::Subscribe(a) if *a == addr)),
                }
            }
        }
    }
}

impl eframe::App for MeasureApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // run message handling
        if self.run() < 0 {
            // show info dialog, then exit
            egui::containers::Modal::new(egui::Id::new("modal dialog")).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label("The PeripheralMgr thread has run into an issue.\nPlease check your Bluetooth adapter and restart the program.");
                    if ui.button("Ok").clicked() {
                        let c = ctx.clone();
                        std::thread::spawn(move || {
                            c.send_viewport_cmd(egui::ViewportCommand::Close);
                        });
                    }
                });
            });
        }

        egui::SidePanel::left("left panel").show(ctx, |ui| {
            // general interface (independent from connected sensors)
            // TODO: use symbols here to make this more compact
            // TODO use this space for testing
            // i'd like to have the sensor list here and select which sensor to manage in the central panel (or something like that)
            ui.vertical(|ui| {
                global_theme_preference_switch(ui);

                if !self.state.sensors.is_empty() {
                    // Sensors label
                    ui.horizontal(|ui| {
                        ui.label("Sensors");
                        ui.add(egui::Separator::default().horizontal());
                    });

                    let mut sensors = self.state.sensors.clone();

                    // one button for each connected sensor
                    // TODO: test with more than one; when more than one selected, display next to each other like in a dashboard
                    for s in sensors.iter_mut() {
                        if ui
                            .toggle_value(
                                &mut s.show,
                                format!("{0} ({1})", s.name.value, s.value.value),
                            )
                            .clicked()
                        {
                            // works, the value is toggled automatically
                            log::debug!("sensor {0} clicked", s.name());
                            s.name.reset();
                        }
                    }
                    // update sensors
                    self.state.sensors = sensors.clone();
                }

                // General label
                ui.horizontal(|ui| {
                    ui.label("General");
                    ui.add(egui::Separator::default().horizontal());
                });

                if ui.button("Disconnect all").clicked() {
                    self.disconnect_all();
                }

                // exit
                if ui.button("Close App").clicked() {
                    log::info!("Closing app. Byebye!");
                    self.cleanup_and_exit(ctx.clone());
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Soil Measurement Sensor Hub");

            // enable button only when no search is in progress
            if ui
                .add_enabled(
                    !self.any_pending(Some(GuiAction::Search)),
                    egui::Button::new("Find Sensors"),
                )
                .clicked()
            {
                self.find_sensors();
            }
            // enable button only when no search is in progress
            if ui
                .add_enabled(
                    !self.any_pending(Some(GuiAction::Connect)),
                    egui::Button::new("Connect"),
                )
                .clicked()
            {
                self.connect_all();
            }

            ui.add(egui::Separator::default());

            // TODO: I have to take this ugly route to be able to iterate through the sensors mutably
            // with self.sensors.iter_mut() the whole self becomes mutably borrowed which messes things up here;
            // find out if there is a better (canonical) way to do this
            let mut sensors = self.state.sensors.clone();
            // one button for each connected sensor
            for s in sensors.iter_mut() {
                if !s.show {
                    continue;
                }

                // idea: have a 'box' per peripheral, with several buttons (read, blink, disconnect)
                ui.add(egui::Label::new(format!("{0} ({1})", s.name(), s.value())));
                ui.add(egui::Label::new(format!("{0}", s.addr())));
                if ui
                    .add_enabled(
                        !self.any_pending(Some(GuiAction::Read(s.addr))),
                        egui::Button::new("Read"),
                    )
                    .clicked()
                {
                    self.read_sensor(s.addr);
                }
                if ui
                    .add_enabled(
                        !self.any_pending(Some(GuiAction::Blink(s.addr))),
                        egui::Button::new("Blink"),
                    )
                    .clicked()
                {
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

                // show spinner while action in progress
                if self.any_pending(None) {
                    let label = if self.any_pending(Some(GuiAction::Connect)) {
                        String::from("Connecting")
                    } else if self.any_pending(Some(GuiAction::Search)) {
                        String::from("Searching")
                    } else {
                        String::new()
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(label);
                    });
                }
            });

            // yes, this updates the ui all the time, but this (no na) also causes cpu usage to go up
            // but we actually need this. the entire msg handling loop is also run in here.
            ui.ctx().request_repaint();
        });
    }
}
