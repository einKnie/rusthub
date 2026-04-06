//! Sensor abstraction
//!
//! The plan is to use this to allow PeripheralMgr to handle arbitrary sensor peripherals
//!
//! For a Sensor to be managed, it has to implement at least
//! * SensorService, for discovery
//! * Sensor, for interaction with the bluetooth peripheral
//!
//! A full default implementation for Sensor exists for bluetooth sensors
//! in sensor_ble

pub mod ble;
pub mod platform;

use super::error::PeripheralError;

/// SensorService
///
/// Generic Sensor service identification
/// must be implemented for specific protocol with any datatype T as needed
pub trait SensorService<T> {
    /// Return id for the service provided by the sensor
    ///
    /// this is used for sensor identification in PeripheralMgr
    fn service_id() -> T;

    /// Return id for the service provided from which data can be read
    fn read_id() -> T;

    /// Return id for the service to which data can be written
    fn write_id() -> T;
}

/// HasPeripheral
pub trait HasPeripheral<T> {
    /// Return a new type which implements HasPeripheral
    fn new_with_peripheral(t: T) -> Self;
    /// Return blutooth Peripheral
    fn peripheral(&self) -> T;
}

/// Sensor
///
/// Generic Sensor Peripheral trait
/// Provides functions that all connections to sensors should provide
/// the implementation is protocol specific
pub trait Sensor: Sync + Send + 'static {
    /// Connect to device
    fn connect(&mut self) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;

    /// Disconnect from device
    fn disconnect(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;

    /// Subscribe to data from device
    fn subscribe(&self) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;

    /// Unsubscribe from data from device
    fn unsubscribe(&self) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;

    /// Write one byte of data to device
    fn write(
        &self,
        data: [u8; 1],
    ) -> impl std::future::Future<Output = Result<(), PeripheralError>> + Send;

    /// Read data from device
    fn read(&self) -> impl std::future::Future<Output = Result<Vec<u8>, PeripheralError>> + Send;
}
