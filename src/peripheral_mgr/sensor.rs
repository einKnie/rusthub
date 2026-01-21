use crate::peripheral_mgr::error::PeripheralError;

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
        }
    }

    pub fn addr(&self) -> BDAddr {
        self.addr
    }

    /// Connect to peripheral
    ///
    /// Check first if already connected
    pub async fn connect(&mut self) -> Result<(), PeripheralError> {
        if match self.peripheral.is_connected().await {
            Ok(res) => res,
            Err(e) => {
                log::debug!("Cannot determine peripheral connection status: {e:?}");
                return Err(PeripheralError::ConnectionError);
            }
        } {
            log::debug!("already connected!");
            return Ok(());
        }

        match self.peripheral.connect().await {
            Ok(_) => {
                log::debug!("Connected to sensor device");
                log::debug!("Peripheral id: {:?}", self.peripheral.id());
                Ok(())
            },
            Err(_) => {
                Err(PeripheralError::ConnectionError)
            }
        }
    }

    pub async fn disconnect(&mut self) -> Result<(), PeripheralError> {
        if ! match self.peripheral.is_connected().await {
            Ok(res) => res,
            Err(e) => {
                log::debug!("Cannot determine peripheral connection status: {e:?}");
                return Err(PeripheralError::ConnectionError);
            }
        } {
            log::debug!("already disconnected!");
            return Ok(());
        }

        match self.peripheral.disconnect().await {
            Ok(_) => Ok(()),
            Err(_) => {
                Err(PeripheralError::ConnectionError)
            }
        }
    }

    pub async fn do_action(&mut self, uuid: Uuid, action: PeripheralAction) -> Result<ActionResult, PeripheralError> {
        // discover services and characteristics
        let _ = self.peripheral.discover_services().await;

        let ch = match self.peripheral.characteristics().iter().find(|c| c.uuid == uuid) {
            None => return Err(PeripheralError::NoCharacteristic),
            Some(char) => char.clone()
        };

        match action {
            PeripheralAction::Write(data) => {
                match self.write(ch, data).await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e)
                }
            },
            PeripheralAction::Read => {
                match self.read(ch).await {
                    Ok(val) => Ok(ActionResult::Data(val)),
                    Err(e) => Err(e),
                }
            },
            PeripheralAction::Subscribe => {
                match self.subscribe(ch).await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                }
            },
            PeripheralAction::Unsubscribe => {
                match self.unsubscribe(ch).await {
                    Ok(_) => Ok(ActionResult::Success),
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
            Err(_) => Err(PeripheralError::IOError)
        }
    }

    pub async fn write(&self, char: Characteristic, data: [u8;1]) -> Result<(), PeripheralError> {
        match self.peripheral.write(&char, &data, WriteType::WithResponse).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError)
        }
    }

    pub async fn subscribe(&self, char: Characteristic) -> Result<(), PeripheralError> {
        match self.peripheral.subscribe(&char).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError),
        }
    }

    pub async fn unsubscribe(&self, char: Characteristic) -> Result<(), PeripheralError> {
        match self.peripheral.unsubscribe(&char).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError)
        }
    }
}
