//! HubGui
//!
//! Graphical interface for periperal management

use crate::cmdmgr::CmdMgr;
use crate::database_mgr::message::{DBCmd, DBResp, DatabaseCmd, DatabaseQuery, DatabaseResp};
use crate::database_mgr::{
    data::{DatabaseEntry, charting},
    database,
};
use crate::peripheral_mgr::message::{HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg};
use crate::peripheral_mgr::peripheral;
use btleplug::api::BDAddr;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use eframe::egui::global_theme_preference_switch;

/// Run the measurent GUI
///
/// This is basically the only thing we run from main at this point
pub async fn run_gui() -> u32 {
    // start peripheral manager
    let (periph_tx, thread_periph_rx) = unbounded_channel();
    let (thread_periph_tx, periph_rx) = unbounded_channel();
    let mgr_handle = tokio::spawn(peripheral::mgr_run(thread_periph_tx, thread_periph_rx));

    // spawn database manager
    let (db_tx, thread_db_rx) = unbounded_channel();
    let (thread_db_tx, db_rx) = unbounded_channel();
    let db_handle = tokio::spawn(database::mgr_run(thread_db_tx, thread_db_rx));

    // eframe options
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    if eframe::run_native(
        "Measurement Hub",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<MeasureApp>::new(MeasureApp::new(
                cc,
                (periph_tx, periph_rx),
                (db_tx, db_rx),
            )))
        }),
    )
    .is_err()
    {
        log::error!("GUI ended with error");
    }

    // wait for manager to join
    let (mgr_res, db_res) = tokio::join!(mgr_handle, db_handle);
    if let Err(e) = mgr_res {
        log::warn!("PeripheralMgr thread has panicked: {e}")
    }
    if let Err(e) = db_res {
        log::warn!("DatabaseMgr thread has panicked: {e}")
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

/// Connected Sensor
///
/// Represents one connected sensor with device address and name
#[derive(Clone, Debug)]
struct ConnectedSensor {
    addr: BDAddr,
    name: SensorName,
    last: u32,
    subscribed: bool,
    show: bool,
    id: i32,
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
        self.last
    }

    /// Get the sensor's address
    pub fn addr(&self) -> BDAddr {
        self.addr
    }

    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn in_db(&self) -> bool {
        self.id != 0
    }
}

/// UiAction
///
/// enum denoting currently running actions
/// mostly used for displaying spinner
#[derive(Clone, Debug, PartialEq)]
enum UiAction {
    Peripheral(PeripheralAction),
    Database(DatabaseAction),
}

/// PeripheralAction
///
/// enum for actions wrt PeripheralMgr
#[allow(unused)]
#[derive(Clone, Debug, PartialEq)]
enum PeripheralAction {
    Any,
    Search,
    Connect,
    Read(BDAddr),
    Blink(BDAddr),
    Subscribe(BDAddr),
}

/// DatabaseAction
///
/// enum for actions wrt DatabaseMgr
#[allow(unused)]
#[derive(Clone, Debug, PartialEq)]
enum DatabaseAction {
    Any,
    WriteSensor(BDAddr),
    DeleteSensor(i32),
    WriteData(i32),
    ReadData(i32),
}

/// App state
#[derive(Clone, Debug)]
struct MeasureAppState {
    sensors: Vec<ConnectedSensor>,
}

/// Mgr connection
/// Generic to hold sender and receiver for a mgr connection
struct MgrConnection<T, U> {
    tx: UnboundedSender<T>,
    rx: UnboundedReceiver<U>,
    pending: CmdMgr<T>,
}

/// Measurement GUI
struct MeasureApp {
    peripheral: MgrConnection<PeripheralCmd, PeripheralMsg>,
    database: MgrConnection<DatabaseCmd, DatabaseResp>,

    state: MeasureAppState,
}

