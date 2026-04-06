//! Sensor implementations

pub mod moisture {
    //! Implementation for Moisture Sensor
    //!

    use crate::peripheral_mgr::sensor::{HasPeripheral, Sensor, SensorService};
    use crate::peripheral_mgr::{
        ActionResult, ManagedSensor, PeripheralAction, bt_peripheral::ManagedBtSensor,
        error::PeripheralError,
    };
    use btleplug::api::Peripheral as _;
    use btleplug::platform::Peripheral;
    use uuid::Uuid;

    /// Moisture Sensor Peripheral
    ///
    /// Represents one individual Moisture Sensor Peripheral
    /// Implements ManagedSensor, SensorService, Sensor, HasBtPeripheral
    #[derive(Debug, Clone)]
    pub struct MoistureSensor {
        /// btleplug Peripheral object as returned from adapter
        pub peripheral: Peripheral,
    }

    impl MoistureSensor {}

    impl std::fmt::Display for MoistureSensor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Moisture Sensor ({})", self.peripheral.id())
        }
    }

    impl PartialEq for MoistureSensor {
        fn eq(&self, other: &Self) -> bool {
            self.peripheral.address() == other.peripheral.address()
        }
    }

    // Implement SensorService with service Uuids as defined in sensor firmware
    impl SensorService<Uuid> for MoistureSensor {
        fn service_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e4")
        }

        fn read_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e6")
        }

        fn write_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67e5")
        }
    }

    // Implement HasPeripheral<Peripheral>
    // so we can get HasBtPeripheral (see sensor.rs)
    impl HasPeripheral<Peripheral> for MoistureSensor {
        fn new_with_peripheral(p: Peripheral) -> Self {
            Self { peripheral: p }
        }

        fn peripheral(&self) -> Peripheral {
            self.peripheral.clone()
        }
    }

    // Implement ManagedSensor
    impl ManagedSensor for MoistureSensor {
        async fn do_action(
            &mut self,
            action: PeripheralAction,
        ) -> Result<ActionResult, PeripheralError>
        where
            Self: Send + Sync + 'static,
        {
            match action {
                PeripheralAction::Write(data) => match self.write(data).await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
                PeripheralAction::Read => match self.read().await {
                    Ok(val) => {
                        // sensor-specific data extraction, as btle api read always returns bytes
                        let d = match <[u8; 4]>::try_from(&val[..4]) {
                            Ok(arr) => u32::from_le_bytes(arr),
                            Err(_) => {
                                return Err(PeripheralError::InvalidData(val));
                            }
                        };
                        Ok(ActionResult::Data(d))
                    }
                    Err(e) => Err(e),
                },
                PeripheralAction::Subscribe => match self.subscribe().await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
                PeripheralAction::Unsubscribe => match self.unsubscribe().await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
            }
        }
    }

    impl ManagedBtSensor for MoistureSensor {}
}

pub mod other {
    //! Implementation for another Sensor
    //! This is for testing

    use crate::peripheral_mgr::sensor::{HasPeripheral, Sensor, SensorService};
    use crate::peripheral_mgr::{
        ActionResult, ManagedSensor, PeripheralAction, bt_peripheral::ManagedBtSensor,
        error::PeripheralError,
    };
    use btleplug::api::Peripheral as _;
    use btleplug::platform::Peripheral;
    use uuid::Uuid;

    /// Other Sensor Peripheral
    ///
    /// Represents one individual Other Sensor Peripheral
    /// Implements ManagedSensor, SensorService, Sensor, HasBtPeripheral
    #[derive(Debug, Clone)]
    pub struct OtherSensor {
        /// btleplug Peripheral object as returned from adapter
        pub peripheral: Peripheral,
    }

    impl OtherSensor {}

    impl std::fmt::Display for OtherSensor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Other Sensor ({})", self.peripheral.id())
        }
    }

    impl PartialEq for OtherSensor {
        fn eq(&self, other: &Self) -> bool {
            self.peripheral.address() == other.peripheral.address()
        }
    }

    // Implement SensorService with service Uuids as defined in sensor firmware
    impl SensorService<Uuid> for OtherSensor {
        fn service_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67f4")
        }

        fn read_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67f6")
        }

        fn write_id() -> Uuid {
            uuid::uuid!("d9feb5df-b55f-44ef-b307-63893c5f67f5")
        }
    }

    // Implement HasPeripheral<Peripheral>
    // so we can get HasBtPeripheral (see sensor.rs)
    impl HasPeripheral<Peripheral> for OtherSensor {
        fn new_with_peripheral(p: Peripheral) -> Self {
            Self { peripheral: p }
        }

        fn peripheral(&self) -> Peripheral {
            self.peripheral.clone()
        }
    }

    // Implement ManagedSensor
    impl ManagedSensor for OtherSensor {
        async fn do_action(
            &mut self,
            action: PeripheralAction,
        ) -> Result<ActionResult, PeripheralError>
        where
            Self: Send + Sync + 'static,
        {
            match action {
                PeripheralAction::Write(data) => match self.write(data).await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
                PeripheralAction::Read => match self.read().await {
                    Ok(val) => {
                        // sensor-specific data extraction, as btle api read always returns bytes
                        let d = match <[u8; 4]>::try_from(&val[..4]) {
                            Ok(arr) => u32::from_le_bytes(arr),
                            Err(_) => {
                                return Err(PeripheralError::InvalidData(val));
                            }
                        };
                        Ok(ActionResult::Data(d))
                    }
                    Err(e) => Err(e),
                },
                PeripheralAction::Subscribe => match self.subscribe().await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
                PeripheralAction::Unsubscribe => match self.unsubscribe().await {
                    Ok(_) => Ok(ActionResult::Success),
                    Err(e) => Err(e),
                },
            }
        }
    }

    impl ManagedBtSensor for OtherSensor {}
}
