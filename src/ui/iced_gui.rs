//! experimental iced gui
//! iced, see: https://docs.rs/iced/latest/iced/index.html

use crate::cmdmgr::CmdMgr;
use crate::database_mgr::{
    data::charting,
    database,
    message::{DBCmd, DBResp, DatabaseCmd, DatabaseQuery, DatabaseResp},
};
use crate::peripheral_mgr;
use crate::peripheral_mgr::message::{HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg};
use crate::ui::{ConnectedSensor, SensorName};
use btleplug::api::BDAddr;
use chrono::Local;
// use iced::task::{Never, Sipper, sipper};
use iced::widget::{button, column, container, rule, scrollable, text, toggler};
use iced::window;
use iced::{Element, Subscription, Task};
// use std::sync::Arc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
// use tokio::task::JoinHandle;

#[allow(unused)]
#[derive(Debug, Clone)]
enum Evt {
    Run,
    Msg,
}

#[allow(unused)]
#[derive(Debug, Clone)]
enum Msg {
    FindSensors,
    ConnectAll,
    Event(Evt),
    SensorToggled(BDAddr, bool),
    ReadSensor(BDAddr),
    BlinkSensor(BDAddr),
    SubscribeSensor(BDAddr),
    UnsubscribeSensor(BDAddr),
    GetData(i32),
    StoreSensor(BDAddr),
    DeleteSensor(BDAddr),
    Exit,
    CleanExit(u32),
    //
    Discovered(BDAddr),
    Connected(BDAddr),
    Disconnected(BDAddr),
    Data(BDAddr, u32),
    PeripheralResponse(u16, HubResp),
}

/// App state
#[derive(Clone, Debug, Default)]
struct MeasureAppState {
    sensors: Vec<ConnectedSensor>,
}

/// Mgr connection
/// Generic to hold sender and receiver for a mgr connection
#[derive(Debug)]
struct MgrConnection<T, U> {
    tx: UnboundedSender<T>,
    rx: Option<UnboundedReceiver<U>>,
    pending: CmdMgr<T>,
    handle: tokio::task::JoinHandle<u32>,
}

/// Measurement GUI
struct MeasureApp {
    peripheral: MgrConnection<PeripheralCmd, PeripheralMsg>,
    database: MgrConnection<DatabaseCmd, DatabaseResp>,

    state: MeasureAppState,
}

impl Drop for MeasureApp {
    fn drop(&mut self) {
        log::debug!("dropping MeasureApp");
    }
}

