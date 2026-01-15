use std::error::Error;

/// Peripheral Error
#[derive(Debug)]
pub enum PeripheralError {
    NoAdapter,
    NoPeripheral,
    NoCharacteristic,
    ReadFailed,
    ConnectionError,
    IOError,
    InvalidData,
}

impl std::fmt::Display for PeripheralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeripheralError::NoAdapter => write!(f, "No Bluetooth adapter found"),
            PeripheralError::NoPeripheral => write!(f, "No MoistureSensor found"),
            PeripheralError::NoCharacteristic => write!(f, "BLE Characteristic not found on sensor device"),
            PeripheralError::ReadFailed => write!(f, "Read failed"),
            PeripheralError::IOError => write!(f, "Read/Write error"),
            PeripheralError::ConnectionError => write!(f, "Connection error"),
            PeripheralError::InvalidData => write!(f, "Unexpected or invalid data"),
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
