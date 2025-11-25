pub mod error {

    use std::error::Error;

    /// Peripheral Error
    #[derive(Debug)]
    pub enum PeripheralError {
        NoAdapter,
        NoPeripheral,
        ConnectionFailed,
        NoCharacteristic,
        WriteFailed,
        ReadFailed,
        OtherError(Box<dyn Error>),
    }

    impl std::fmt::Display for PeripheralError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                PeripheralError::NoAdapter => write!(f, "No Bluetooth adapter found"),
                PeripheralError::NoPeripheral => write!(f, "No MoistureSensor found"),
                PeripheralError::ConnectionFailed => write!(f, "Connection failed"),
                PeripheralError::NoCharacteristic => write!(f, "BLE Characteristic not found on sensor device"),
                PeripheralError::WriteFailed => write!(f, "Write failed"),
                PeripheralError::ReadFailed => write!(f, "Read failed"),
                PeripheralError::OtherError(e) => write!(f, "Other Error: {e}",)
            }
        }
    }

    impl Error for PeripheralError {}

    impl PeripheralError {

        /// Return a boxed instance
        /// 
        /// not really needed anymore
        pub fn new_boxed(err: PeripheralError) -> Box<dyn Error> {
            Box::new(err)
        }
    }

}

pub mod peripheral {
    use super::error::PeripheralError;

