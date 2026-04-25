//! Ui implementation using the slint crate

// unfortuneataly need this for the slint! macro, unreadable otherwise
#![allow(missing_docs)]

use crate::cmdmgr::CmdMgr;
use crate::database_mgr::{
    data::charting,
    database,
    message::{DBCmd, DBResp, DatabaseCmd, DatabaseQuery, DatabaseResp},
};
use crate::peripheral_mgr;
use crate::peripheral_mgr::message::{HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg};
use btleplug::api::BDAddr;
use chrono::Local;
use std::collections::HashMap;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use slint;
use slint::Model;
use slint::run_event_loop_until_quit;

slint::include_modules!();

#[derive(Debug, Clone)]
enum UiCmd {
    /// Ping
    Ping,
    /// Check for new sensors
    FindSensors,
    /// Connect to sensor with addr
    Connect(i32),
    /// Connect to all known sensors
    ConnectAll,
    /// Disconnect from sensor with addr
    Disconnect(i32),
    /// Subscribe to receive data from sensor with addr
    Subscribe(i32),
    /// Unsubscribe from data from sensor with addr
    Unsubscribe(i32),
    /// Read data from sensor with addr
    ReadFrom(i32),
    /// Blink sensor with addr
    Blink(i32),
    /// Blink all connected sensors
    BlinkAll,

    UpdateName(i32, String),
    /// stop the PeripheralMgr thread
    StopThread,
}

