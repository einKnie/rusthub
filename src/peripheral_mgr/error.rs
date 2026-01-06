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