impl MeasureApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        periph_channels: (
            UnboundedSender<PeripheralCmd>,
            UnboundedReceiver<PeripheralMsg>,
        ),
        db_channels: (
            UnboundedSender<DatabaseCmd>,
            UnboundedReceiver<DatabaseResp>,
        ),
    ) -> Self {
        let pending_p = CmdMgr::default();
        pending_p.start_handler();

        let pending_db = CmdMgr::default();
        pending_db.start_handler();

        Self {
            peripheral: MgrConnection {
                tx: periph_channels.0,
                rx: periph_channels.1,
                pending: pending_p,
            },
            database: MgrConnection {
                tx: db_channels.0,
                rx: db_channels.1,
                pending: pending_db,
            },
            state: MeasureAppState {
                sensors: Vec::<ConnectedSensor>::new(),
            },
        }
    }

    /// Send Command to the database
    ///
    /// This also adds the command to the pending database actions
    fn send_database_command(&mut self, cmd: DBCmd) {
        let cmd = self.database.pending.add(DatabaseCmd::new(cmd));
        self.database.tx.send(cmd).unwrap();
    }

    /// Send Command to the Peripheral manager
    ///
    /// This also adds the command to the pending peripheral actions
    fn send_peripheral_command(&mut self, cmd: HubCmd) {
        let cmd = self.peripheral.pending.add(PeripheralCmd::new(cmd));
        self.peripheral.tx.send(cmd).unwrap();
    }

    fn find_sensors(&mut self) {
        log::info!("looking for sensors");
        self.send_peripheral_command(HubCmd::FindSensors);
    }

    fn ping(&mut self) {
        log::debug!("GUI managing sensors:");
        dbg!(&self.state.sensors);
        log::debug!("GUI pending commands:");
        dbg!(&self.peripheral.pending);

        log::info!("pinging manager");
        self.send_peripheral_command(HubCmd::Ping);
    }

    fn ping_db(&mut self) {
        log::info!("pinging database");
        self.send_database_command(DBCmd::Ping);
    }

    fn blink(&mut self, addr: BDAddr) {
        log::info!("blinking led");
        self.send_peripheral_command(HubCmd::Blink(addr));
    }

    fn connect(&mut self, addr: BDAddr) {
        log::info!("connecting to sensor");
        self.send_peripheral_command(HubCmd::Connect(addr));
    }

    fn blink_all(&mut self) {
        log::info!("blinking led");
        self.send_peripheral_command(HubCmd::BlinkAll);
    }

    fn read_sensor(&mut self, addr: BDAddr) {
        log::info!("reading from sensor");
        self.send_peripheral_command(HubCmd::ReadFrom(addr));
    }

    fn subscribe(&mut self, addr: BDAddr) {
        log::info!("subscribing to data from sensor");
        self.send_peripheral_command(HubCmd::Subscribe(addr));
    }

    fn unsubscribe(&mut self, addr: BDAddr) {
        log::info!("unsubscribing to data from sensor");
        self.send_peripheral_command(HubCmd::Unsubscribe(addr));
    }

    fn disconnect_all(&mut self) {
        let sensors = self.state.sensors.clone();
        for p in sensors.iter() {
            self.send_peripheral_command(HubCmd::Disconnect(p.addr));
        }
    }

    fn connect_all(&mut self) {
        self.send_peripheral_command(HubCmd::ConnectAll);
    }

    fn cleanup_and_exit(&mut self, ctx: egui::Context) {
        // tell PeripheralMgr to stop
        self.send_peripheral_command(HubCmd::StopThread);

        // tell database mrg to stop
        self.send_database_command(DBCmd::StopThread);

        // works (https://github.com/emilk/egui/discussions/4103#discussioncomment-9225022)
        std::thread::spawn(move || {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        });
    }

    // non-blocking to be run inside frame update
    // @todo: is there another way i could do this? or is this fine?
    fn run(&mut self) -> i8 {
        match self.peripheral.rx.try_recv() {
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
                            p.last = data;
                            let id = p.id();
                            if p.in_db() {
                                self.send_database_command(DBCmd::AddEntry(
                                    id,
                                    chrono::Local::now(),
                                    data,
                                ));
                            }
                        }
                    }
                    HubEvent::DeviceConnected(addr) => {
                        log::info!("Async Device Connected: {addr:?}");
                        let sensorname =
                            SensorName::new(format!("Sensor {:?}", self.state.sensors.len() + 1));

                        self.state.sensors.push(ConnectedSensor {
                            addr,
                            name: sensorname.clone(),
                            last: 0,
                            subscribed: false,
                            show: false,
                            id: 0,
                        });
                        self.send_database_command(DBCmd::AddSensor(
                            u64::from(addr),
                            sensorname.value,
                        ));
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

                if let Some(cmd) = self.peripheral.pending.pop(id) {
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
                            p.last = data;
                            let id = p.id();
                            if p.in_db() {
                                self.send_database_command(DBCmd::AddEntry(
                                    id,
                                    chrono::Local::now(),
                                    data,
                                ));
                            }
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

        match self.database.rx.try_recv() {
            // handle response message
            Ok(DatabaseResp::Response(id, val)) => {
                log::debug!("Received database response message");
                if let Some(cmd) = self.database.pending.pop(id) {
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
                    return 1;
                }
                match val {
                    DBResp::SensorAdded(a) => {
                        if let Some(_p) = self
                            .state
                            .sensors
                            .iter_mut()
                            .find(|p| u64::from(p.addr) == a)
                        {
                            // request sensor id
                            log::debug!("Sensor was added to database. requesting id");
                            self.send_database_command(DBCmd::Get(DatabaseQuery::SensorID(a)));
                        }
                    }
                    DBResp::SensorKnown(a, name, id) => {
                        if let Some(p) = self
                            .state
                            .sensors
                            .iter_mut()
                            .find(|p| u64::from(p.addr) == a)
                        {
                            p.name.next = name;
                            p.name.update();
                            p.id = id;
                        }
                    }
                    DBResp::SensorDeleted(id) => {
                        if let Some(p) = self.state.sensors.iter_mut().find(|p| p.id == id) {
                            p.id = 0;
                        }
                    }
                    DBResp::SensorId(a, id) => {
                        if let Some(p) = self
                            .state
                            .sensors
                            .iter_mut()
                            .find(|p| u64::from(p.addr) == a)
                        {
                            log::debug!("Got sensor Id for sensor");
                            p.id = id;
                        }
                    }
                    DBResp::Success => (),
                    DBResp::Failed => (),
                    DBResp::Data(_vec) => (),
                }
            }

            // handle (or ignore) errors
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                return -1;
            }
        }
        0
    }

    /// Any pending Commands
    ///
    /// Check if any commands to PeripheralMgr or Database are open
    /// Response depends on the `action`
    fn any_pending(&self, action: UiAction) -> bool {
        match action {
            UiAction::Peripheral(act) => {
                let kind = act;
                let pending: Vec<_> = self
                    .peripheral
                    .pending
                    .get_current()
                    .iter()
                    .map(|cmd| cmd.msg)
                    .collect();

                match kind {
                    PeripheralAction::Any => !pending.is_empty(),
                    PeripheralAction::Search => {
                        pending.iter().any(|c| matches!(c, HubCmd::FindSensors))
                    }
                    PeripheralAction::Connect => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::ConnectAll | HubCmd::Connect(_))),
                    PeripheralAction::Read(addr) => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::ReadFrom(a) if *a == addr)),
                    PeripheralAction::Blink(addr) => pending.iter().any(|c| match c {
                        HubCmd::Blink(a) if *a == addr => true,
                        HubCmd::BlinkAll => true,
                        _ => false,
                    }),
                    PeripheralAction::Subscribe(addr) => pending
                        .iter()
                        .any(|c| matches!(c, HubCmd::Subscribe(a) if *a == addr)),
                }
            }
            UiAction::Database(act) => {
                let kind = act;
                let pending: Vec<_> = self
                    .database
                    .pending
                    .get_current()
                    .iter()
                    .map(|cmd| cmd.msg.clone())
                    .collect();

                match kind {
                    DatabaseAction::Any => !pending.is_empty(),
                    DatabaseAction::WriteSensor(addr) => pending
                        .iter()
                        .any(|c| matches!(c, DBCmd::AddSensor(a, _) if *a == u64::from(addr))),
                    DatabaseAction::DeleteSensor(id) => pending
                        .iter()
                        .any(|c| matches!(c, DBCmd::DeleteSensor(i) if *i == id)),
                    DatabaseAction::WriteData(id) => pending
                        .iter()
                        .any(|c| matches!(c, DBCmd::AddEntry(i, _, _) if *i == id)),
                    DatabaseAction::ReadData(id) => pending.iter().any(|c| match c {
                        DBCmd::Get(query) => match query {
                            DatabaseQuery::TsAfter(i, _) if *i == id => true,
                            DatabaseQuery::TsBefore(i, _) if *i == id => true,
                            DatabaseQuery::TsDuration(i, _, _) if *i == id => true,
                            _ => false,
                        },
                        _ => false,
                    }),
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
                    ui.label("A thread has run into an issue.\nPlease check your Bluetooth adapter and database server and restart the program.");
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
                            .toggle_value(&mut s.show, format!("{0} ({1})", s.name.value, s.last))
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
                    !self.any_pending(UiAction::Peripheral(PeripheralAction::Search)),
                    egui::Button::new("Find Sensors"),
                )
                .clicked()
            {
                self.find_sensors();
            }
            // enable button only when no search is in progress
            if ui
                .add_enabled(
                    !self.any_pending(UiAction::Peripheral(PeripheralAction::Connect)),
                    egui::Button::new("Connect"),
                )
                .clicked()
            {
                self.connect_all();
            }

            ui.add(egui::Separator::default());

            egui::ScrollArea::vertical().show(ui, |ui| {
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
                            !self.any_pending(UiAction::Peripheral(PeripheralAction::Read(s.addr))),
                            egui::Button::new("Read"),
                        )
                        .clicked()
                    {
                        self.read_sensor(s.addr);
                    }
                    if ui
                        .add_enabled(
                            !self
                                .any_pending(UiAction::Peripheral(PeripheralAction::Blink(s.addr))),
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
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut s.name.next));

                        if ui.button("Change Name").clicked() {
                            log::debug!("name change requested");
                            s.name.update();
                            self.send_database_command(DBCmd::UpdateSensor(
                                s.id,
                                s.name.value.clone(),
                            ));
                        }
                    });

                    if s.in_db() {
                        if ui.button("get stored data").clicked() {
                            self.get_data(s.id);
                        }
                        if ui
                            .add_enabled(
                                !self.any_pending(UiAction::Database(
                                    DatabaseAction::DeleteSensor(s.id),
                                )),
                                egui::Button::new("Delete from DB"),
                            )
                            .clicked()
                        {
                            log::debug!("removing sensor from database");
                            self.send_database_command(DBCmd::DeleteSensor(s.id));
                        }
                    } else if ui
                        .add_enabled(
                            !self.any_pending(UiAction::Database(DatabaseAction::WriteSensor(
                                s.addr,
                            ))),
                            egui::Button::new("Add to DB"),
                        )
                        .clicked()
                    {
                        log::debug!("adding sensor to database");
                        self.send_database_command(DBCmd::AddSensor(u64::from(s.addr), s.name()));
                    }

                    ui.add(egui::Separator::default());
                }

                // update sensors (in case smth was changed)
                self.state.sensors = sensors.clone();
            });

            // general interface (independent from connected sensors)
            ui.horizontal(|ui| {
                if ui.button("Ping").clicked() {
                    self.ping();
                }

                if ui.button("Ping DB").clicked() {
                    self.ping_db();
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
                if self.any_pending(UiAction::Peripheral(PeripheralAction::Any)) {
                    let label = if self.any_pending(UiAction::Peripheral(PeripheralAction::Connect))
                    {
                        String::from("Connecting")
                    } else if self.any_pending(UiAction::Peripheral(PeripheralAction::Search)) {
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
