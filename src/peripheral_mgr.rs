pub mod sensor;
pub mod error;


pub mod peripheral {
    use crate::peripheral_mgr::error::error::PeripheralError;
    use crate::peripheral_mgr::sensor::sensor::{SensorPeripheral, PeripheralAction, ActionResult};

    use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, BDAddr, ScanFilter};
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use std::time::Duration;
    use tokio::time;
    use uuid::{uuid,Uuid};
    use crossbeam_channel::{Sender, Receiver, TryRecvError};
    use futures::stream::StreamExt;

    /// Specific Characteristic UUID set by Sensor Peripheral
    /// for controlling the sensor LED
    const LED_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e5");
    const SENSOR_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e6");

    /// Event Message
    ///
    /// Sent from PeripheralMgr on bluetooth event
    /// @todo this should probably be changed to contain the peripheral id instead, since that makes more sense
    /// since we get the id automatically with the event *and* we can identify a peripheral much more easily via the id than the address
    /// only thing i need to check is that the id is actually as unique as the address...
    #[derive(Debug)]
    pub enum EventMsg {
        DeviceDiscovered(BDAddr),
        DeviceConnected(BDAddr),
        DeviceDisconnected(BDAddr),
        ServiceDiscovered(BDAddr),
        SearchFailed,
        NewData(BDAddr, u32),
    }

    /// Hub Message
    ///
    /// Sent from main to the PeripheralMgr
    /// To be used for thread control as well as peripheral commands
    /// @todo this should be split in two enums, per usecase
    /// since I will want to reuse the thread control msgs also for other threads
    /// @todo find out how i can check for two types in one stream (box? trait? can en enum impl a trait, even?)
    #[derive(Debug)]
    pub enum HubMsg {
        StopThread,
        Ping,
        BlinkAll,
        Blink(BDAddr),
        ReadFrom(BDAddr),
        Subscribe(BDAddr),
        Unsubscribe(BDAddr),
        FindSensors,
        Connect(BDAddr),
        Disconnect(BDAddr),
    }

    /// Run the Peripheral Manager
    ///
    /// Init and run the Mgr; this should be run as a separate thread
    pub async fn mgr_run(tx: Sender<EventMsg>, rx: Receiver<HubMsg>) -> u32 {
        // init the manager
        let mut mgr = PeripheralMgr::new(tx, rx);
        if mgr.init().await.is_ok() {
            log::info!("Peripheral Manager initialized!");
            mgr.run().await;
        }
        0
    }


    /// Peripheral Manager
    ///
    /// Manager for Peripheral devices
    ///
    #[derive(Debug,Clone)]
    pub struct PeripheralMgr {
        central: Option<Adapter>,
        sensors: Vec<SensorPeripheral>,

        tx: Sender<EventMsg>,
        rx: Receiver<HubMsg>,
    }

    impl std::fmt::Display for PeripheralMgr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "BLE Peripheral Manager for {:?} known sensors", self.sensors.len())
        }
    }

    impl PeripheralMgr {

        pub fn new( tx: Sender<EventMsg>, rx: Receiver<HubMsg>) -> Self {
            Self {
                central: None,
                sensors: Vec::<SensorPeripheral>::new(),
                tx,
                rx
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
                    Err(_) => return Err(PeripheralError::NoAdapter)
                };
                let adapters = match manager.adapters().await {
                    Ok(adp) => adp,
                    Err(_) => return Err(PeripheralError::NoAdapter)
                };
                if adapters.is_empty() {
                    log::warn!("No bluetooth adapter found");
                    return Err(PeripheralError::NoAdapter);
                }

                self.central = Some(adapters.into_iter().nth(0).unwrap());
                log::debug!("Initialized!");
            }
            Ok(())
        }

        /// Find all SensorPeripherals
        ///
        /// Check all bluetooth peripherals. If one matches with our spec, then connect to it and add to vector
        async fn find_sensors(&mut self) -> Result<(), PeripheralError> {
            let prevlen = self.sensors.len();
            log::info!("known sensors: {prevlen:?}");

            for p in self.central.as_ref().unwrap().peripherals().await.unwrap() {
                if p.properties()
                    .await
                    .unwrap()
                    .unwrap()
                    .local_name
                    .iter()
                    .any(|name| name.contains("MoistureSensor"))
                {
                    log::debug!("found sensor device!");
                    let addr = p.properties().await.unwrap().unwrap().address;

                    match self.sensors.iter().find(|&p| p.addr == addr) {
                        Some(_) => {
                            log::debug!("Sensor already known!");
                            continue;
                        },
                        None => (),
                    };

                    match p.connect().await {
                        Ok(_) => log::debug!("connected"),
                        Err(e) => {
                            log::warn!("Failed to connect to sensor! {:?}", e);
                            return Err(PeripheralError::ConnectionFailed);
                        }
                    };

                    log::debug!("Connected to sensor device");
                    log::debug!("Peripheral id: {:?}", p.id());
                    log::debug!("hw addr: {:?}", addr);
                    self.sensors.push(SensorPeripheral::new(p, addr));
                }
            }

            if self.sensors.len() == prevlen {
                log::debug!("no peripheral found (old len: {prevlen:?} - newlen: {:?})", self.sensors.len());
                Err(PeripheralError::NoPeripheral)
            } else {
                log::debug!("found a new sensor! (old len: {prevlen:?} - newlen: {:?})", self.sensors.len());
                Ok(())
            }
        }

        /// Find Peripheral from address
        ///
        /// Return a SensorPeripheral with the given address, if found
        async fn find_sensor(&mut self, addr: BDAddr) -> Result<SensorPeripheral, PeripheralError> {
            let mut found = Vec::<Peripheral>::new();

            for p in self.central.as_ref().unwrap().peripherals().await.unwrap() {
                if p.properties()
                    .await
                    .unwrap()
                    .unwrap()
                    .address == addr
                {
                    log::debug!("found sensor device!");
                    found.push(p);
                }
            }

            match found.len() {
                0 => {
                    log::debug!("no peripheral found");
                    Err(PeripheralError::NoPeripheral)
                },
                1 => {
                    log::debug!("one peripheral found");
                    Ok(SensorPeripheral::new(found.pop().unwrap(), addr))
                }
                _ => {
                    log::debug!("multiple peripherals found");
                    Err(PeripheralError::NoPeripheral)
                }
            }
        }

        /// Connect Peripheral
        ///
        /// Connect to a Peripheral with the given address
        /// If the Peripheral is not known yet, find it and add it to the list as well
        /// @todo check if this even works, when i want to later address a peripheral added to the list here
        async fn connect(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            match p.connect().await {
                Ok(true) => Ok(()),
                Ok(false) => Err(PeripheralError::ConnectionFailed),
                Err(e) => {
                    log::debug!("connection failed: {e:?}");
                    Err(e)
                }
            }
        }

        /// Disconnect Peripheral
        ///
        /// disconnect from a Peripheral with the given address
        async fn disconnect(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            match p.disconnect().await {
                Ok(true) => Ok(()),
                Ok(false) => Err(PeripheralError::ConnectionFailed),
                Err(e) => Err(e)
            }
        }

        /// Blink all SensorPeripherals
        ///
        /// Perform the Blink routine on all known SensorPeripherals, sequentially
        async fn blinky_all(&mut self) -> Result<(), PeripheralError> {
            for mut p in self.sensors.clone() {
                dbg!(&p);
                if self.blink_sensor(&mut p).await.is_err() {
                    log::warn!("Failed to blink Peripheral ({:?})", p.addr());
                }
            }
            Ok(())
        }

        /// Blink a Peripheral with given address
        ///
        /// Perform the Blink routine on a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn blink(&self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            self.blink_sensor(&mut p).await
        }

        /// Blink a given SensorPeripheral
        ///
        /// Perform the Blink routine on a connected SensorPeripheral
        async fn blink_sensor(&self, p: &mut SensorPeripheral) -> Result<(), PeripheralError> {

            for i in 0..21 {
                let led_cmd: [u8;1] = match i%2 {
                    0 => [0],
                    _ => [1]
                };
                log::debug!("writing: {:?}", led_cmd);
                p.do_action(LED_CHARACTERISTIC_UUID, PeripheralAction::Write(led_cmd)).await?;
                time::sleep(Duration::from_millis(200)).await;
            }
            Ok(())
        }

        /// Read from a Peripheral with given address
        ///
        /// Read sensor data from a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn read(&self, addr: BDAddr) -> Result<u32, PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("reading from sensor peripheral");
            let data = match p.do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Read).await {
                Ok(ActionResult::Data(res)) => {
                    log::debug!("read sensor data: {res:?}");
                    res
                },
                Ok(res) => {
                    log::debug!("unexpected ActionResult received: {res:?}");
                    return Err(PeripheralError::NoPeripheral); // todo better error here
                },
                Err(e) => {
                    log::warn!("Failed to read sensor data: {e:?}");
                    return Err(PeripheralError::NoPeripheral); // todo better error here
                }
            };
            log::info!("Read from sensor: {data:?}");
            Ok(data)
        }

        /// Subscribe to data from a Peripheral with given address
        ///
        /// Subscribe to data updates from a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn subscribe(&self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("subscribing to data from sensor peripheral");
            match p.do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Subscribe).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    log::warn!("Failed to subscribe to sensor data: {e:?}");
                    Err(PeripheralError::NoPeripheral) // todo better error here
                }
            }
        }

        /// Unsubscribe to data from a Peripheral with given address
        ///
        /// Unubscribe to data updates from a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        async fn unsubscribe(&self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            log::debug!("unsubscribing to data from sensor peripheral");
            match p.do_action(SENSOR_CHARACTERISTIC_UUID, PeripheralAction::Unsubscribe).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    log::warn!("Failed to unsubscribe to sensor data: {e:?}");
                    Err(PeripheralError::NoPeripheral) // todo better error here
                }
            }
        }

        /// Run Peripheral Manager
        ///
        /// Run the PeripheralMgr loop.
        ///
        pub async fn run(&mut self) -> u32 {

            let central_state = match self.central.as_ref().unwrap().adapter_state().await {
                Ok(s) => s,
                Err(_) => {
                    log::error!("could not get central state!");
                    return 0;
                }
            };
            log::info!("CentralState: {:?}", central_state);

            // Each adapter has an event stream, we fetch via events(),
            // simplifying the type, this will return what is essentially a
            // Future<Result<Stream<Item=CentralEvent>>>.
            let mut events = match self.central.as_ref().unwrap().events().await {
                Ok(ev) => ev,
                Err(_) => {
                    log::error!("could not get central events!");
                    return 0;
                }
            };

            // start scanning for devices
            self.central.as_ref().unwrap().start_scan(ScanFilter::default()).await.unwrap();

            log::info!("event handler intialized!");

            loop {
                // check for an event  - with timeout making this non-blocking
                if let Ok(Some(event)) = time::timeout(Duration::from_nanos(1), events.next()).await {
                    match event {
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
                                self.tx.send(EventMsg::DeviceDiscovered(addr)).unwrap();
                            }
                        },
                        CentralEvent::StateUpdate(state) => {
                            log::info!("AdapterStatusUpdate {:?}", state);
                        },
                        CentralEvent::DeviceConnected(id) => {
                            let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                            let properties = peripheral.properties().await.unwrap();
                            let addr = properties.as_ref().unwrap().address;

                            let name = properties
                                .and_then(|p| p.local_name)
                                .map(|local_name| local_name.to_string())
                                .unwrap_or_default();
                            // we only care about our sensor here
                            if name.contains("MoistureSensor") {
                                log::info!("DeviceConnected: {:?} {}", addr, name);
                                self.tx.send(EventMsg::DeviceConnected(addr)).unwrap();
                            }
                        },
                        CentralEvent::DeviceDisconnected(id) => {
                            let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                            let properties = peripheral.properties().await.unwrap();
                            let addr = properties.as_ref().unwrap().address;

                            let name = properties
                                .and_then(|p| p.local_name)
                                .map(|local_name| local_name.to_string())
                                .unwrap_or_default();
                            // we only care about our sensor here
                            if name.contains("MoistureSensor") {
                                log::info!("DeviceDisconnected: {:?} {}", addr, name);
                                self.tx.send(EventMsg::DeviceDisconnected(addr)).unwrap();
                            }
                        },
                        CentralEvent::ServicesAdvertisement { id, services } => {
                            if let Some(services) =
                                services.into_iter().find(|&s| s.eq(&LED_CHARACTERISTIC_UUID)) {
                                let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = peripheral.properties().await.unwrap();
                                let addr = properties.unwrap().address;
                                log::info!("ServicesAdvertisement: {:?}, {:?}", id, services);
                                self.tx.send(EventMsg::ServiceDiscovered(addr)).unwrap();
                            }
                        },
                        // let's ignore other events for now
                        _ => ()
                    }
                }

                // now let's check if something came in from main
                match self.rx.try_recv() {
                    Ok(HubMsg::StopThread) => {
                        log::info!("received stop command from main");
                        break;
                    },
                    Ok(HubMsg::Ping) => {
                        log::info!("received Ping from main");
                    }
                    Ok(HubMsg::BlinkAll) => {
                        log::info!("blinking all sensors");
                        if self.blinky_all().await.is_err() {
                            log::warn!("BlinkAll failed");
                        }
                    },
                    Ok(HubMsg::Blink(addr)) => {
                        log::info!("blinking led on peripheral ({addr:?})");
                        if self.blink(addr).await.is_err() {
                            log::warn!("Blink failed for {addr:?}");
                        }
                    },
                    Ok(HubMsg::ReadFrom(addr)) => {
                        log::info!("Reading from periphaeral ({addr:?})");
                        match self.read(addr).await {
                            Err(_) => {
                                log::warn!("Failed to read from sensor ({addr:?})");
                            },
                            Ok(res) => {
                                log::info!("sending new value ({res:?}) to hub");
                                self.tx.send(EventMsg::NewData(addr, res)).unwrap();
                            }
                        }
                    }
                    Ok(HubMsg::Subscribe(addr)) => {
                        if self.subscribe(addr).await.is_err() {
                            log::warn!("Failed to subscribe to sensor data!");
                        }
                    },
                    Ok(HubMsg::Unsubscribe(addr)) => {
                        if self.unsubscribe(addr).await.is_err() {
                            log::warn!("Failed to unsubscribe!");
                        }
                    },
                    Ok(HubMsg::FindSensors) => {
                        log::info!("looking for sensors");
                        if self.find_sensors().await.is_err() {
                            log::warn!("Finding Sensors failed");
                            self.tx.send(EventMsg::SearchFailed).unwrap();
                        }
                    },
                    Ok(HubMsg::Connect(addr)) => {
                        log::info!("connecting to peripheral ({addr:?})");
                        if self.connect(addr).await.is_err() {
                            log::warn!("Connection failed ({addr:?})");
                        }
                    },
                    Ok(HubMsg::Disconnect(addr)) => {
                        log::info!("Disconnecting from peripheral ({addr:?})");
                        if self.disconnect(addr).await.is_err() {
                            log::warn!("Disconnect failed ({addr:?})");
                        }
                    },
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => {
                        log::info!("disconnected from main! Stopping");
                        break;
                    }
                };

                // check notfication channels
                // TODO: this does not really work
                // sometimes i receive notifications, mostly not though
                // not sure which side is responsible yet (arduino ble, or rust btleplug or even bluez)
                for p in self.sensors.iter() {
                    //log::debug!("checking notifications");
                    let mut stream = match p.peripheral.notifications().await {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("Could not get notification stream from peripheral: {e:?}");
                            continue;
                        }
                    };

                    if let Ok(Some(data)) = time::timeout(Duration::from_nanos(1), stream.next()).await {
                        log::debug!("Received data notification from sensor!");
                        let d = match <[u8;4]>::try_from(&data.value[..4]) {
                            Ok(arr) => u32::from_le_bytes(arr),
                            Err(_) => {
                                log::debug!("invalid value notification received: {data:?}");
                                continue;
                            }
                        };
                        log::debug!("sending data to hub: {d:?}");
                        self.tx.send(EventMsg::NewData(p.addr, d)).unwrap();
                    }

                }
            }

            log::debug!("PeripheralMgr::run() ending");

            // let's clean up after ourselves
            if self.central.as_ref().unwrap().stop_scan().await.is_err() {
                log::warn!("Failed to stop Scan");
            }
            1
        }
    }
}