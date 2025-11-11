
pub mod ble_mgr {

    use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType, Characteristic, BDAddr};
    use btleplug::platform::{Adapter, Manager, Peripheral};
    use std::error::Error;
    use std::time::Duration;
    use tokio::time;
    use uuid::{uuid,Uuid};
    use log;

    const LED_CHARACTERISTIC_UUID: Uuid = uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e5"); // uuid as set by device


    //// error

    #[derive(Debug)]
    enum BluetoothError{
        NoAdapter,
        NotInitialized,
        NoSensor,
        ConnectionFailed,
        NoCharacteristic,
        WriteFailed,
        ReadFailed,
    }


    impl std::fmt::Display for BluetoothError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                BluetoothError::NoAdapter => write!(f, "No Bluetooth adapter found"),
                BluetoothError::NotInitialized => write!(f, "Bluetooth module not initialized"),
                BluetoothError::NoSensor => write!(f, "No MoistureSensor found"),
                BluetoothError::ConnectionFailed => write!(f, "Connection failed"),
                BluetoothError::NoCharacteristic => write!(f, "BLE Characteristic not found on sensor device"),
                BluetoothError::WriteFailed => write!(f, "Write failed"),
                BluetoothError::ReadFailed => write!(f, "Read failed"),
            }
        }
    }

    impl Error for BluetoothError {}


    //// connectionmgr

    // pub trait ConnectionMgr {

    //     fn new() -> Self;

    //     async fn init(&mut self) -> Result<(), Box<dyn Error>>;
    //     async fn connect_sensor(&mut self) -> Result<(), Box<dyn Error>>;
    //     async fn disconnect_sensor(&mut self) -> Result<(), Box<dyn Error>>;

    //     fn identify(&self) {
    //         log::info!("ConnectionMgr");
    //     }
    // }

    #[derive(Debug)]
    pub struct BleSensor {
        peripheral: Peripheral,
        addr: BDAddr,

        connected: bool,
    }

    impl std::fmt::Display for BleSensor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Sensor Peripheral ({:?})", self.addr)
        }
    }

    impl BleSensor {
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

        pub async fn connect(mut self) -> Result<bool, Box<dyn Error>> {
            if self.peripheral.is_connected().await.unwrap() {
                log::info!("already connected!");
                return Ok(true);
            }

            match self.peripheral.connect().await {
                Ok(_) => log::debug!("connected"),
                Err(e) => {
                    log::warn!("Failed to connect to sensor! {:?}", e);
                    return Err(Box::new(BluetoothError::ConnectionFailed));
                }
            };

            log::debug!("Connected to sensor device");
            log::debug!("Peripheral id: {:?}", self.peripheral.id());
            log::debug!("properties:");
            let addr = self.peripheral.properties().await.unwrap().unwrap().address;
            log::debug!("{:?}", addr);

            self.connected = true;
            Ok(true)
        }

        pub async fn disconnect(mut self) -> Result<bool, Box<dyn Error>> {
            if !self.peripheral.is_connected().await.unwrap() {
                log::info!("already disconnected!");
                self.connected = false;
                return Ok(true);
            }

            match self.peripheral.disconnect().await {
                Ok(_) => {
                    log::debug!("disconnected");
                    self.connected = false;
                    return Ok(true);
                },
                Err(e) => {
                    log::warn!("Failed to disconnect from sensor {:?}", self);
                    return Err(Box::new(BluetoothError::ConnectionFailed));
                }
            }
        }

        pub async fn blinky(&self) -> Result<(), Box<dyn Error>> {

            let mut cleanup = false;
            if !self.connected {
                self.peripheral.connect().await?;
                cleanup = true;
            }

            // discover services and characteristics
            self.peripheral.discover_services().await?;

            // find the characteristic we want
            let chars = self.peripheral.characteristics();
            let cmd_char = match chars.iter().find(|c| c.uuid == LED_CHARACTERISTIC_UUID) {
                None => return Err(Box::new(BluetoothError::NoCharacteristic)),
                Some(char) => char.clone()
            };

            log::debug!("command char: {:?}", cmd_char);

            // blinky
            for i in 0..21 {
                let led_cmd: [u8;1] = match i%2 {
                    0 => [0],
                    _ => [1]
                };
                log::debug!("writing: {:?}", led_cmd);
                let _ = self.peripheral.write(&cmd_char, &led_cmd, WriteType::WithResponse).await;
                time::sleep(Duration::from_millis(200)).await;
            }

            if cleanup {
                self.peripheral.disconnect().await?;
            }
            Ok(())
        }
    }

    pub struct BleMgr {
        central: Option<Adapter>,
        pub sensors: Vec<BleSensor>,
    }

    impl std::fmt::Display for BleMgr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "BLE Peripheral Manager for {:?} known sensors", self.sensors.len())
        }
    }

    impl BleMgr {

        pub fn new() -> Self {
            Self {
                central: None,
                sensors: Vec::<BleSensor>::new(),
            }
        }

        pub async fn init(&mut self) -> Result<(), Box<dyn Error>> {
            if self.central.is_none() {
                let manager = Manager::new().await?;
                let adapters = manager.adapters().await?;
                if adapters.is_empty() {
                    log::warn!("No bluetooth adapter found");
                    return Err(Box::new(BluetoothError::NoAdapter));
                }
                    
                self.central = Some(adapters.into_iter().nth(0).unwrap());
                log::debug!("Initialized!");
            }
            Ok(())
        }

        pub async fn find_sensors(&mut self) -> Result<(), Box<dyn Error>> {
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
                            return Err(Box::new(BluetoothError::ConnectionFailed));
                        }
                    };

                    log::debug!("Connected to sensor device");
                    log::debug!("Peripheral id: {:?}", p.id());
                    log::debug!("properties:");
                    let addr = p.properties().await.unwrap().unwrap().address;
                    log::debug!("hw addr: {:?}", addr);
                    self.sensors.push(BleSensor::new(p, addr));
                }
            }
            Ok(())
        }
    }

    // //// ble

    // pub struct Ble {
    //     central: Option<Adapter>,
    //     sensor: Option<Peripheral>,

    //     pub connected: bool,
    // }

    // impl Ble {
    //     async fn find_sensor(&self) -> Option<Peripheral> {
    //         for p in self.central.as_ref().unwrap().peripherals().await.unwrap() {
    //             if p.properties()
    //                 .await
    //                 .unwrap()
    //                 .unwrap()
    //                 .local_name
    //                 .iter()
    //                 .any(|name| name.contains("MoistureSensor"))
    //             {
    //                 log::debug!("found sensor device!");
    //                 return Some(p);
    //             }
    //         }
    //         None
    //     }

    //     async fn get_characteristic(&self, uuid: Uuid) -> Result<Characteristic, Box<dyn Error>> {
    //         if !self.connected {
    //             return Err(Box::new(BluetoothError::NoSensor));
    //         }

    //         // find the characteristic we want
    //         let chars = self.sensor.as_ref().unwrap().characteristics();
    //         match chars.iter().find(|c| c.uuid == uuid) {
    //             None => Err(Box::new(BluetoothError::NoCharacteristic)),
    //             Some(char) => Ok(char.clone())
    //         }
    //     }

    //     async fn blinky(&self) -> Result<(), Box<dyn Error>> {

    //         let s = match &self.sensor {
    //             None => {
    //                 log::debug!("no performing blinky on None sensor!");
    //                 return Err(Box::new(BluetoothError::NoSensor));
    //             },
    //             Some(sensor) => sensor
    //         };

    //         // discover services and characteristics
    //         s.discover_services().await?;

    //         // find the characteristic we want
    //         let cmd_char = match self.get_characteristic(LED_CHARACTERISTIC_UUID).await {
    //             Err(e) => return Err(e),
    //             Ok(char) => char
    //         };

    //         log::debug!("command char: {}", cmd_char);

    //         // blinky
    //         for i in 0..21 {
    //             let led_cmd: [u8;1] = match i%2 {
    //                 0 => [0],
    //                 _ => [1]
    //             };
    //             log::debug!("writing: {:?}", led_cmd);
    //             let _ = s.write(&cmd_char, &led_cmd, WriteType::WithResponse).await;
    //             time::sleep(Duration::from_millis(200)).await;
    //         }

    //         Ok(())
    //     }

    //     fn identify(&self) {
    //         log::info!("Ble");
    //     }
    // }

    // impl ConnectionMgr for Ble {
    //     fn new() -> Self {
    //         Self {
    //             central: None,
    //             sensor: None,

    //             connected: false,
    //         }
    //     }

    //     async fn init(&mut self) -> Result<(), Box<dyn Error>> {
    //         if self.central.is_none() {
    //             let manager = Manager::new().await?;
    //             let adapters = manager.adapters().await?;
    //             if adapters.is_empty() {
    //                 log::warn!("No bluetooth adapter found");
    //                 return Err(Box::new(BluetoothError::NoAdapter));
    //             }
                    
    //             self.central = Some(adapters.into_iter().nth(0).unwrap());
    //             log::debug!("Initialized!");
    //         }
    //         Ok(())
    //     }

    //     async fn connect_sensor(&mut self) -> Result<(), Box<dyn Error>> {

    //         let sensor = match &self.sensor {
    //             Some(s) => {
    //                 s
    //             },
    //             None => {
    //                 log::debug!("looking for new sensor!");

    //                 if self.central.is_none() {
    //                     log::warn!("Bluetooth is not initialized!");
    //                     return Err(Box::new(BluetoothError::NotInitialized));
    //                 }

    //                 // start scanning for devices
    //                 self.central.as_ref().unwrap().start_scan(ScanFilter::default()).await?;
    //                 // instead of waiting, you can use central.events() to get a stream which will
    //                 // notify you of new devices, for an example of that see examples/event_driven_discovery.rs
    //                 time::sleep(Duration::from_secs(1)).await;

    //                 &match self.find_sensor().await {
    //                     None => {
    //                         log::warn!("Sensor not found!");
    //                         return Err(Box::new(BluetoothError::NoSensor));
    //                     },
    //                     Some(s) => {
    //                         s
    //                     }
    //                 }
    //             }
    //         };

    //         if sensor.is_connected().await.unwrap() {
    //             log::info!("already connected!");
    //             return Ok(());
    //         }

    //         match sensor.connect().await {
    //             Ok(_) => log::debug!("connected"),
    //             Err(e) => {
    //                 log::warn!("Failed to connect to sensor! {:?}", e);
    //                 return Err(Box::new(BluetoothError::ConnectionFailed));
    //             }
    //         };

    //         log::debug!("Connected to sensor device");
    //         log::debug!("Peripheral id: {:?}", sensor.id());
    //         log::debug!("properties:");
    //         log::debug!("{:?}", sensor.properties().await.unwrap().unwrap().address);

    //         self.connected = true;
    //         self.sensor = Some(sensor.clone());
    //         Ok(())
    //     }

    //     async fn disconnect_sensor(&mut self) -> Result<(), Box<dyn Error>> {
    //         match &self.sensor {
    //             Some(p) => {
    //                 p.disconnect().await?;
    //                 self.connected = false;
    //                 Ok(())
    //             },
    //             None => Ok(())
    //         }
    //     }



    // }

}