/// run the slint ui
pub async fn run_gui() -> u32 {
    // start peripheral manager
    let (periph_tx, thread_periph_rx) = unbounded_channel();
    let (thread_periph_tx, periph_rx) = unbounded_channel();
    let mgr_handle = slint::spawn_local(async_compat::Compat::new(peripheral_mgr::mgr_run(
        thread_periph_tx,
        thread_periph_rx,
    )))
    .unwrap();

    // spawn database manager
    let (db_tx, thread_db_rx) = unbounded_channel();
    let (thread_db_tx, db_rx) = unbounded_channel();
    let db_handle = slint::spawn_local(async_compat::Compat::new(database::mgr_run(
        thread_db_tx,
        thread_db_rx,
    )))
    .unwrap();

    // new Ui
    let ui = HubUi::new().unwrap();

    // start ui manager
    let (ui_tx, ui_thread_rx) = unbounded_channel();
    let weak_ui = ui.as_weak();
    let ui_handle = slint::spawn_local(async_compat::Compat::new(ui_mgr_run(
        (periph_tx, periph_rx),
        (db_tx, db_rx),
        weak_ui,
        ui_thread_rx,
    )))
    .unwrap();

    // ui.on_add_sensor
    // called from backend
    let weak_ui = ui.as_weak();
    ui.on_add_sensor(move |sensor| {
        let mut current_sensors: Vec<Sensor> = weak_ui.unwrap().get_sensors().iter().collect();
        current_sensors.push(sensor);
        let model = std::rc::Rc::new(slint::VecModel::from(current_sensors));
        weak_ui.unwrap().set_sensors(model.into());
    });

    // ui.on_update_sensor
    // called from backend
    let weak_ui = ui.as_weak();
    ui.on_update_sensor(move |sensor| {
        log::debug!("update_sensor callback called: {sensor:?}");
        let mut current_sensors: Vec<Sensor> = weak_ui.unwrap().get_sensors().iter().collect();
        if let Some(old) = current_sensors
            .iter_mut()
            .find(|p| p.sensor_id == sensor.sensor_id)
        {
            // want to keep value of 'show' since that is ui-only
            let show = old.show;
            *old = sensor;
            old.show = show;
        }
        let model = std::rc::Rc::new(slint::VecModel::from(current_sensors));
        weak_ui.unwrap().set_sensors(model.into());
    });

    // ui.on_sensor_subscribe
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_subscribe(move |id, val| {
        log::debug!("subscribing to sensor");
        if val {
            tx.send(UiCmd::Subscribe(id)).unwrap();
        } else {
            tx.send(UiCmd::Unsubscribe(id)).unwrap();
        }
    });

    // ui.on_ping
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_ping(move || {
        log::debug!("Ping!");
        tx.send(UiCmd::Ping).unwrap();
    });

    // ui.on_sensor_connect
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_connect(move |id, connect| {
        if connect {
            log::debug!("UI connect to sensor!");
            tx.send(UiCmd::Connect(id)).unwrap();
        } else {
            log::debug!("UI disconnect from sensor!");
            tx.send(UiCmd::Disconnect(id)).unwrap();
        }
    });

    // ui.on_blink
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_blink(move |id| {
        log::debug!("blinking sensor");
        tx.send(UiCmd::Blink(id)).unwrap();
    });

    // ui.on_sensor_read
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_read(move |id| {
        log::debug!("reading from sensor");
        tx.send(UiCmd::ReadFrom(id)).unwrap();
    });

    // ui.on_sensor_name
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_name(move |id, name| {
        log::debug!("sensor name changed sensor");
        tx.send(UiCmd::UpdateName(id, name.to_string())).unwrap();
    });

    // ui.on_close
    //
    let tx = ui_tx.clone();
    let weak_ui = ui.as_weak();
    ui.on_close_button(move || {
        // this only tells ui-mgr thread to stop
        // which in turn sends stop signal to other mgrs
        // a repeated timer is running alongside all of this, which triggers
        // an event/callback if all manager threads have stopped.
        // in the callback, we check if
        log::debug!("ui stopped, sending stop to managers");
        tx.send(UiCmd::StopThread).unwrap();
        let _ = weak_ui.unwrap().hide();

        // backup timer to hard stop if threads are not finished after 10 secs
        slint::Timer::single_shot(std::time::Duration::from_secs(10), move || {
            log::warn!("threads did not stop after 10 sec");
            slint::quit_event_loop().unwrap();
        });
    });

    // ui.on_stop_condition
    let weak_ui = ui.as_weak();
    ui.on_stop_condition(move || {
        let ui = weak_ui.unwrap();
        if ui.get_exit_initiated() {
            // exit was initiated and all threads are done: exit
            slint::quit_event_loop().unwrap();
        } else {
            // exit was not initiated but for some reason all threads have stopped
            log::warn!("Background threads have stopped working!");
        }
    });

    // task/thread to handle the other thread handles
    // but how do i make it run without returning it here?
    let weak_ui = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(500),
        move || {
            if mgr_handle.is_finished() && db_handle.is_finished() && ui_handle.is_finished() {
                let ui = weak_ui.unwrap();
                ui.invoke_stop_condition();
            }
        },
    );

    // run the ui
    // use run_event_loop_until_quit so we can hide the window while waiting for threads to stop
    let _ = ui.show();
    match run_event_loop_until_quit() {
        Ok(_) => (),
        Err(e) => log::debug!("Slint UI ended with error: {e}"),
    };
    let _ = ui.hide();

    0
}

/// Run the Ui manager
async fn ui_mgr_run(
    periph_channels: (
        UnboundedSender<PeripheralCmd>,
        UnboundedReceiver<PeripheralMsg>,
    ),
    db_channels: (
        UnboundedSender<DatabaseCmd>,
        UnboundedReceiver<DatabaseResp>,
    ),
    ui: slint::Weak<HubUi>,
    main_rx: UnboundedReceiver<UiCmd>,
) -> u32 {
    let mut app = MeasureApp::new(periph_channels, db_channels, ui, main_rx);
    app.run().await;

    log::debug!("Ui manager has ended");
    0
}

/// App state
#[derive(Clone, Debug)]
struct MeasureAppState {
    ui_sensors: Vec<Sensor>,
    sensor_map: HashMap<BDAddr, i32>,
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
    ui: slint::Weak<HubUi>,
    main_rx: UnboundedReceiver<UiCmd>,