impl MeasureApp {
    /// Initialize
    ///
    /// this is iced api
    /// This functions as a new() with encapsulated managers
    fn boot() -> Self {
        // spawn peripheral manager
        let (periph_tx, thread_periph_rx) = unbounded_channel();
        let (thread_periph_tx, periph_rx) = unbounded_channel();
        let mgr_handle = tokio::spawn(peripheral_mgr::mgr_run(thread_periph_tx, thread_periph_rx));

        // spawn database manager
        let (db_tx, thread_db_rx) = unbounded_channel();
        let (thread_db_tx, db_rx) = unbounded_channel();
        let db_handle = tokio::spawn(database::mgr_run(thread_db_tx, thread_db_rx));

        let pending_p = CmdMgr::default();
        pending_p.start_handler();

        let pending_db = CmdMgr::default();
        pending_db.start_handler();

        log::debug!("boot finished");

        Self {
            peripheral: MgrConnection {
                tx: periph_tx,
                rx: Some(periph_rx),
                pending: pending_p,
                handle: mgr_handle,
            },
            database: MgrConnection {
                tx: db_tx,
                rx: Some(db_rx),
                pending: pending_db,
                handle: db_handle,
            },
            state: MeasureAppState {
                #[cfg(not(feature = "test_sensors"))]
                sensors: Vec::<ConnectedSensor>::new(),
                #[cfg(feature = "test_sensors")]
                sensors: Vec::<ConnectedSensor>::from(vec![
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 1]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 2]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 3]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 4]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 5]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                    ConnectedSensor {
                        addr: BDAddr::from([0, 1, 2, 3, 4, 6]),
                        name: SensorName::new("test".to_string()),
                        last: 0,
                        subscribed: false,
                        show: false,
                        id: 0,
                    },
                ]),
            },
        }
    }

    /// Update the state
    ///
    /// this is iced api
    /// Msgs are generated in view()
    fn update(&mut self, message: Msg) -> Task<Msg> {
        match message {
            Msg::Exit => {
                log::debug!("Got quit command");
                // tell PeripheralMgr to stop
                self.send_peripheral_command(HubCmd::StopThread);

                // tell database mrg to stop
                self.send_database_command(DBCmd::StopThread);

                // ideally, this should wait for threads to stop and then close the window
                // but its not working
                return window::latest().and_then(window::close);
            }
            Msg::CleanExit(res) => {
                if res != 0 {
                    log::warn!("Not a clean exit");
                }
                // ideally, this should wait for threads to stop and then close the window
                return window::latest().and_then(window::close);
            }
            Msg::FindSensors => {
                log::debug!("looking for sensors");
                self.send_peripheral_command(HubCmd::FindSensors);
            }
            Msg::ConnectAll => {
                log::debug!("connecting to all known sensors");
                self.send_peripheral_command(HubCmd::ConnectAll);
            }
            Msg::SensorToggled(addr, state) => {
                if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                    p.show = state;
                }
            }
            Msg::ReadSensor(addr) => {
                log::debug!("reading from sensor");
                self.send_peripheral_command(HubCmd::ReadFrom(addr));
            }
            Msg::BlinkSensor(addr) => {
                log::debug!("reading from sensor");
                self.send_peripheral_command(HubCmd::Blink(addr));
            }
            Msg::SubscribeSensor(addr) => {
                if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                    p.subscribed = true;
                    self.send_peripheral_command(HubCmd::Subscribe(addr));
                }
            }
            Msg::UnsubscribeSensor(addr) => {
                if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr) {
                    p.subscribed = false;
                    self.send_peripheral_command(HubCmd::Unsubscribe(addr));
                }
            }
            Msg::GetData(id) => {
                self.send_database_command(DBCmd::Get(DatabaseQuery::TsBefore(id, Local::now())));
            }
            Msg::StoreSensor(addr) => {
                // todo store sensor in db
                self.send_database_command(DBCmd::AddSensor(addr.into(), String::from("Name")));
            }
            Msg::DeleteSensor(addr) => {
                if let Some(p) = self.state.sensors.iter().find(|p| p.addr == addr) {
                    self.send_database_command(DBCmd::DeleteSensor(p.id));
                }
            }
            Msg::Event(evt) => {
                // log::debug!("received event: {evt:?}");
                match evt {
                    Evt::Run => {
                        // log::debug!("running the msg loop");
                        self.run();
                    }
                    Evt::Msg => {
                        log::debug!("message received!");
                    }
                }
            }

            Msg::Discovered(_addr) => {
                log::debug!("device discovered!");
            }
            Msg::Connected(_addr) => {
                log::debug!("device connected!");
            }
            Msg::Disconnected(_addr) => {
                log::debug!("device discoonected");
            }
            Msg::PeripheralResponse(_id, _msg) => {
                log::debug!("received peripheral response!");
            }
            Msg::Data(_addr, _data) => {
                log::debug!("received peripheral data");
            }
        }
        Task::none()
    }

    /// Generate visible sensors area of UI
    fn generate_sensor_view(&self) -> Vec<Element<'_, Msg>> {
        let mut shown_sensors: Vec<Element<Msg>> = vec![];

        for sensor in self.state.sensors.iter() {
            if sensor.show {
                let mut buttons: Vec<Element<Msg>> = vec![
                    button("Read")
                        .on_press(Msg::ReadSensor(sensor.addr()))
                        .into(),
                    button("Blink")
                        .on_press(Msg::BlinkSensor(sensor.addr()))
                        .into(),
                ];

                if sensor.subscribed {
                    buttons.push(
                        button("Unsubscribe")
                            .on_press(Msg::UnsubscribeSensor(sensor.addr()))
                            .into(),
                    );
                } else {
                    buttons.push(
                        button("Subscribe")
                            .on_press(Msg::SubscribeSensor(sensor.addr()))
                            .into(),
                    );
                }

                if sensor.in_db() {
                    buttons.push(
                        button("Fetch data")
                            .on_press(Msg::GetData(sensor.id()))
                            .into(),
                    );
                    buttons.push(
                        button("DELETE from DB")
                            .on_press(Msg::DeleteSensor(sensor.addr()))
                            .into(),
                    );
                } else {
                    buttons.push(
                        button("Add to DB")
                            .on_press(Msg::StoreSensor(sensor.addr()))
                            .into(),
                    );
                }

                let sensor_view = iced::widget::Column::from_vec(buttons).spacing(5);

                shown_sensors.push(
                    container(column![
                        text(format!("{} ({})", sensor.name(), sensor.value())),
                        sensor_view
                    ])
                    .style(container::bordered_box)
                    .padding(5)
                    .into(),
                )
            }
        }

        shown_sensors
    }

    /// Show the UI
    ///
    /// this is iced api
    /// construct and show the ui
    fn view(&self) -> Element<'_, Msg> {
        let welcome_msg = format!("Managing {} sensors", self.state.sensors.len());
        let mut s: Vec<Element<Msg>> = vec![];
        for sensor in self.state.sensors.iter() {
            s.push(
                toggler(sensor.show)
                    .label(sensor.name())
                    .on_toggle(|state| Msg::SensorToggled(sensor.addr(), state))
                    .into(),
            )
        }

        let sensor_grid = iced::widget::Row::from_vec(s).spacing(5).padding(5).wrap();
        let sensor_ctrl_grid = iced::widget::Grid::from_vec(self.generate_sensor_view()).spacing(5);
        container(
            column![
                text(welcome_msg),
                button("Find sensors").on_press(Msg::FindSensors),
                button("Connect").on_press(Msg::ConnectAll),
                rule::horizontal(2),
                sensor_grid,
                // put the exit button into the scollable, so it is still visible when many sensors are active
                // todo: fix this, the stuff below a scollable should remain visible, i.e. scrollable must take their size into account
                scrollable(column![
                    sensor_ctrl_grid,
                    rule::horizontal(2),
                    button("Exit").on_press(Msg::Exit)
                ]), // this goes out of view when scrollable gets too large -.-
            ]
            .spacing(5),
        )
        .padding(5)
        .into()
    }

    /// Define a subscription
    ///
    /// this is iced api
    /// not sure how to use this yet
    fn subscription(&self) -> Subscription<Msg> {
        // todo: find a better way to do this, would love to just run the run() here insteads of just triggering it

        iced::time::every(iced::time::Duration::from_millis(10))
            .map(|_instant| Msg::Event(Evt::Run))
    }

    // fn run_mgrs(
    //     &mut self,
    //     _p_rx: UnboundedReceiver<PeripheralMsg>,
    //     _d_rx: UnboundedReceiver<DatabaseResp>,
    // ) -> impl Sipper<Never, Msg> {
    //     let mut p = self.peripheral.rx.take().unwrap();
    //     let mut d = self.database.rx.take().unwrap();
    //     sipper(async move |mut output| {
    //         // ~~init threads and then listen for events forever~~
    //         // does not work, i need to able to send to threads from gui -.- duh!
    //         loop {
    //             tokio::select! {
    //                 Some(msg) = p.recv() => {
    //                     match msg {
    //                         PeripheralMsg::Event(val) => {
    //                             log::debug!("Received event message");
    //                             dbg!(&val);

    //                             match val {
    //                                 HubEvent::DeviceDiscovered(addr) => {
    //                                     log::info!("Device Discovered: {addr:?}");
    //                                     output.send(Msg::Discovered(addr)).await;
    //                                 }
    //                                 HubEvent::NewData(addr, data) => {
    //                                     log::info!("Async received new sensor data for {addr:?}");
    //                                     output.send(Msg::Data(addr, data)).await;
    //                                 }
    //                                 HubEvent::DeviceConnected(addr) => {
    //                                     log::info!("Async Device Connected: {addr:?}");
    //                                     output.send(Msg::Connected(addr)).await;
    //                                 }
    //                                 HubEvent::DeviceDisconnected(addr) => {
    //                                     log::info!("Async Device Disconnected: {addr:?}");
    //                                     output.send(Msg::Disconnected(addr)).await;
    //                                 }
    //                             };
    //                         }
    //                         PeripheralMsg::Response(id, val) => {
    //                             log::debug!("peripheral response received");
    //                             output.send(Msg::PeripheralResponse(id, val)).await;
    //                         }
    //                     }
    //                 }
    //                 Some(msg) = d.recv() => {
    //                     log::debug!("Database message received: {msg:?}");
    //                     output.send(Msg::Event(Evt::Msg)).await;

    //                 }
    //             }
    //         }
    //     })
    // }

    // implement run async as a Sipper,
    // ideally, with everything inside (incl. starting the threads, should make it easier)
    // and then run this as a subscription (see: https://github.com/iced-rs/iced/blob/master/examples/websocket/src/main.rs#L81)
    // fn subscription_run(&self) -> Subscription<Msg> {
    //     // let channel = iced::stream::channel();
    //     let p_rx = self.peripheral.rx.take().unwrap();
    //     let d_rx = self.database.rx.take().unwrap();
    //     // let d_rx = self.database.rx;
    //     Subscription::run(|| {
    //         sipper(async move |mut output| {
    //             // ~~init threads and then listen for events forever~~
    //             // does not work, i need to able to send to threads from gui -.- duh!
    //             loop {
    //                 tokio::select! {
    //                     Some(msg) = p_rx.recv() => {
    //                         match msg {
    //                             PeripheralMsg::Event(val) => {
    //                                 log::debug!("Received event message");
    //                                 dbg!(&val);

    //                                 match val {
    //                                     HubEvent::DeviceDiscovered(addr) => {
    //                                         log::info!("Device Discovered: {addr:?}");
    //                                         output.send(Msg::Discovered(addr)).await;
    //                                         //self.send_peripheral_command(HubCmd::Connect(addr));
    //                                     }
    //                                     HubEvent::NewData(addr, data) => {
    //                                         log::info!("Async received new sensor data for {addr:?}");
    //                                         output.send(Msg::Data(addr, data)).await;
    //                                         // if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr)
    //                                         // {
    //                                         //     p.last = data;
    //                                         //     let id = p.id();
    //                                         //     if p.in_db() {
    //                                         //         self.send_database_command(DBCmd::AddEntry(
    //                                         //             id,
    //                                         //             chrono::Local::now(),
    //                                         //             data,
    //                                         //         ));
    //                                         //     }
    //                                         // }
    //                                     }
    //                                     HubEvent::DeviceConnected(addr) => {
    //                                         log::info!("Async Device Connected: {addr:?}");
    //                                         output.send(Msg::Connected(addr)).await;

    //                                         // let sensorname = SensorName::new(format!(
    //                                         //     "Sensor {:?}",
    //                                         //     self.state.sensors.len() + 1
    //                                         // ));

    //                                         // self.state.sensors.push(ConnectedSensor {
    //                                         //     addr,
    //                                         //     name: sensorname.clone(),
    //                                         //     last: 0,
    //                                         //     subscribed: false,
    //                                         //     show: false,
    //                                         //     id: 0,
    //                                         // });
    //                                         // self.send_database_command(DBCmd::AddSensor(
    //                                         //     u64::from(addr),
    //                                         //     sensorname.value,
    //                                         // ));
    //                                     }
    //                                     HubEvent::DeviceDisconnected(addr) => {
    //                                         log::info!("Async Device Disconnected: {addr:?}");
    //                                         output.send(Msg::Disconnected(addr)).await;

    //                                         // let removed = self
    //                                         //     .state
    //                                         //     .sensors
    //                                         //     .extract_if(.., |x| x.addr() == addr)
    //                                         //     .collect::<Vec<_>>();
    //                                         // log::info!("Removed from UI: {removed:?}");
    //                                     }
    //                                 };
    //                             }
    //                             PeripheralMsg::Response(id, val) => {
    //                                 log::debug!("peripheral response received");
    //                                 output.send(Msg::PeripheralResponse(id, val)).await;
    //                             }
    //                         }
    //                     }
    //                     Some(msg) = d_rx.recv() => {
    //                         log::debug!("Database message received: {msg:?}");
    //                         output.send(Msg::Event(Evt::Msg)).await;

    //                     }
    //                 }
    //             }
    //         })
    //     })
    // }

    // non-blocking to be run inside frame update
    // @todo: is there another way i could do this? or is this fine?
    fn run(&mut self) -> i8 {
        if let Some(rx) = self.peripheral.rx.as_mut() {
            // match self.peripheral.rx.try_recv() {
            match rx.try_recv() {
                // handle Event message
                Ok(PeripheralMsg::Event(val)) => {
                    log::debug!("Received event message");
                    dbg!(&val);

                    match val {
                        HubEvent::DeviceDiscovered(addr) => {
                            log::info!("Device Discovered: {addr:?}");
                            self.send_peripheral_command(HubCmd::Connect(addr));
                        }
                        HubEvent::NewData(addr, data) => {
                            log::info!("Async received new sensor data for {addr:?}");
                            if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr)
                            {
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
                            let sensorname = SensorName::new(format!(
                                "Sensor {:?}",
                                self.state.sensors.len() + 1
                            ));

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
                            if let Some(p) = self.state.sensors.iter_mut().find(|p| p.addr == addr)
                            {
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
        }

        if let Some(rx) = self.database.rx.as_mut() {
            // match self.database.rx.try_recv() {
            match rx.try_recv() {
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
                        DBResp::Data(vec) => {
                            log::debug!("received data!");
                            // just for testing: draw received data
                            // todo: find out how i can directly display in gui popup
                            dbg!(&vec);
                            charting::draw_chart("", vec);
                        }
                    }
                }

                // handle (or ignore) errors
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    return -1;
                }
            }
        }
        0
    }

    /// Send Command to the database
    ///
    /// This also adds the command to the pending database actions
    fn send_database_command(&mut self, cmd: DBCmd) {
        let cmd = self.database.pending.add(DatabaseCmd::new(cmd));
        match self.database.tx.send(cmd) {
            Ok(_) => (),
            Err(e) => log::warn!("failed to send database cmd: {e}"),
        }
    }

    /// Send Command to the Peripheral manager
    ///
    /// This also adds the command to the pending peripheral actions
    fn send_peripheral_command(&mut self, cmd: HubCmd) {
        let cmd = self.peripheral.pending.add(PeripheralCmd::new(cmd));
        match self.peripheral.tx.send(cmd) {
            Ok(_) => (),
            Err(e) => log::warn!("failed to send peripheral cmd: {e}"),
        }
    }
}

/// Run the measurent GUI
///
/// This is basically the only thing we run from main at this point
pub fn run_gui() -> u32 {
    let settings = iced::window::Settings {
        resizable: true,
        transparent: true,
        blur: true,
        closeable: true, // does nothing for some reason?
        ..Default::default()
    };
    if iced::application(MeasureApp::boot, MeasureApp::update, MeasureApp::view)
        .window(settings)
        .theme(iced::Theme::KanagawaDragon)
        .subscription(MeasureApp::subscription)
        .run()
        .is_err()
    {
        log::error!("GUI ended with error");
    }
    0
}
