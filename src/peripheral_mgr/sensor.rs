pub mod sensor {

    use crate::peripheral_mgr::error::error::PeripheralError;

    use btleplug::api::{Peripheral as _, WriteType, BDAddr, Characteristic};
    use btleplug::platform::Peripheral;
    use uuid::Uuid;

    #[derive(Debug)]
    pub enum PeripheralAction {
        Write([u8;1]),
        Read,
        Subscribe,
        Unsubscribe,
    }

    #[derive(Debug)]
    pub enum ActionResult {
        Success,
        Data(u32),
    }
    
    /// Sensor Peripheral
    /// 
    /// Individual Sensor Peripheral device
    /// Represents one Sensor Peripheral
    /// @todo should i remove this abstraction? i feel like this introduces more complexity than is necessary
    #[derive(Debug,Clone)]
    pub struct SensorPeripheral {
        pub peripheral: Peripheral,
        pub addr: BDAddr,
        connected: bool,
        value: u32,
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
                value: 0,
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
            if match self.peripheral.is_connected().await {
                Ok(res) => res,
                Err(e) => {
                    log::debug!("Cannot determine peripheral connection status: {e:?}");
                    false
                }
            } {
                log::info!("already connected!");
                self.connected = true;
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
            if ! match self.peripheral.is_connected().await {
                Ok(res) => res,
                Err(e) => {
                    log::debug!("Cannot determine peripheral connection status: {e:?}");
                    true
                }
            } {
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

        pub async fn do_action(&mut self, uuid: Uuid, action: PeripheralAction) -> Result<ActionResult, PeripheralError> {
            // discover services and characteristics
            let _ = self.peripheral.discover_services().await;

            let chars = self.peripheral.characteristics();
            let cmd_char = match chars.iter().find(|c| c.uuid == uuid) {
                None => return Err(PeripheralError::NoCharacteristic),
                Some(char) => char.clone()
            };

            match action {
                PeripheralAction::Write(data) => {
                    match self.write(cmd_char, data).await {
                        Ok(_) => {
                            log::debug!("Write success");
                            Ok(ActionResult::Success)
                        },
                        Err(e) => Err(e)
                    }
                },
                PeripheralAction::Read => {
                    match self.read(cmd_char).await {
                        Ok(val) => {
                            log::debug!("Read success: {val:?}");
                            Ok(ActionResult::Data(val))
                        },
                        Err(e) => Err(e),
                    }
                },
                PeripheralAction::Subscribe => {
                    match self.subscribe(cmd_char).await {
                        Ok(_) => {
                            log::debug!("Subscribe success");
                            Ok(ActionResult::Success)
                        },
                        Err(e) => Err(e),
                    }
                },
                PeripheralAction::Unsubscribe => {
                    match self.unsubscribe(cmd_char).await {
                        Ok(_) => {
                            log::debug!("Unsubscribe success");
                            Ok(ActionResult::Success)
                        },
                        Err(e) => Err(e),
                    }
                }
            }
        }

        pub async fn read(&self, char: Characteristic) -> Result<u32, PeripheralError> {
            match self.peripheral.read(&char).await {
                Ok(res) => {
                    let d = match <[u8;4]>::try_from(&res[..4]) {
                        Ok(arr) => arr,
                        Err(_) => {
                            return Err(PeripheralError::ReadFailed);
                        }
                    };
                    Ok(u32::from_le_bytes(d))
                },
                Err(_) => Err(PeripheralError::ReadFailed)
            }
        }

        pub async fn write(&self, char: Characteristic, data: [u8;1]) -> Result<(), PeripheralError> {
            match self.peripheral.write(&char, &data, WriteType::WithResponse).await {
                Ok(_) => Ok(()),
                Err(_) => Err(PeripheralError::WriteFailed)
            }
        }

        pub async fn subscribe(&self, char: Characteristic) -> Result<(), PeripheralError> {
            match self.peripheral.subscribe(&char).await {
                Ok(_) => {
                    log::debug!("Subscribed!");
                    Ok(())
                },
                Err(e) => {
                    log::warn!("Subscribing failed: {e:?}");
                    Err(PeripheralError::WriteFailed)
                }
            }
        }

        pub async fn unsubscribe(&self, char: Characteristic) -> Result<(), PeripheralError> {
            match self.peripheral.unsubscribe(&char).await {
                Ok(_) => Ok(()),
                Err(_) => Err(PeripheralError::WriteFailed)
            }
        }
    }
}