    use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, WriteType, BDAddr, ScanFilter};
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use std::time::Duration;
    use tokio::time;
    use uuid::{uuid,Uuid};
    use crossbeam_channel::{Sender, Receiver, TryRecvError};
    use futures::stream::StreamExt;

    /// Specific Characteristic UUID set by Sensor Peripheral
    /// for controlling the sensor LED
    const LED_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e5");

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
        FindSensors,
        Connect(BDAddr),
        Disconnect(BDAddr),
    }
    
    /// Sensor Peripheral
    /// 
    /// Individual Sensor Peripheral device
    /// Represents one Sensor Peripheral
    /// @todo should i remove this abstraction? i feel like this introduces more complexity than is necessary
    #[derive(Debug,Clone)]
    pub struct SensorPeripheral {
        peripheral: Peripheral,
        addr: BDAddr,
        connected: bool,
    }

    impl std::fmt::Display for SensorPeripheral {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Sensor Peripheral ({:?})", self.addr)
        }
    }

    impl PartialEq for SensorPeripheral {
        fn eq(&self, other: &Self) -> bool {
            self.addr == other.addr
        }
    }

    impl SensorPeripheral {
        pub fn new(p: Peripheral, a: BDAddr) -> Self {
            Self {
                peripheral: p,
                addr: a,
                connected: false,
            }
        }

        pub fn addr(self) -> BDAddr {
            self.addr
        }

        pub fn connected(self) -> bool {
            self.connected
        }

        /// Connect to peripheral
        /// 
        /// Check first if already connected
        pub async fn connect(&mut self) -> Result<bool, PeripheralError> {
            if self.peripheral.is_connected().await.unwrap() {
                log::info!("already connected!");
                return Ok(true);
            }

            match self.peripheral.connect().await {
                Ok(_) => (),
                Err(e) => {
                    log::warn!("Failed to connect to sensor! {:?}", e);
                    return Err(PeripheralError::ConnectionFailed);
                }
            };

            log::debug!("Connected to sensor device");
            log::debug!("Peripheral id: {:?}", self.peripheral.id());
            log::debug!("Properties:");
            let addr = self.peripheral.properties().await.unwrap().unwrap().address;
            log::debug!("{:?}", addr);

            self.connected = true;
            Ok(true)
        }

        pub async fn disconnect(&mut self) -> Result<bool, PeripheralError> {
            if !self.peripheral.is_connected().await.unwrap() {
                log::info!("already disconnected!");
                self.connected = false;
                return Ok(true);
            }

            match self.peripheral.disconnect().await {
                Ok(_) => {
                    log::debug!("disconnected");
                    self.connected = false;
                    Ok(true)
                },
                Err(e) => {
                    log::warn!("Failed to disconnect from sensor {:?}: {:?}", self, e);
                    Err(PeripheralError::ConnectionFailed)
                }
            }
        }

        pub async fn read(self) -> Result<u32, PeripheralError> {
            todo!("reading not yet implemented")
        }

        pub async fn write(&self, uuid: Uuid, data: [u8;1]) -> Result<(), PeripheralError> {
            // discover services and characteristics
            let _ = self.peripheral.discover_services().await;

            let chars = self.peripheral.characteristics();
            let cmd_char = match chars.iter().find(|c| c.uuid == uuid) {
                None => return Err(PeripheralError::NoCharacteristic),
                Some(char) => char.clone()
            };

            match self.peripheral.write(&cmd_char, &data, WriteType::WithResponse).await {
                Ok(_) => Ok(()),
                Err(_) => Err(PeripheralError::WriteFailed)
            }
        }
    }


    /// Peripheral Manager
    /// 
    /// Manager for Peripheral devices
    /// 
    #[derive(Debug,Clone)]
    pub struct PeripheralMgr {
        central: Option<Adapter>,
        sensors: Vec<SensorPeripheral>,
    }

    impl std::fmt::Display for PeripheralMgr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "BLE Peripheral Manager for {:?} known sensors", self.sensors.len())
        }
    }

    impl Default for PeripheralMgr {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PeripheralMgr {

        pub fn new() -> Self {
            Self {
                central: None,
                sensors: Vec::<SensorPeripheral>::new(),
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
        pub async fn find_sensors(&mut self) -> Result<(), PeripheralError> {

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

                    match p.connect().await {
                        Ok(_) => log::debug!("connected"),
                        Err(e) => {
                            log::warn!("Failed to connect to sensor! {:?}", e);
                            return Err(PeripheralError::ConnectionFailed);
                        }
                    };

                    log::debug!("Connected to sensor device");
                    log::debug!("Peripheral id: {:?}", p.id());
                    log::debug!("properties:");
                    let addr = p.properties().await.unwrap().unwrap().address;
                    log::debug!("hw addr: {:?}", addr);
                    self.sensors.push(SensorPeripheral::new(p, addr));
                }
            }

            match self.sensors.len() {
                0 => {
                    log::debug!("no peripheral found");
                    Err(PeripheralError::NoPeripheral)
                },
                _ => Ok(())
            }
        }

        /// Find Peripheral from address
        /// 
        /// Return a SensorPeripheral with the given address, if found
        pub async fn find_sensor(&mut self, addr: BDAddr) -> Result<SensorPeripheral, PeripheralError> {
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
        pub async fn connect(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    match self.find_sensor(addr).await {
                        Ok(p) => {
                            self.sensors.push(p.clone());
                            p
                        },
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
            };
            match p.connect().await {
                Ok(true) => Ok(()),
                Ok(false) => Err(PeripheralError::ConnectionFailed),
                Err(e) => Err(e)
            }
        }

        /// Disconnect Peripheral
        /// 
        /// disconnect from a Peripheral with the given address
        pub async fn disconnect(&mut self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    match self.find_sensor(addr).await {
                        Ok(p) => {
                            self.sensors.push(p.clone());
                            p
                        },
                        Err(e) => {
                            return Err(e);
                        }
                    }
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
        pub async fn blinky_all(&mut self) -> Result<(), PeripheralError> {
            for mut p in self.sensors.clone() {
                dbg!(&p);
                if self.blinky(&mut p).await.is_err() {
                    println!("Failed to blink Peripheral ({:?})", p.addr());
                }
            }
            Ok(())
        }

        /// Blink a Peripheral with given address
        /// 
        /// Perform the Blink routine on a Peripheral with address *addr*
        /// This can fail if no peripheral with the given address is found
        pub async fn blink(&self, addr: BDAddr) -> Result<(), PeripheralError> {
            let mut p = match self.sensors.iter().find(|&p| p.addr == addr) {
                Some(p) => p.clone(),
                None => {
                    return Err(PeripheralError::NoPeripheral);
                }
            };
            self.blinky(&mut p).await
        }

        /// Blink a given SensorPeripheral
        /// 
        /// Perform the Blink routine on a connected SensorPeripheral
        async fn blinky(&self, p: &mut SensorPeripheral) -> Result<(), PeripheralError> {

            for i in 0..21 {
                let led_cmd: [u8;1] = match i%2 {
                    0 => [0],
                    _ => [1]
                };
                log::debug!("writing: {:?}", led_cmd);
                p.write(LED_CHARACTERISTIC_UUID, led_cmd).await?;
                time::sleep(Duration::from_millis(200)).await;
            }
            Ok(())
        }

        /// Run Peripheral Manager
        /// 
        /// Run the PeripheralMgr loop.
        /// 
        pub async fn run(&mut self, tx: Sender<EventMsg>, rx: Receiver<HubMsg>) -> u32 {

            let central_state = match self.central.as_ref().unwrap().adapter_state().await {
                Ok(s) => s,
                Err(_) => {
                    println!("could not get central state!");
                    return 0;
                }
            };
            println!("CentralState: {:?}", central_state);

            // Each adapter has an event stream, we fetch via events(),
            // simplifying the type, this will return what is essentially a
            // Future<Result<Stream<Item=CentralEvent>>>.
            let mut events = match self.central.as_ref().unwrap().events().await {
                Ok(ev) => ev,
                Err(_) => {
                    println!("could not get central events!");
                    return 0;
                }
            };

            // start scanning for devices
            self.central.as_ref().unwrap().start_scan(ScanFilter::default()).await.unwrap();

            println!("event handler intialized!");


            loop {
                // check for an event  - with timeout making this non-blocking
                if let Ok(Some(event)) = time::timeout(Duration::from_nanos(1), events.next()).await {
                    match event {
                        CentralEvent::DeviceDiscovered(id) => {
                            let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                            let properties = peripheral.properties().await.unwrap();
                            let addr = properties.as_ref().unwrap().address;

                            let name = properties
                                .and_then(|p| p.local_name)
                                .map(|local_name| local_name.to_string())
                                .unwrap_or_default();
                            // we only care about our sensor here
                            if name.contains("MoistureSensor") {
                                println!("DeviceDiscovered: {:?} {}", addr, name);
                                tx.send(EventMsg::DeviceDiscovered(addr)).unwrap();
                            }
                        },
                        CentralEvent::StateUpdate(state) => {
                            println!("AdapterStatusUpdate {:?}", state);
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
                                println!("DeviceConnected: {:?} {}", addr, name);
                                tx.send(EventMsg::DeviceConnected(addr)).unwrap();
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
                                println!("DeviceDisconnected: {:?} {}", addr, name);
                                tx.send(EventMsg::DeviceDisconnected(addr)).unwrap();
                            }
                        },
                        CentralEvent::ServicesAdvertisement { id, services } => {
                            if let Some(services) =
                                services.into_iter().find(|&s| s.eq(&LED_CHARACTERISTIC_UUID)) {
                                let peripheral = self.central.as_ref().unwrap().peripheral(&id).await.unwrap();
                                let properties = peripheral.properties().await.unwrap();
                                let addr = properties.unwrap().address;
                                println!("ServicesAdvertisement: {:?}, {:?}", id, services);
                                tx.send(EventMsg::ServiceDiscovered(addr)).unwrap();
                            }
                        },
                        // let's ignore these other events for now
                        _ => ()
                        // CentralEvent::DeviceUpdated(id) => {
                        //     println!("DeviceUpdated: {:?}", id);
                        // },
                        // CentralEvent::ManufacturerDataAdvertisement {
                        //     id,
                        //     manufacturer_data,
                        // } => {
                        //      println!(
                        //          "ManufacturerDataAdvertisement: {:?}, {:?}",
                        //          id, manufacturer_data
                        //      );
                        // },
                        // CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                        //     println!("ServiceDataAdvertisement: {:?}, {:?}", id, service_data);
                        // },
                    }
                }

                // now let's check if something came in from main
                match rx.try_recv() {
                    Ok(HubMsg::StopThread) => {
                        println!("received stop command from main");
                        break;
                    },
                    Ok(HubMsg::Ping) => {
                        println!("received Ping from main");
                    }
                    Ok(HubMsg::BlinkAll) => {
                        println!("blinking all sensors");
                        if self.blinky_all().await.is_err() {
                            println!("BlinkAll failed");
                        }
                    },
                    Ok(HubMsg::Blink(addr)) => {
                        println!("blinking led on peripheral ({addr:?})");
                        if self.blink(addr).await.is_err() {
                            println!("Blink failed for {addr:?}");
                        }
                    },
                    Ok(HubMsg::FindSensors) => {
                        println!("looking for sensors");
                        if self.find_sensors().await.is_err() {
                            println!("Finding Sensors failed");
                        }
                    },
                    Ok(HubMsg::Connect(addr)) => {
                        println!("connecting to peripheral ({addr:?})");
                        if self.connect(addr).await.is_err() {
                            println!("Connection failed ({addr:?})");
                        }
                    },
                    Ok(HubMsg::Disconnect(addr)) => {
                        println!("Disconnecting from peripheral ({addr:?})");
                        if self.disconnect(addr).await.is_err() {
                            println!("Disconnect failed ({addr:?})");
                        }
                    },
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => {
                        println!("disconnected from main! Stopping");
                        break;
                    }
                };
            }

            // let's clean up after ourselves
            if self.central.as_ref().unwrap().stop_scan().await.is_err() {
                println!("Failed to stop Scan");
            }
            1
        }
    }
}