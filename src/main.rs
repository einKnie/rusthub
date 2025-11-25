
// pub mod hubgui;
pub mod peripheral_mgr;

use std::error::Error;
use crossbeam_channel::{bounded, Sender, Receiver};
use peripheral_mgr::peripheral::{PeripheralMgr, HubMsg, EventMsg};


/// Run the Peripheral Manager
/// Init and run the Mgr; this should be run as a separate thread
async fn mgr_run(tx: Sender<EventMsg>, rx: Receiver<HubMsg>) -> u32 {
    // init the manager
    let mut mgr = PeripheralMgr::new();
    if mgr.init().await.is_ok() {
        println!("Peripheral Manager initialized!");
        mgr.run(tx, rx).await;
    }
    0
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    log::info!("Hello world!");

    // need to create channels for both directions individually, otherwise we risk receiving our own message
    let (main_tx, thread_rx) = bounded(4);
    let (thread_tx, main_rx) = bounded(4);

    let mut mgr = PeripheralMgr::new();
    mgr.init().await?;

    let mgr_handle = tokio::spawn(mgr_run(thread_tx, thread_rx));

    loop {
        match main_rx.recv() {
            Ok(val) => {
                dbg!(&val);
                match val {
                    EventMsg::DeviceDiscovered(addr) => {
                        log::info!("Device Discovered: {addr:?}");
                        main_tx.send(HubMsg::Connect(addr)).unwrap();
                    },
                    EventMsg::DeviceConnected(addr) => {
                        log::info!("Device Connected: {addr:?}");
                        main_tx.send(HubMsg::Blink(addr)).unwrap();
                    },
                    EventMsg::DeviceDisconnected(addr) => {
                        // for testing; let's stop the thread if a device disconnects
                        log::info!("Device Disconnected: {addr:?}");
                        break;
                    },
                    EventMsg::ServiceDiscovered(addr) => log::info!("Found Moisture LED service: {addr:?}"), // does nothing: i think b/c the sensor does not advertise; i guess i have to add that lol
                };
            },
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }

    // send shut-down command to thread
    main_tx.send(HubMsg::StopThread).unwrap();
    if mgr_handle.await.is_err() {
        log::warn!("Peripheral Manager thread ended unsuccessfully");
    } // make sure we wait until thread has stopped

    log::info!("Done!");
    Ok(())
}
