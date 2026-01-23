pub mod error;
pub mod message;
pub mod sensor;

pub mod peripheral {
    use crate::peripheral_mgr::error::PeripheralError;
    use crate::peripheral_mgr::message::{HubCmd, HubEvent, HubResp, PeripheralCmd, PeripheralMsg};
    use crate::peripheral_mgr::sensor::{ActionResult, PeripheralAction, SensorPeripheral};

    use btleplug::api::{
        BDAddr, Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter,
    };
    use btleplug::platform::{Adapter, Manager};
    use futures::future::join_all;
    use futures::stream::StreamExt;
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
    use tokio::time;
    use uuid::{Uuid, uuid};

    /// Specific Characteristic UUID set by Sensor Peripheral
    /// for controlling the sensor LED
    const LED_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e5");
    const SENSOR_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e6");

    /// Run the Peripheral Manager
    ///
    /// Init and run the Mgr; this should be run as a separate thread
    pub async fn mgr_run(
        tx: UnboundedSender<PeripheralMsg>,
        rx: UnboundedReceiver<PeripheralCmd>,
    ) -> u32 {
        // init the manager
        let mut mgr = PeripheralMgr::new(tx, rx);
        if mgr.init().await.is_err() {
            panic!("Initialisation failed!");
        }

        log::info!("Peripheral Manager initialized!");
        match mgr.run().await {
            0 => 0,
            _ => panic!("Peripheral Manager failed"),
        }
    }

    /// Peripheral Manager
    ///
    /// Manager for Peripheral devices
    ///
    #[derive(Debug)]
    pub struct PeripheralMgr {
        central: Option<Adapter>,
        sensors: Vec<SensorPeripheral>,
        subscriptions: HashMap<BDAddr, UnboundedSender<PeripheralCmd>>,

        tx: UnboundedSender<PeripheralMsg>,
        rx: UnboundedReceiver<PeripheralCmd>,
    }

