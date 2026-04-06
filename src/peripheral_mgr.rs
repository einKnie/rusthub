//! PeripheralMgr
//!
//! Facilitates handling of peripherals

pub mod error;
pub mod message;
pub mod sensor;

/// PeripheralAction
///
/// Defined peripheral interactions
#[derive(Debug, Clone)]
pub enum PeripheralAction {
    /// Write one byte to peripheral
    Write([u8; 1]),
    /// Read data from peripheral
    Read,
    /// subscribe to notifications from peripheral
    Subscribe,
    /// unsubscribe from notifications from peripheral
    Unsubscribe,
}

/// ActionResult
///
/// Result for a PeripheralAction
#[derive(Debug, Clone)]
pub enum ActionResult {
    /// Generic success
    Success,
    /// Read data
    Data(u32),
}

/// A managed Sensor abstraction for the PeripheralMgr
///
/// The top level trait a type must implement
/// to be managed by PeripheralMgr
pub trait ManagedSensor: sensor::Sensor + Clone + std::fmt::Display {
    /// Perform a PeripheralAction
    fn do_action(
        &mut self,
        action: PeripheralAction,
    ) -> impl std::future::Future<Output = Result<ActionResult, error::PeripheralError>> + Send;
}

use self::{
    bt_peripheral::BtPeripheralMgr, error::PeripheralError, message::*,
    sensor::platform::moisture::MoistureSensor,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Peripheral Manager
///
/// Manager for Peripheral devices,
/// currently only implemented for bluetrooth peripheral
pub trait PeripheralMgr {
    /// Init the manager, allow for early exits
    fn init(&mut self) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;
    /// Run the manager
    fn run(&mut self) -> impl std::future::Future<Output = u32> + Send;
    /// Handle Command
    fn handle_cmd(&mut self, cmd: PeripheralCmd) -> impl futures::Future<Output = ()>;
}

/// Run the Peripheral Manager
///
/// Init and run the Mgr; this should be started as a separate thread
/// This should either be implemented per sensor
/// or updated to run several PeripheralMgrs for different sensors - if that is the desired behavior
pub async fn mgr_run(
    tx: UnboundedSender<PeripheralMsg>,
    rx: UnboundedReceiver<PeripheralCmd>,
) -> u32 {
    // init a PeripheralMgr for the bluteooth moisture sensor
    let mut mgr = BtPeripheralMgr::<MoistureSensor>::new(tx, rx);
    if mgr.init().await.is_err() {
        panic!("Initialisation failed!");
    }

    log::info!("Peripheral Manager initialized!");
    match mgr.run().await {
        0 => 0,
        _ => panic!("Peripheral Manager failed"),
    }
}

/// Bluetooth Peripheral Manager
///
/// Implements the Peripheral Mgr fpr bluetooth sensors
/// and provides access to BT peripherals
pub mod bt_peripheral {
    use super::error::PeripheralError;
    use super::message::{HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg};
    use super::{ActionResult, ManagedSensor, PeripheralAction, PeripheralMgr};
    use futures::future::join_all;
    use futures::stream::StreamExt;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
    use tokio::time;

    // bluetooth specifics
    use super::sensor::ble::HasBtPeripheral;
    use btleplug::api::{
        BDAddr, Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter,
    };
    use btleplug::platform::{Adapter, Manager};

    /// A managed Bluetooth Sensor abstraction for the PeripheralMgr
    ///
    /// One of the top level trait a type must implement
    /// to be managed by PeripheralMgr
    pub trait ManagedBtSensor: ManagedSensor + HasBtPeripheral {}

    impl<T: ManagedBtSensor> std::fmt::Display for BtPeripheralMgr<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "BLE Peripheral Manager for {} known sensors",
                self.sensors.len()
            )
        }
    }

    /// Bluetooth Peripheral Manager
    ///
    /// Manager for Peripheral devices,
    /// supports arbitrary blutooth sensor peripherals
    #[derive(Debug)]
    pub struct BtPeripheralMgr<T> {
        central: Option<Adapter>,
        sensors: Vec<T>,
        subscriptions: HashMap<BDAddr, UnboundedSender<PeripheralCmd>>,
        tx: UnboundedSender<PeripheralMsg>,
        rx: UnboundedReceiver<PeripheralCmd>,
    }

    // Implement PeripheralMgr for bluetooth peripheral manager
    impl<T: ManagedBtSensor> PeripheralMgr for BtPeripheralMgr<T> {
        /// Initialize
        async fn init(&mut self) -> Result<(), PeripheralError> {
            if self.central.is_none() {
                let manager = match Manager::new().await {
                    Ok(mgr) => mgr,
                    Err(_) => return Err(PeripheralError::NoAdapter),
                };
                let adapters = match manager.adapters().await {
                    Ok(adp) => adp,
                    Err(_) => return Err(PeripheralError::NoAdapter),
                };
                if adapters.is_empty() {
                    log::error!("No bluetooth adapter found");
                    return Err(PeripheralError::NoAdapter);
                }

                self.central = Some(adapters.into_iter().nth(0).unwrap());
                log::debug!("Initialized!");
            }
            Ok(())
        }

        /// Run Peripheral Manager
        ///
        /// Run the PeripheralMgr loop.
        ///
        async fn run(&mut self) -> u32
        where
            T: HasBtPeripheral,
        {
            match self.central.as_ref().unwrap().adapter_state().await {
                Ok(CentralState::PoweredOn) => (),
                Ok(_) => {
                    log::error!("Bluetooth adapter not powered on");
                    return 1;
                }
                Err(_) => {
                    log::error!("Could not get Bluetooth adapter state");
                    return 1;
                }
            };

            // get the adapter event stream
            let mut events = match self.central.as_ref().unwrap().events().await {
                Ok(ev) => ev,
                Err(_) => {
                    log::error!("Could not get Bluetooth event stream");
                    return 1;
                }
            };

            // start scanning for devices
            self.central
                .as_ref()
                .unwrap()
                .start_scan(ScanFilter::default())
                .await
                .unwrap();
            log::debug!("event handler intialized!");

            loop {
                tokio::select! {
                    Some(msg) = events.next() => {
                        match msg {
                            CentralEvent::DeviceDiscovered(id) => {
                                let new_peripheral = match self.central.as_ref().unwrap().peripheral(&id).await {
                                    Ok(p) => p,
                                    Err(_) => {
                                        continue;
                                    }
                                };
                                let properties = new_peripheral.properties().await.unwrap().unwrap();
                                let addr = properties.address;
                                let name = properties.local_name
                                    .map(|local_name| local_name.to_string())
                                    .unwrap_or_default();
                                let services = properties.services;

                                // we only care about our sensor here
                                if services.contains(&T::service_id())
                                {
                                    log::info!("DeviceDiscovered: {:?} {} {}", addr, name, id);

                                    // add new sensor to list and inform hub
                                    let new = T::new_with_peripheral(new_peripheral);
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceDiscovered(addr))).unwrap();
                                    self.sensors.push(new);

                                }
                            },
                            CentralEvent::StateUpdate(state) => {
                                log::info!("AdapterStatusUpdate {:?}", state);
                            },
                            CentralEvent::DeviceConnected(id) => {
                                let changed_peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = changed_peripheral.properties().await.unwrap();
                                let addr = properties.as_ref().unwrap().address;

                                if self.sensors.iter().any(|p| p.addr() == addr) {
                                    log::info!("DeviceConnected: {:?}", addr);
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceConnected(addr))).unwrap();
                                }
                            },
                            CentralEvent::DeviceDisconnected(id) => {
                                let changed_peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = changed_peripheral.properties().await.unwrap();
                                let addr = properties.as_ref().unwrap().address;

                                // check if the disconnected device is known to us
                                // peripheral name is not consistently available here, so let's compare address
                                if self.sensors.iter().any(|p| p.addr() == addr) {
                                    log::info!("DeviceDisconnected: {:?}", addr);
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceDisconnected(addr))).unwrap();
                                }
                            },
                            // let's ignore other events for now
                            _ => ()
                        }
                    },
                    // rx.recv()returns None when the channel is closed,
                    // so use that to stop the thread if the ui for some reason exits
                    result = self.rx.recv() => {
                        if let Some(msg) = result {
                            match msg.msg {
                                HubCmd::StopThread => {
                                    log::info!("received stop command from main");
                                    break;
                                },
                                ref _other => {
                                    self.handle_cmd(msg).await;
                                }
                            }
                        } else {
                            // channel was closed, better stop
                            log::info!("channel to main is closed");
                            break;
                        }
                    }
                }
            }

            // let's clean up after ourselves
            log::debug!("PeripheralMgr::run() cleaning up");

            // stop scanning for devices
            if self.central.as_ref().unwrap().stop_scan().await.is_err() {
                log::warn!("Failed to stop Scan");
            }

            // stop any notification threads
            log::debug!("* stopping any running notification threads");
            for tx in self.subscriptions.values() {
                tx.send(PeripheralCmd::new(HubCmd::StopThread)).unwrap();
            }

            // disconnect all peripherals
            log::debug!("* disconnecting all sensors");
            for mut p in self.sensors.clone() {
                if p.disconnect().await.is_err() {
                    log::warn!("Disconnect failed ({0})", p.addr());
                }
            }

            log::debug!("PeripheralMgr::run() done");
            0
        }

        /// Handle Command
        ///
        /// Handle commands from the Hub.
        /// Longer-running commands are handled in tasks and send their response async once finished.
        /// Immediate (and some quick) commands are handled directly
        async fn handle_cmd(&mut self, cmd: PeripheralCmd)
        where
            T: HasBtPeripheral,
        {
            let task_tx = self.tx.clone();

            match cmd.msg {
                HubCmd::Ping => {
                    // immmediate
                    // this is debug content
                    log::info!("received Ping from main");
                    log::debug!("Peripheral-mgr managing the following peripherals:");
                    for s in self.sensors.iter() {
                        log::debug!("{s}");
                    }
                    self.tx
                        .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                        .unwrap();
                }

                HubCmd::BlinkAll => {
                    log::info!("blinking all peripherals");
                    // spawn one task to spawn sensor-specific tasks and await them all before sending the Success resp
                    let mut sensors = self.sensors.clone();
                    tokio::spawn(async move {
                        let tasks: Vec<_> = sensors
                            .iter_mut()
                            .map(|sensor| {
                                let mut s = sensor.clone();
                                tokio::spawn(async move {
                                    for i in 0..21 {
                                        let led_cmd: [u8; 1] = match i % 2 {
                                            0 => [0],
                                            _ => [1],
                                        };
                                        log::debug!("writing: {:?} to {:?}", led_cmd, s.addr());
                                        let _ = s.do_action(PeripheralAction::Write(led_cmd)).await;
                                        time::sleep(Duration::from_millis(200)).await;
                                    }
                                })
                            })
                            .collect();
                        join_all(tasks).await;
                        task_tx
                            .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                            .unwrap();
                    });
                }

                HubCmd::Blink(addr) => {
                    log::info!("blinking led on peripheral ({addr:?})");
                    let mut p = match self.sensors.iter().find(|&p| p.addr() == addr) {
                        Some(p) => p.clone(),
                        None => {
                            task_tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };

                    tokio::spawn(async move {
                        for i in 0..21 {
                            let led_cmd: [u8; 1] = match i % 2 {
                                0 => [0],
                                _ => [1],
                            };
                            log::debug!("writing: {:?} to peripheral ({:?})", led_cmd, p.addr());
                            let _ = p.do_action(PeripheralAction::Write(led_cmd)).await;
                            time::sleep(Duration::from_millis(200)).await;
                        }
                        task_tx
                            .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                            .unwrap();
                    });
                }

                HubCmd::ReadFrom(addr) => {
                    log::info!("Reading from peripheral ({addr:?})");
                    let mut p = match self.sensors.iter().find(|&p| p.addr() == addr) {
                        Some(p) => p.clone(),
                        None => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };

                    tokio::spawn(async move {
                        match p.do_action(PeripheralAction::Read).await {
                            Ok(ActionResult::Data(res)) => {
                                log::debug!("read peripheral data: {res:?}");
                                task_tx
                                    .send(PeripheralMsg::Response(
                                        cmd.id,
                                        HubResp::ReadData(addr, res),
                                    ))
                                    .unwrap();
                            }
                            Ok(res) => {
                                log::debug!("unexpected ActionResult received: {res:?}");
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                    .unwrap();
                            }
                            Err(e) => {
                                log::warn!("Failed to read peripheral data: {e}");
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                    .unwrap();
                            }
                        };
                    });
                }

                HubCmd::Subscribe(addr) => {
                    // no change needed
                    match self.subscribe(addr).await {
                        Err(_) => {
                            log::warn!("Failed to subscribe to peripheral data!");
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                .unwrap();
                        }
                    }
                }

                HubCmd::Unsubscribe(addr) => {
                    // no change needed
                    match self.unsubscribe(addr).await {
                        Err(_) => {
                            log::warn!("Failed to unsubscribe from peripheral data!");
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                .unwrap();
                        }
                    }
                }

                HubCmd::FindSensors => {
                    log::info!("looking for peripherals");
                    // todo run in thread somehow
                    match self.find_sensors().await {
                        Err(_) => {
                            log::warn!("Finding new peripherals failed");
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                .unwrap();
                        }
                    }
                }

                HubCmd::Connect(addr) => {
                    log::info!("connecting to peripheral ({addr:?})");

                    let mut p = match self.sensors.iter().find(|&p| p.addr() == addr) {
                        Some(p) => p.clone(),
                        None => {
                            log::warn!("peripheral ({addr:?}) not found");
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };

                    tokio::spawn(async move {
                        match p.connect().await {
                            Err(_) => {
                                log::warn!("Failed to connect to peripheral!");
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                    .unwrap();
                            }
                            Ok(_) => {
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                    .unwrap();
                            }
                        }
                    });
                }

                HubCmd::ConnectAll => {
                    log::debug!("connecting to all known peripherals");
                    let sensors = self.sensors.clone();
                    let mut err = 0;

                    tokio::spawn(async move {
                        for mut s in sensors {
                            if s.connect().await.is_err() {
                                log::debug!("failed to connect to peripheral ({:?}", s.addr());
                                err += 1;
                            }
                        }

                        if err > 0 {
                            task_tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                        } else {
                            task_tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                .unwrap();
                        }
                    });
                }

                HubCmd::Disconnect(addr) => {
                    log::info!("disonnecting from peripheral ({addr:?})");

                    let mut p = match self.sensors.iter().find(|&p| p.addr() == addr) {
                        Some(p) => p.clone(),
                        None => {
                            log::warn!("peripheral ({addr:?}) not found");
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };

                    tokio::spawn(async move {
                        match p.disconnect().await {
                            Err(_) => {
                                log::warn!("Failed to disconnect from peripheral!");
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                    .unwrap();
                            }
                            Ok(_) => {
                                task_tx
                                    .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                                    .unwrap();
                            }
                        }
                    });
                }
                HubCmd::StopThread => (), // handled elsewhere
            }
        }
    }

    // Implement BtPeripheralMgr for an arbitrary sensor
    impl<T: ManagedBtSensor> BtPeripheralMgr<T> {
        /// Generate a new BtPeripheralMgr
        pub fn new(
            tx: UnboundedSender<PeripheralMsg>,
            rx: UnboundedReceiver<PeripheralCmd>,
        ) -> Self {
            Self {
                central: None,
                sensors: Vec::<T>::new(),
                subscriptions: HashMap::new(),
                tx,
                rx,
            }
        }

        /// Find all SensorPeripherals
        ///
        /// Check all bluetooth peripherals. If one matches with our spec, add to vector
        /// (at this point this is basically a backup, but the sensor usually advertises itself anyway through the bt event channel)
        /// platform-specific
        async fn find_sensors(&mut self) -> Result<(), PeripheralError>
        where
            T: HasBtPeripheral,
        {
            let prevlen = self.sensors.len();

            for p in self.central.as_ref().unwrap().peripherals().await.unwrap() {
                // detect sensors via the sensor characteristic
                let services = match p.properties().await {
                    Ok(Some(s)) => s.services,
                    Ok(None) => Vec::new(),
                    Err(_) => Vec::new(),
                };
                if services.contains(&T::service_id()) {
                    let addr = p.properties().await.unwrap().unwrap().address;
                    let id = p.id();

                    if self.sensors.iter().any(|p| p.addr() == addr) {
                        // already known
                        continue;
                    }

                    log::debug!("found new sensor device! ({addr:?}) {id}");
                    self.sensors.push(T::new_with_peripheral(p));
                    self.tx
                        .send(PeripheralMsg::Event(HubEvent::DeviceDiscovered(addr)))
                        .unwrap();
                }
            }

            if self.sensors.len() == prevlen {
                log::debug!(
                    "no new peripheral found (old len: {prevlen:?} - newlen: {:?})",
                    self.sensors.len()
                );
                Err(PeripheralError::NoPeripheral)
            } else {
                log::debug!(
                    "found a new sensor! (old len: {prevlen:?} - newlen: {:?})",
                    self.sensors.len()
                );
                Ok(())
            }
        }

        /// Subscribe to data from a Peripheral with given address
        ///
        /// Subscribe to data updates from a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn subscribe(&mut self, addr: BDAddr) -> Result<(), PeripheralError>
        where
            T: HasBtPeripheral,
        {
            let mut sensors = self.sensors.clone();

            let p = match sensors.iter_mut().find(|p| p.addr() == addr) {
                Some(p) => p,
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("subscribing to data from sensor peripheral");
            match p.do_action(PeripheralAction::Subscribe).await {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("Failed to subscribe to sensor data: {e}");
                    return Err(e);
                }
            }

            // start notification thread
            let (mgr_tx, thread_rx) = unbounded_channel();
            let _handle = tokio::spawn(Self::handle_notifications(
                p.clone(),
                self.tx.clone(),
                thread_rx,
            ));

            // store Sender in hashmap so we can stop the thread on unsubscribe
            self.subscriptions.insert(addr, mgr_tx);

            self.sensors = sensors;
            Ok(())
        }

        /// Unsubscribe to data from a Peripheral with given address
        ///
        /// Unubscribe to data updates from a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn unsubscribe(&mut self, addr: BDAddr) -> Result<(), PeripheralError>
        where
            T: HasBtPeripheral,
        {
            let mut sensors = self.sensors.clone();

            let p = match sensors.iter_mut().find(|p| p.addr() == addr) {
                Some(p) => p,
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("unsubscribing to data from sensor peripheral");
            match p.do_action(PeripheralAction::Unsubscribe).await {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("Failed to unsubscribe to sensor data: {e}");
                    return Err(e);
                }
            }

            // stop notification thread (and remove entry from hashmap in the process)
            match self.subscriptions.remove(&addr) {
                Some(tx) => {
                    tx.send(PeripheralCmd::new(HubCmd::StopThread)).unwrap();
                }
                None => {
                    log::debug!("subscription not found in current subscriptions");
                }
            };

            self.sensors = sensors;
            Ok(())
        }

        /// Sensor Notification Handling
        ///
        /// Continuously check for notifications on the given stream
        /// and transmit received data via the given sender, until receiving a ThreadStop command on the receiver.
        ///
        /// @todo Is there a way to combine the stream and receiver for a blocking wait on both?
        /// that would improve performance (not that there's any issues so far)
        /// platform-specific
        pub async fn handle_notifications(
            p: T,
            tx: UnboundedSender<PeripheralMsg>,
            mut rx: UnboundedReceiver<PeripheralCmd>,
        ) -> u32
        where
            T: HasBtPeripheral,
        {
            log::debug!("Hello from notification thread for [{0}]", p.addr());

            let mut stream = match p.peripheral().notifications().await {
                Ok(s) => s,
                Err(e) => {
                    panic!("Could not get notification stream {e}");
                }
            };

            loop {
                tokio::select! {
                    Some(msg) = stream.next() => {
                        log::debug!("Received data notification from sensor!");
                        let d = match <[u8;4]>::try_from(&msg.value[..4]) {
                            Ok(arr) => u32::from_le_bytes(arr),
                            Err(_) => {
                                log::debug!("invalid value notification received: {msg:?}");
                                // ignore for now, so far this case has never happened and
                                // if it occurs i want to know if this even is a stop condition
                                // or e.g. the next value would be ok again
                                continue;
                            }
                        };
                        tx.send(PeripheralMsg::Event(HubEvent::NewData(p.addr(), d))).unwrap();
                    },
                    Some(PeripheralCmd{id: _ , msg: HubCmd::StopThread}) = rx.recv() => {
                        log::info!("received stop command from main");
                        break;
                    }
                }
            }

            log::debug!("Leaving notification thread for [{0}]", p.addr());
            0
        }
    }
}
