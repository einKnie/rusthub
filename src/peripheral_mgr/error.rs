//! Error types for PeripheralMgr
//!

use std::error::Error;

/// Peripheral Error
#[derive(Debug, Clone)]
pub enum PeripheralError {
    /// No BT adapter found
    NoAdapter,
    /// No BT peripheral detected
    NoPeripheral,
    /// No characteristic detected
    NoCharacteristic,
    /// Connection to peripheral failed
    ConnectionError,
    /// Read/Write operation failed
    IOError,
    /// Invalid/unexpected data read from sensor
    InvalidData(Vec<u8>),
}

impl std::fmt::Display for PeripheralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeripheralError::NoAdapter => write!(f, "No Bluetooth adapter found"),
            PeripheralError::NoPeripheral => write!(f, "No MoistureSensor found"),
            PeripheralError::NoCharacteristic => {
                write!(f, "BLE Characteristic not found on sensor device")
            }
            PeripheralError::IOError => write!(f, "Read/Write error"),
            PeripheralError::ConnectionError => write!(f, "Connection error"),
            PeripheralError::InvalidData(d) => write!(f, "Invalid data received: {d:?}"),
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
