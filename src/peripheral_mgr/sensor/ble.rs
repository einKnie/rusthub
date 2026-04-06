//! Traits and default implementations for BLE connected sensors
//!

use super::super::error::PeripheralError;
use super::{HasPeripheral, Sensor, SensorService};
use btleplug::api::{BDAddr, Characteristic, Peripheral as _, WriteType};
use btleplug::platform::{Peripheral, PeripheralId};
use uuid::Uuid;

/// HasBtPeripheral
///
/// extends HasPeripheral for Bluetooth devices
/// and adds id() and addr() functions
pub trait HasBtPeripheral: HasPeripheral<Peripheral> + SensorService<Uuid> {
    /// Get peripheral id
    fn peripheral_id(&self) -> PeripheralId;
    /// Get peripheral address
    fn addr(&self) -> BDAddr;
    /// Get a sensor service
    fn get_service(&self, id: Uuid) -> Option<Characteristic>;
}

/// Bluetooth Peripheral default implementation for
/// all types that implement HasPeripheral<Peripheral>
impl<T: HasPeripheral<Peripheral> + SensorService<Uuid>> HasBtPeripheral for T {
    fn peripheral_id(&self) -> PeripheralId {
        self.peripheral().id()
    }

    fn addr(&self) -> BDAddr {
        self.peripheral().address()
    }

    fn get_service(&self, id: Uuid) -> Option<Characteristic> {
        self.peripheral()
            .characteristics()
            .iter()
            .find(|c| c.uuid == id)
            .cloned()
    }
}

/// Default implementation for Sensor with bluetooth Peripheral
///
/// This is a generic implementation requiring a few prerequisites.
/// As long as HasBtPeripheral and SensorService<Uuid> are implemented,
/// this implementation is also available.
/// This means that only the relatively short traits have to be
/// implemented by every type that may act as a Bluetooth connected Sensor
impl<T: HasBtPeripheral + Send + Sync + 'static> Sensor for T {
    /// Connect to peripheral
    async fn connect(&mut self) -> Result<(), PeripheralError> {
        if match self.peripheral().is_connected().await {
            Ok(res) => res,
            Err(e) => {
                log::debug!("Cannot determine peripheral connection status: {e:?}");
                return Err(PeripheralError::ConnectionError);
            }
        } {
            log::debug!("already connected!");
            return Ok(());
        }

        match self.peripheral().connect().await {
            Ok(_) => {
                // test if it works when i just discover once here?
                let _ = self.peripheral().discover_services().await;
                Ok(())
            }
            Err(_) => Err(PeripheralError::ConnectionError),
        }
    }

    /// Disconnect from peripheral
    async fn disconnect(&mut self) -> Result<(), PeripheralError> {
        if !match self.peripheral().is_connected().await {
            Ok(res) => res,
            Err(e) => {
                log::debug!("Cannot determine peripheral connection status: {e:?}");
                return Err(PeripheralError::ConnectionError);
            }
        } {
            log::debug!("already disconnected!");
            return Ok(());
        }

        match self.peripheral().disconnect().await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::ConnectionError),
        }
    }

    /// Subscribe to notifications from peripheral
    async fn subscribe(&self) -> Result<(), PeripheralError> {
        let _ = self.peripheral().discover_services().await;

        let char = match self.get_service(Self::read_id()) {
            None => return Err(PeripheralError::NoCharacteristic),
            Some(ch) => ch.clone(),
        };

        match self.peripheral().subscribe(&char).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError),
        }
    }

    /// Unsubscribe from notifications from peripheral
    async fn unsubscribe(&self) -> Result<(), PeripheralError> {
        let _ = self.peripheral().discover_services().await;

        let char = match self.get_service(Self::read_id()) {
            None => return Err(PeripheralError::NoCharacteristic),
            Some(ch) => ch.clone(),
        };

        match self.peripheral().unsubscribe(&char).await {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError),
        }
    }

    /// Read from peripheral
    async fn read(&self) -> Result<Vec<u8>, PeripheralError> {
        let _ = self.peripheral().discover_services().await;

        let char = match self.get_service(Self::read_id()) {
            None => return Err(PeripheralError::NoCharacteristic),
            Some(ch) => ch.clone(),
        };
        match self.peripheral().read(&char).await {
            Ok(res) => Ok(res),
            Err(_) => Err(PeripheralError::IOError),
        }
    }

    /// Write to peripheral
    async fn write(&self, data: [u8; 1]) -> Result<(), PeripheralError> {
        let _ = self.peripheral().discover_services().await;

        let char = match self.get_service(Self::write_id()) {
            None => return Err(PeripheralError::NoCharacteristic),
            Some(ch) => ch.clone(),
        };

        match self
            .peripheral()
            .write(&char, &data, WriteType::WithResponse)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(PeripheralError::IOError),
        }
    }
}
