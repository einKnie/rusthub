//! Ui implementation using the slint crate

// unfortuneataly need this for the slint! macro, unreadable otherwise
#![allow(missing_docs)]

use crate::cmdmgr::{Command, CmdMgr};
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
use std::sync::atomic::{AtomicU16, Ordering};

use slint;
use slint::Model;
use slint::run_event_loop_until_quit;
use slint::CloseRequestResponse;

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
    /// update sensor name (ui -> mgr)!
    UpdateName(i32, String),
    /// stop the PeripheralMgr thread
    StopThread,
    // delete sensor from database
    DbDeleteSensor(i32),
    /// fetch data for sensor from db
    DbGetData(i32),
}

/// run the slint ui
/// tbh not sure how i can structure this to be less spaghetti
pub async fn run_gui() -> u32 {
    // HubUi is generated (in build.rs) from 'sensorui.slint'
    let ui = HubUi::new().unwrap();

    //
    // 1. START MANAGER THREADS
    // Initially, i thought this only works by starting the threads with slint::spawn_local,
    // but this runs the code within the main thread event loop, which means a panic in a mgr
    // (e.g. db_mgr when database server is not running)
    // will actually crash the main thread. So this is not feasible.
    // turns out that it works with tokio, although now i'm not sure what the initial issue was.
    // could still be that i need the async_compat, but for now let's see, so far it works without

    // spawn peripheral manager
    let (periph_tx, thread_periph_rx) = unbounded_channel();
    let (thread_periph_tx, periph_rx) = unbounded_channel();
    let mgr_handle = tokio::spawn(peripheral_mgr::mgr_run(thread_periph_tx, thread_periph_rx));

    // spawn database manager
    let (db_tx, thread_db_rx) = unbounded_channel();
    let (thread_db_tx, db_rx) = unbounded_channel();
    let db_handle = tokio::spawn(database::mgr_run(thread_db_tx, thread_db_rx));

    // spawn ui manager
    let (ui_tx, ui_thread_rx) = unbounded_channel();
    let weak_ui = ui.as_weak();
    let ui_handle = tokio::spawn(ui_mgr_run(
        (periph_tx, periph_rx),
        (db_tx, db_rx),
        weak_ui,
        ui_thread_rx,
    ));

    //
    // 2. DEFINE UI CALLBACKS
    //

    // ui.on_add_sensor
    // called from backend
    let weak_ui = ui.as_weak();
    ui.on_add_sensor(move |sensor| {
        log::debug!("add_sensor callback called: {sensor:?}");
        let mut current_sensors: Vec<Sensor> = weak_ui.unwrap().get_sensors().iter().collect();
        current_sensors.push(sensor);
        let model = std::rc::Rc::new(slint::VecModel::from(current_sensors));
        weak_ui.unwrap().set_sensors(model.into());
    });

    // ui.on_remove_sensor
    // called from backend
    let weak_ui = ui.as_weak();
    ui.on_remove_sensor(move |sensor| {
        log::debug!("remove_sensor callback called: {sensor:?}");
        let mut current_sensors: Vec<Sensor> = weak_ui.unwrap().get_sensors().iter().collect();
        let removed = current_sensors
                        .extract_if(.., |x| x.sensor_id == sensor.sensor_id)
                        .collect::<Vec<_>>();
        log::info!("Removed from UI: {removed:?}");
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
        if val {
            tx.send(UiCmd::Subscribe(id)).unwrap();
            log::debug!("subscribing to sensor");
        } else {
            tx.send(UiCmd::Unsubscribe(id)).unwrap();
            log::debug!("unsubscribing from sensor");
        }
    });

    // ui.on_sensor_db
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_db(move |id, val| {
        log::debug!("changing database status for sensor");
        if !val {
            // only delete; sensors are added to db automatically when found
            tx.send(UiCmd::DbDeleteSensor(id)).unwrap();
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

    // ui.on_sensor_read_db
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_read_db(move |id| {
        log::debug!("reading from sensor");
        tx.send(UiCmd::DbGetData(id)).unwrap();
    });

    // ui.on_sensor_name
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_sensor_name(move |id, name| {
        log::debug!("sensor name changed sensor");
        tx.send(UiCmd::UpdateName(id, name.to_string())).unwrap();
    });

    // ui.on_find_sensors
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_find_sensors(move || {
        log::debug!("ui find sensors");
        tx.send(UiCmd::FindSensors).unwrap();
    });

    // ui.on_connect_all
    // called from frontend
    let tx = ui_tx.clone();
    ui.on_connect_all(move || {
        log::debug!("ui find sensors");
        tx.send(UiCmd::ConnectAll).unwrap();
    });

    //
    // 3. DEFINE FN & CALLBACKS FOR UI CLEAN EXIT
    //

    // repeating timer to handle the other thread handles
    let weak_ui = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(500),
        move || {
            let periph = mgr_handle.is_finished();
            let db = db_handle.is_finished();
            let ui = ui_handle.is_finished();

            let slint_ui = weak_ui.unwrap();

            if periph && db && ui {
                slint_ui.invoke_stop_condition();
            }
            // i'm thinking about handling other cases here as well: warning to user when any thread has stopped,
            // e.g. database manager has run into an issue, so there won't be db access.
            // but is this sound design? how do i communicate this from here? easiest would be with additional cllbacks of course,
            // but i'm not well versed enought in gui-development to know if this is how these things are done..

            else {
                if periph {
                    log::warn!("perpheral manager thread has stopped");
                    slint_ui.invoke_sensor_mgr_stopped();
                }
                if db {
                    log::warn!("database manager thread has stopped");
                    slint_ui.invoke_db_mgr_stopped();
                }
                if ui {
                    log::warn!("ui manager thread has stopped");
                }
            }
        },
    );

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

    // ui.on_close
    // called from frontend
    let tx = ui_tx.clone();
    let weak_ui = ui.as_weak();
    ui.on_close_button(move || {
        // this only tells ui-mgr thread to stop
        // which in turn sends stop signal to other mgrs
        // the repeated timer running alongside all of this triggers
        // an event/callback if all manager threads have stopped.
        // in the callback, we check if closing is in progress
        // and if yes: exit
        log::debug!("ui stopped, sending stop to managers");
        tx.send(UiCmd::StopThread).unwrap();

        weak_ui.unwrap().set_exit_initiated(true);
        let _ = weak_ui.unwrap().hide();

        // backup timer to hard stop if threads are not finished after 10 secs
        slint::Timer::single_shot(std::time::Duration::from_secs(10), move || {
            log::warn!("threads did not stop after 10 sec");
            slint::quit_event_loop().unwrap();
        });
    });

    let weak_ui = ui.as_weak();
    ui.window().on_close_requested(move || {
        // set the callback in case the main window is closed not via the close-button
        log::debug!("main ui window closed");
        weak_ui.unwrap().invoke_close_button();
        CloseRequestResponse::HideWindow
    });

    //
    // 4. RUN
    //

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

/////////////////////////////////////////////////////////////////
/////////////////////// MeasureApp //////////////////////////////
/////////////////////////////////////////////////////////////////

/// UniqueCounter
///
/// Helper to supply unique ids for newly-detected sensors
struct UniqueCounter {
}

impl UniqueCounter {
    /// provide next unique id
    pub fn next() -> u16 {
        static CNT: AtomicU16 = AtomicU16::new(1);

        CNT.fetch_add(1, Ordering::Relaxed)
    }
}

/// App state
///
/// contains a vector of known sensors,
/// as well as a map from the ui-based sensor_id to the peripheral address
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
        match self.database.tx.send(cmd) {
            Ok(_) => (),
            Err(e) => {
                log::warn!("failed to send db cmd: {e}");
            }
        }
    }

    /// Send Command to the Peripheral manager
    ///
    /// This also adds the command to the pending peripheral actions
    fn send_peripheral_command(&mut self, cmd: HubCmd) {
        let cmd = self.peripheral.pending.add(PeripheralCmd::new(cmd));
        match self.peripheral.tx.send(cmd) {
            Ok(_) => (),
            Err(e) => {
                log::warn!("failed to send peripheral cmd: {e}");
            }
        }
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
        self.update_sensor_state(addr, SensorState::Connecting);
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

    /// Update a sensor data value
    ///
    /// set the value of the sensor to new value,
    /// if the sensor is in the DB, store value in DB
    /// and lastly update sensor in UI
    fn update_sensor_value(&mut self, addr: BDAddr, data: i32) {
        log::debug!("update sensor value");
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    s.last = data;
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

    /// Update a sensor state
    fn update_sensor_state(&mut self, addr: BDAddr, state: SensorState) {
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    s.state = state;
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

    /// Update a sensor connection value
    ///
    /// set the value of the sensor to new value,
    /// if the sensor is in the DB, store value in DB
    /// and lastly update sensor in UI
    /// @todo update, don't really need 'connected' anymore due to state
    fn update_sensor_subscribed(
        &mut self,
        addr: BDAddr,
        subscribed: bool,
    ) {
        if let Some(id) = self.state.sensor_map.get(&addr) {
            let sensor = match self
                .state
                .ui_sensors
                .iter_mut()
                .find(|s| s.sensor_id == *id)
            {
                Some(s) => {
                    s.subscribed = subscribed;
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

    /// get btlpeplug's BDAddr from u64
    ///
    /// works, but i need to find a use/place for it
    fn addr_from_id(id: u64) -> BDAddr {
        let bytes: [u8;6] = {
            let id_bytes = id.to_be_bytes();

            [id_bytes[2],
            id_bytes[3],
            id_bytes[4],
            id_bytes[5],
            id_bytes[6],
            id_bytes[7]]
        };

        BDAddr::from(bytes)
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
                                    let sensor_id: i32 = UniqueCounter::next() as i32;

                                    // debug code, todo: remove
                                    let derived_id = u64::from(addr);
                                    let derived_addr = MeasureApp::addr_from_id(derived_id);
                                    log::debug!("### original addr: {addr:?}; id: {derived_id}, derived addr: {derived_addr:?})");

                                    // add sensor in state 'connecting' b/c we start connecting automatically
                                    let sens = Sensor {
                                        state: SensorState::Connecting,
                                        sensor_id,
                                        name: sensorname.clone().into(),
                                        last: 0,
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
                                    self.update_sensor_state(addr, SensorState::Connected);
                                }
                                HubEvent::DeviceDisconnected(addr) => {
                                    log::info!("Async Device Disconnected: {addr:?}");
                                    self.update_sensor_subscribed(addr, false);
                                    self.update_sensor_state(addr, SensorState::Known);
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

                                    // todo: fix this, this is too much, but it looks like i might need it?
                                    match (cmd.msg(), val.clone()) {
                                        (HubCmd::Connect(addr),HubResp::Success) => {
                                            self.update_sensor_state(addr, SensorState::Connected);
                                        }
                                        (HubCmd::Connect(addr),HubResp::Failed) => {
                                            self.update_sensor_state(addr, SensorState::Known);
                                        }
                                        (HubCmd::Disconnect(addr),HubResp::Success) => {
                                            self.update_sensor_state(addr, SensorState::Known);
                                        }
                                        (HubCmd::Disconnect(addr),HubResp::Failed) => {
                                            self.update_sensor_state(addr, SensorState::Connected);
                                        }
                                        (_,_) => ()
                                    }
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
                                    log::debug!("sensor deleted from database");

                                    let removed = self.state.ui_sensors
                                        .extract_if(.., |x| x.db_id == id)
                                        .collect::<Vec<_>>();

                                    let sensor = match removed.len() {
                                        0 => {
                                            log::debug!("deleted sensor not found in internal sensors list");
                                            None
                                        }
                                        1 => removed.first().cloned(),
                                        _ => {
                                            log::debug!("more than one sensor found disconnected in internal sensors list");
                                            None
                                        }
                                    };

                                    if let Some(s) = sensor {
                                        if let Some(addr) = self.addr(s.sensor_id) {
                                            self.send_peripheral_command(HubCmd::ForgetSensor(addr));
                                        }
                                        // remove sensor altogether
                                        let ui = self.ui.clone();
                                        let _ = slint::invoke_from_event_loop(move || ui.unwrap().invoke_remove_sensor(s));
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

                // handle UI commands
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
                                    self.update_sensor_state(addr, SensorState::Connecting);
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
                                    self.update_sensor_subscribed(addr, true);
                                    log::debug!("ui mgr received Subscribe({addr:?}) command");
                                }
                            }
                            UiCmd::Unsubscribe(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::Unsubscribe(addr));
                                    self.update_sensor_subscribed(addr, false);
                                    log::debug!("ui mgr received Unsubscribe({addr:?}) command");
                                }
                            }
                            UiCmd::ReadFrom(id)=> {
                                if let Some(addr) = self.addr(id) {
                                    self.send_peripheral_command(HubCmd::ReadFrom(addr));
                                    log::debug!("ui mgr received Subscribe({addr:?}) command");
                                }
                            }
                            UiCmd::DbDeleteSensor(id) => {
                                let sensor = self.state.ui_sensors.iter().find(|x| x.sensor_id == id);
                                if let Some(s) = sensor {
                                    self.send_database_command(DBCmd::DeleteSensor(s.db_id));
                                }
                            }
                            UiCmd::DbGetData(id) => {
                                let sensor = self.state.ui_sensors.iter().find(|x| x.sensor_id == id);
                                if let Some(s) = sensor {
                                    self.get_data(s.db_id);
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
                                let sensor =  match self.state.ui_sensors.iter_mut().find(|s| s.sensor_id == id) {
                                    Some(s) => {
                                        s.name = name.into();
                                        Some(s.clone())
                                    }
                                    None => None
                                };
                                if let Some(s) = sensor {
                                    // update database entry
                                    self.send_database_command(DBCmd::UpdateSensor(s.db_id, s.name.to_string()));

                                    // update sensor in ui
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

    fn handle_peripheral_response(&mut self, cmd: HubCmd, resp: HubResp) -> bool {
        match (cmd,resp) {
            (HubCmd::Ping, _) => (),

            (HubCmd::FindSensors, HubResp::Failed) => {
                log::debug!("FindSensors failed");
            }
            (HubCmd::FindSensors, HubResp::Success) => {
                log::debug!("FindSensors succeeded");
            }

            (HubCmd::Connect(a), HubResp::Failed) => {
                log::debug!("Connect to sensor ({a:?} failed");
            }
            (HubCmd::Connect(a), HubResp::Success) => {
                log::debug!("Connect to sensor ({a:?} succeeded");
            }

            (HubCmd::ConnectAll, HubResp::Failed) => {
                log::debug!("Connecting to all sensors failed");
            }
            (HubCmd::ConnectAll, HubResp::Success) => {
                log::debug!("Connecting to all sensors succeeded");
            }

            (HubCmd::Disconnect(a), HubResp::Failed) => {
                log::debug!("Disconnect from sensor ({a:?} failed");
            }
            (HubCmd::Disconnect(a), HubResp::Success) => {
                log::debug!("Disconnect from sensor ({a:?} succeeded");
            }

            (HubCmd::Subscribe(a), HubResp::Failed) => {
                log::debug!("Subscribe to sensor ({a:?} failed");
            }
            (HubCmd::Subscribe(a), HubResp::Success) => {
                log::debug!("Subscribe to sensor ({a:?} succeeded");
            }

            (HubCmd::Unsubscribe(a), HubResp::Failed) => {
                log::debug!("Unsubscibe from sensor ({a:?} failed");
            }
            (HubCmd::Unsubscribe(a), HubResp::Success) => {
                log::debug!("Unsubscribe from sensor ({a:?} succeeded");
            }

            (HubCmd::ReadFrom(a), HubResp::Failed) => {
                log::debug!("Read from sensor ({a:?} failed");
            }
            (HubCmd::ReadFrom(a), HubResp::ReadData(_,_)) => {
                log::debug!("Read from sensor ({a:?} succeeded");
            }

            (HubCmd::Blink(a), HubResp::Failed) => {
                log::debug!("Blink sensor ({a:?} failed");
            }
            (HubCmd::Blink(a), HubResp::Success) => {
                log::debug!("Blink sensor ({a:?} succeeded");
            }

            (HubCmd::BlinkAll, HubResp::Failed) => {
                log::debug!("Blinking all sensors failed");
            }
            (HubCmd::BlinkAll, HubResp::Success) => {
                log::debug!("Blinking to all sensors succeeded");
            }

            (HubCmd::ForgetSensor(a), HubResp::Failed) => {
                log::debug!("Forgetting sensor ({a:?} failed");
            }
            (HubCmd::ForgetSensor(a), HubResp::Success) => {
                log::debug!("Forgetting sensor ({a:?} succeeded");
            }

            (HubCmd::StopThread, _) => (),

            (c,r) => {
                log::debug!("Invalid response received for command: {c:?} => {r:?}");
            }

        };

        true
    }

    /// Find sensor peripheral address from ui id
    fn addr(&self, id: i32) -> Option<BDAddr> {
        self.state
            .sensor_map
            .iter()
            .find_map(|(key, &val)| if val == id { Some(key) } else { None })
            .copied()
    }

    /// Find sensor id from peripheral address
    fn id(&self, addr: BDAddr) -> Option<i32> {
        self.state.sensor_map.get(&addr).copied()
    }
}