    state: MeasureAppState,
}

impl MeasureApp {
    pub fn new(
        periph_channels: (
            UnboundedSender<PeripheralCmd>,
            UnboundedReceiver<PeripheralMsg>,
        ),
        db_channels: (
            UnboundedSender<DatabaseCmd>,
            UnboundedReceiver<DatabaseResp>,
        ),
        ui: slint::Weak<HubUi>,
        main_rx: UnboundedReceiver<UiCmd>,
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
            ui,
            main_rx,
            state: MeasureAppState {
                ui_sensors: Vec::new(),
                sensor_map: HashMap::new(),
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

    /// Print debug info
    fn ping(&mut self) {
        log::debug!("GUI managing sensors:");
        dbg!(&self.state.ui_sensors);
        log::debug!("GUI pending commands:");
        dbg!(&self.peripheral.pending);
        dbg!(&self.database.pending);

        log::info!("pinging manager");
        self.send_peripheral_command(HubCmd::Ping);
    }

    /// Connect to sensor
    fn connect(&mut self, addr: BDAddr) {
        log::info!("connecting to sensor");
        let ui = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_connect_add());
        self.send_peripheral_command(HubCmd::Connect(addr));
    }

    fn get_data(&mut self, id: i32) {
        self.send_database_command(DBCmd::Get(DatabaseQuery::TsBefore(id, Local::now())));
    }

    fn cleanup_and_exit(&mut self) {
        log::debug!("uiMgr cleanup&exit");
        // tell PeripheralMgr to stop
        self.send_peripheral_command(HubCmd::StopThread);

        // tell database mrg to stop
        self.send_database_command(DBCmd::StopThread);
    }

    /// Updates a sensor value
    ///
    /// set the value of the sensor to new value,
    /// if the sensor is in the DB, store value in DB
    /// and lastly update sensor in UI
    fn update_sensor_value(&mut self, addr: BDAddr, data: i32) {
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    s.last = data as i32;
                    Some(s.clone())
                }
                None => None,
            };

