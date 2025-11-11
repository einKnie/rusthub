pub mod bt;
pub mod hubgui;

use std::error::Error;
use tokio;
use log::{info, warn, debug};
use env_logger;

use std::time::Duration;
use tokio::time;

use bt::ble_mgr::BleMgr;
use std::thread;
use std::sync::mpsc;

// async fn run_ble_mgr(tx) -> Result<(), Box<dyn Error>> {
//     let mut btmgr :Ble = ConnectionMgr::new();
//     btmgr.init().await?;

    
// }

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    info!("Hello world!");

    let mut ble = BleMgr::new();
    ble.init().await?;
    ble.find_sensors().await?;

    for sensor in ble.sensors {
        sensor.blinky().await?;
    }

    // let (tx, rx) = mpsc::channel();
    // let ble_mgr = thread::spawn(move || {
    //     run_ble_mgr();
    // })

    // let _ = hubgui::run_gui();

    // let mut btmgr :Ble = ConnectionMgr::new();
    // btmgr.init().await?;

    // for i in 0..3 {
    //     match btmgr.connect_sensor().await {
    //         Ok(_) =>  {
    //             log::debug!("connected!");
    //             let _ = btmgr.blinky().await;
    //             btmgr.disconnect_sensor().await;
    //             log::debug!("disconnected!");
    //         },
    //         Err(e) => log::debug!("error: {:?}", e)
    //     };
    //     time::sleep(Duration::from_millis(500)).await;
    // };

    // ble_mgr.join().unwrap();

    Ok(())
}