    impl std::fmt::Display for PeripheralMgr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "BLE Peripheral Manager for {:?} known sensors",
                self.sensors.len()
            )
        }
    }

    impl PeripheralMgr {
        pub fn new(
            tx: UnboundedSender<PeripheralMsg>,
            rx: UnboundedReceiver<PeripheralCmd>,
        ) -> Self {
            Self {
                central: None,
                sensors: Vec::<SensorPeripheral>::new(),
                subscriptions: HashMap::new(),
                tx,
                rx,
            }
        }

        /// Initialize
        ///
        /// Establishes connection to bluetooth stack.
        /// This function *must* be called before any other operation can be carried out
        pub async fn init(&mut self) -> Result<(), PeripheralError> {
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

        /// Find all SensorPeripherals
        ///
        /// Check all bluetooth peripherals. If one matches with our spec, add to vector
        /// (at this point this is basically a backup, but the sensor usually advertises itself anyway through the bt event channel)
        async fn find_sensors(&mut self) -> Result<(), PeripheralError> {
            let prevlen = self.sensors.len();

            for p in self.central.as_ref().unwrap().peripherals().await.unwrap() {
                if p.properties()
                    .await
                    .unwrap()
                    .unwrap()
                    .local_name
                    .iter()
                    .any(|name| name.contains("MoistureSensor"))
                {
                    let addr = p.properties().await.unwrap().unwrap().address;

                    if self.sensors.iter().any(|p| p.addr == addr) {
                        continue;
                    }

                    log::debug!("found new sensor device! ({addr:?})");
                    self.sensors.push(SensorPeripheral::new(p, addr));
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
        async fn subscribe(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut sensors = self.sensors.clone();

            let p = match sensors.iter_mut().find(|p| p.addr == addr) {
                Some(p) => p,
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("subscribing to data from sensor peripheral");
            match p
                .do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Subscribe)
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("Failed to subscribe to sensor data: {e:?}");
                    return Err(PeripheralError::IOError);
                }
            }

            // start notification thread
            let (mgr_tx, thread_rx) = unbounded_channel();
            let _handle = tokio::spawn(PeripheralMgr::handle_notifications(
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
        async fn unsubscribe(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut sensors = self.sensors.clone();

            let p = match sensors.iter_mut().find(|p| p.addr == addr) {
                Some(p) => p,
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("unsubscribing to data from sensor peripheral");
            match p
                .do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Unsubscribe)
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("Failed to unsubscribe to sensor data: {e:?}");
                    return Err(PeripheralError::IOError);
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
        pub async fn handle_notifications(
            p: SensorPeripheral,
            tx: UnboundedSender<PeripheralMsg>,
            mut rx: UnboundedReceiver<PeripheralCmd>,
        ) -> u32 {
            log::debug!("Hello from notification thread for [{0}]", p.addr);

            let mut stream = match p.peripheral.notifications().await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Could not get notification stream from peripheral: {e:?}");
                    panic!("Could not get notification stream");
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
                        log::debug!("sending data to hub: {d:?}");
                        tx.send(PeripheralMsg::Event(HubEvent::NewData(p.addr, d))).unwrap();
                    },
                    Some(PeripheralCmd{id: _ , msg: HubCmd::StopThread}) = rx.recv() => {
                        log::info!("received stop command from main");
                        break;
                    }
                }
            }

            log::debug!("Leaving notification thread for [{0}]", p.addr);
            0
        }

        /// Handle Command
        ///
        /// Handle commands from the Hub.
        /// Longer-running commands are handled in tasks and send their response async once finished.
        /// Immediate (and some quick) commands are handled directly
        async fn handle_cmd(&mut self, cmd: PeripheralCmd) {
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
                    log::info!("blinking all sensors");
                    // spawn one task to spawn sensor-specific tasks and await them all before sending the Success resp
                    let mut sensors = self.sensors.clone();
                    tokio::spawn(async move {
                        let tasks: Vec<_> = sensors
                            .iter_mut()
                            .map(|peripheral| {
                                let mut p = peripheral.clone();
                                tokio::spawn(async move {
                                    for i in 0..21 {
                                        let led_cmd: [u8; 1] = match i % 2 {
                                            0 => [0],
                                            _ => [1],
                                        };
                                        log::debug!("writing: {:?} to {:?}", led_cmd, p.addr);
                                        let _ = p
                                            .do_action(
                                                LED_CHARACTERISTIC_UUID,
                                                PeripheralAction::Write(led_cmd),
                                            )
                                            .await;
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
                    let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
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
                            log::debug!("writing: {:?} to {:?}", led_cmd, p.addr);
                            let _ = p
                                .do_action(
                                    LED_CHARACTERISTIC_UUID,
                                    PeripheralAction::Write(led_cmd),
                                )
                                .await;
                            time::sleep(Duration::from_millis(200)).await;
                        }
                        task_tx
                            .send(PeripheralMsg::Response(cmd.id, HubResp::Success))
                            .unwrap();
                    });
                }

                HubCmd::ReadFrom(addr) => {
                    log::info!("Reading from peripheral ({addr:?})");
                    let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                        Some(p) => p.clone(),
                        None => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };

                    tokio::spawn(async move {
                        match p
                            .do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Read)
                            .await
                        {
                            Ok(ActionResult::Data(res)) => {
                                log::debug!("read sensor data: {res:?}");
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
                                log::warn!("Failed to read sensor data: {e:?}");
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
                            log::warn!("Failed to subscribe to sensor data!");
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
                            log::warn!("Failed to unsubscribe from sensor data!");
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
                    log::info!("looking for sensors");
                    // todo run in thread somehow
                    match self.find_sensors().await {
                        Err(_) => {
                            log::warn!("Finding new Sensors failed");
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
                    let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                        Some(p) => p.clone(),
                        None => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };
                    log::info!("connecting to peripheral ({addr:?})");

                    tokio::spawn(async move {
                        match p.connect().await {
                            Err(_) => {
                                log::warn!("Failed to connect to sensor!");
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
                    let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                        Some(p) => p.clone(),
                        None => {
                            self.tx
                                .send(PeripheralMsg::Response(cmd.id, HubResp::Failed))
                                .unwrap();
                            return;
                        }
                    };
                    log::info!("disonnecting from peripheral ({addr:?})");

                    tokio::spawn(async move {
                        match p.disconnect().await {
                            Err(_) => {
                                log::warn!("Failed to disconnect from sensor!");
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

        /// Run Peripheral Manager
        ///
        /// Run the PeripheralMgr loop.
        ///
        pub async fn run(&mut self) -> u32 {
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
                                let peripheral = match self.central.as_ref().unwrap().peripheral(&id).await {
                                    Ok(p) => p,
                                    Err(_) => {
                                        continue;
                                    }
                                };
                                let properties = peripheral.properties().await.unwrap();
                                let addr = properties.as_ref().unwrap().address;

                                let name = properties
                                    .and_then(|p| p.local_name)
                                    .map(|local_name| local_name.to_string())
                                    .unwrap_or_default();
                                // we only care about our sensor here
                                if name.contains("MoistureSensor") {
                                    log::info!("DeviceDiscovered: {:?} {}", addr, name);

                                    // add new sensor to list and inform hub
                                    self.sensors.push(SensorPeripheral::new(peripheral, addr));
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceDiscovered(addr))).unwrap();
                                }
                            },
                            CentralEvent::StateUpdate(state) => {
                                log::info!("AdapterStatusUpdate {:?}", state);
                            },
                            CentralEvent::DeviceConnected(id) => {
                                let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = peripheral.properties().await.unwrap();
                                let addr = properties.as_ref().unwrap().address;

                                if self.sensors.iter().any(|p| p.addr == addr) {
                                    log::info!("DeviceConnected: {:?}", addr);
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceConnected(addr))).unwrap();
                                }
                            },
                            CentralEvent::DeviceDisconnected(id) => {
                                let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = peripheral.properties().await.unwrap();
                                let addr = properties.as_ref().unwrap().address;

                                // check if the disconnected device is known to us
                                // peripheral name is not consistently available here, so let's compare address
                                if self.sensors.iter().any(|p| p.addr == addr) {
                                    log::info!("DeviceDisconnected: {:?}", addr);
                                    self.tx.send(PeripheralMsg::Event(HubEvent::DeviceDisconnected(addr))).unwrap();
                                }
                            },
                            // let's ignore other events for now
                            _ => ()
                        }
                    },
                    Some(msg) = self.rx.recv() => {
                        match msg.msg {
                            HubCmd::StopThread => {
                                log::info!("received stop command from main");
                                break;
                            },
                            ref _other => {
                                self.handle_cmd(msg).await;
                            }
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
                    log::warn!("Disconnect failed ({0})", p.addr);
                }
            }

            log::debug!("PeripheralMgr::run() done");
            0
        }
    }
}