            if let Some(s) = sensor {
                if s.db_id != 0 {
                    self.send_database_command(DBCmd::AddEntry(
                        s.db_id,
                        chrono::Local::now(),
                        data as u32,
                    ));
                }
                let ui = self.ui.clone();
                let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_update_sensor(s));
            }
        } else {
            log::debug!("did not find sensor for updating: {addr:?}");
            dbg!(&self.state.ui_sensors);
        }
    }

    /// Updates a sensor connection value
    ///
    /// set the value of the sensor to new value,
    /// if the sensor is in the DB, store value in DB
    /// and lastly update sensor in UI
    fn update_sensor_connected(
        &mut self,
        addr: BDAddr,
        connected: Option<bool>,
        subscribed: Option<bool>,
    ) {
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    if let Some(connected) = connected {
                        s.connected = connected;
                    }
                    if let Some(subscibed) = subscribed {
                        s.subscribed = subscibed;
                    }
                    Some(s.clone())
                }
                None => None,
            };

            if let Some(s) = sensor {
                let ui = self.ui.clone();
                let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_update_sensor(s));
            }
        } else {
            log::debug!("did not find sensor for updating: {addr:?}");
            dbg!(&self.state.ui_sensors);
        }
    }

    /// Update sensor name and id
    ///
    /// convenience function for database events
    /// updates the sensor with the provided
    fn update_sensor_name_and_id(
        &mut self,
        addr: BDAddr,
        db_name: Option<String>,
        db_id: Option<i32>,
    ) {
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    if let Some(new_name) = db_name {
                        s.name = new_name.into();
                    }
                    if let Some(new_id) = db_id {
                        s.db_id = new_id;
                    }
                    Some(s.clone())
                }
                None => None,
            };

            if let Some(s) = sensor {
                let ui = self.ui.clone();
                let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_update_sensor(s));
            } else {
                log::debug!("did not find sensor for name and id update: {addr:?}");
            }
        } else {
            log::debug!("did not find sensor id for name and id update: {addr:?}");
            dbg!(&self.state.ui_sensors);
        }
    }

    /// run thread comms
    async fn run(&mut self) -> i8 {
        loop {
            tokio::select! {
                Some(msg) = self.peripheral.rx.recv() => {
                    match msg {
                        // handle Event message
                        PeripheralMsg::Event(val) => {
                            log::debug!("Received event message");
                            dbg!(&val);

                            match val {
                                HubEvent::DeviceDiscovered(addr) => {
                                    log::info!("Device Discovered: {addr:?}");
                                    let sensorname = format!("Sensor {:?}", self.state.ui_sensors.len() + 1);
                                    let sensor_id: i32 = self.state.ui_sensors.len() as i32;

                                    let sens = Sensor {
                                        sensor_id,
                                        name: sensorname.clone().into(),
                                        last: 0,
                                        connected: false,
                                        subscribed: false,
                                        show: false,
                                        db_id: 0
                                    };

                                    // add sensor to UI
                                    self.state.ui_sensors.push(sens.clone());
                                    self.state.sensor_map.insert(addr, sensor_id);

                                    let ui = self.ui.clone();
                                    let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_add_sensor(sens));

                                    self.send_database_command(DBCmd::AddSensor(
                                        u64::from(addr),
                                        sensorname,
                                    ));

                                    self.connect(addr);
                                }
                                HubEvent::NewData(addr, data) => {
                                    log::info!("Async received new sensor data for {addr:?}");

                                    // update UI
                                    self.update_sensor_value(addr, data as i32);
                                }
                                HubEvent::DeviceConnected(addr) => {
                                    log::info!("Async Device Connected: {addr:?}");

                                    let ui = self.ui.clone();
                                    let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_connect_del());

                                    self.update_sensor_connected(addr, Some(true), None);


                                }
                                HubEvent::DeviceDisconnected(addr) => {
                                    log::info!("Async Device Disconnected: {addr:?}");
                                    self.update_sensor_connected(addr, Some(false), Some(false));
                                }
                            };
                        }

                        // handle Response message
                        PeripheralMsg::Response(id, val) => {
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
                                    self.update_sensor_value(addr, data as i32);
                                }
                            };
                        }
                    }
                }

                Some(msg) = self.database.rx.recv() => {
                    match msg {
                        // handle response message
                        DatabaseResp::Response(id, val) => {
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

                                    // request sensor id
                                    log::debug!("Sensor was added to database. requesting id");
                                    self.send_database_command(DBCmd::Get(DatabaseQuery::SensorID(a)));
                                }
                                DBResp::SensorKnown(a, name, id) => {

                                    if let Some(addr) = self.state
                                    .sensor_map
                                    .iter()
                                    .find_map(|(key, &_val)| if u64::from(*key) == a { Some(key) } else { None })
                                    .copied() {
                                        self.update_sensor_name_and_id(addr, Some(name), Some(id));
                                    } else {
                                        log::debug!("received sensorKnown for unknown sensor! {name}, {id}")
                                    }

                                }
                                DBResp::SensorDeleted(id) => {

                                    let mut sensor_id = 0;
                                    if let Some(s) = self.state.ui_sensors.iter_mut().find(|s| s.db_id == id) {
                                        s.db_id = 0;
                                        sensor_id = s.sensor_id;
                                    }

                                    if let Some(addr) = self.addr(sensor_id) {
                                        self.update_sensor_name_and_id(addr, None, Some(0));
                                    }
                                }
                                DBResp::SensorId(a, id) => {

                                    if let Some(addr) = self.state
                                    .sensor_map
                                    .iter()
                                    .find_map(|(key, &_val)| if u64::from(*key) == a { Some(key) } else { None })
                                    .copied() {
                                        self.update_sensor_name_and_id(addr, None, Some(id));
                                    } else {
                                        log::debug!("Sensor not found for id change");
                                    }


                                }
                                DBResp::Success => (),
                                DBResp::Failed => (),
                                DBResp::Data(vec) => {
                                    log::debug!("received data!");
                                    // just for testing: draw received data
                                    // todo: find out how i can directly display in gui popup
                                    dbg!(&vec);
                                    charting::draw_chart("", vec);
                                }
                            }
                        }
                    }
                }

                res = self.main_rx.recv() => {
                    if let Some(msg) = res {
                        match msg {
                            UiCmd::StopThread => {
                                log::debug!("received stop command from main");
                                self.cleanup_and_exit();
                                break;
                            }
                            UiCmd::Ping => {
                                log::debug!("ui mgr received Ping command");
                                self.ping();
                            }
                            UiCmd::Connect(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    log::debug!("ui mgr received Connect({addr:?}) command");
                                    self.connect(addr);
                                }
                            }
                            UiCmd::Disconnect(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    log::debug!("ui mgr received Disconnect({addr:?}) command");
                                    self.send_peripheral_command(HubCmd::Disconnect(addr));
                                }
                            }
                            UiCmd::ConnectAll=> {
                                log::debug!("ui mgr received ConnectAll command");
                                self.send_peripheral_command(HubCmd::ConnectAll);
                            }
                            UiCmd::Subscribe(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::Subscribe(addr));
                                    self.update_sensor_connected(addr, None, Some(true));
                                    log::debug!("ui mgr received Subscribe({addr:?}) command");
                                }
                            }
                            UiCmd::Unsubscribe(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::Unsubscribe(addr));
                                    self.update_sensor_connected(addr, None, Some(false));
                                    log::debug!("ui mgr received Unsubscribe({addr:?}) command");
                                }
                            }
                            UiCmd::ReadFrom(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::ReadFrom(addr));
                                    log::debug!("ui mgr received Subscribe({addr:?}) command");
                                }
                            }
                            UiCmd::FindSensors => {
                                log::debug!("ui mgr received FindSensors command");
                                self.send_peripheral_command(HubCmd::FindSensors);
                            }
                            UiCmd::Blink(id) => {
                                log::debug!("ui mgr received Blink({id:?}) command");
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::Blink(addr));
                                }
                            }
                            UiCmd::BlinkAll => {
                                log::debug!("ui mgr received BlinkAll command");
                                self.send_peripheral_command(HubCmd::BlinkAll);
                            }
                            UiCmd::UpdateName(id, name) => {
                                // sensor name is changed from UI
                                log::debug!("ui mgr received update name command");
                                // update sensor in vec
                                // update database entry
                                let sensor =  match self.state.ui_sensors.iter_mut().find(|s| s.sensor_id == id) {
                                    Some(s) => {
                                        s.name = name.into();
                                        Some(s.clone())
                                    }
                                    None => None
                                };
                                if let Some(s) = sensor {
                                    self.send_database_command(DBCmd::UpdateSensor(s.db_id, s.name.to_string()));
                                    // also update sensor name here
                                    let ui = self.ui.clone();
                                    let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_update_sensor(s));
                                }
                            }
                        }
                    } else {
                        log::warn!("channel to main is closed");
                        self.cleanup_and_exit();
                        break;
                    }
                }
            }
        }
        log::debug!("UiMgr run() complete");
        0
    }

    /// Find sensor peripheral address from ui id
    fn addr(&self, id: i32) -> Option<BDAddr> {
        self.state
            .sensor_map
            .iter()
            .find_map(|(key, &val)| if val == id { Some(key) } else { None })
            .copied()
    }

    fn id(&self, addr: BDAddr) -> Option<i32> {
        self.state.sensor_map.get(&addr).copied()
    }
}
