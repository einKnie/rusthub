use std::sync::atomic::{AtomicU16, Ordering};
use btleplug::api::BDAddr;

/// Hub Resp
///
/// Sent from PeripheralMgr in Response to a PeripheralCmd
#[derive(Debug, Clone)]
pub enum HubResp {
    ReadData(BDAddr, u32),
    Success,
    Failed,
}

/// Hub Event
///
/// Sent from PeripheralMgr on bluetooth event
#[derive(Debug, Clone)]
pub enum HubEvent {
    DeviceDiscovered(BDAddr),
    DeviceConnected(BDAddr),
    DeviceDisconnected(BDAddr),
    NewData(BDAddr, u32),
}

#[derive(Debug, Clone)]
pub enum PeripheralMsg {
    Event(HubEvent),
    Response(u16, HubResp),
}

/// Hub Message
///
/// Sent from main to the PeripheralMgr
/// To be used for thread control as well as peripheral commands
/// @todo this should be split in two enums, per usecase
/// since I will want to reuse the thread control msgs also for other threads
/// @todo find out how i can check for two types in one stream (box? trait? can en enum impl a trait, even?)
#[derive(Debug, Clone)]
pub enum HubCmd {
    Ping,
    FindSensors,
    Connect(BDAddr),
    ConnectAll,
    Disconnect(BDAddr),
    Subscribe(BDAddr),
    Unsubscribe(BDAddr),
    ReadFrom(BDAddr),
    Blink(BDAddr),
    BlinkAll,
    StopThread,
}


///////////////////////////////////////////////////////////////
/// nice would be to have handling of pending messages in here as well, fully encapsulated
/// does this make sense?
/// like, keep a vec of pending IDs here and have cmd.handle() check if the id is pending and remove if it is or return err if not?


#[derive(Debug, Clone)]
pub struct PeripheralCmd {
    pub id: u16,
    pub msg: HubCmd,
}

impl PeripheralCmd {
    pub fn new(msg: HubCmd) -> PeripheralCmd {
        static CNT: AtomicU16 = AtomicU16::new(1); // we start at 1 to keep 0 for eventmsg (but: what about rollover?)

        PeripheralCmd {
            id: CNT.fetch_add(1, Ordering::Relaxed),
            msg
        }
    }

    pub fn validate_response(&self, resp: &HubResp) -> bool {
        // check if HubResp is valid depending on HubCmd
        match (self.msg.clone(), resp) {
            // only ReadFrom may return ReadData or Failed, all others return Success or Failed
            (HubCmd::ReadFrom(_), HubResp::ReadData(_,_) | HubResp::Failed) => (),
            (_,HubResp::Success | HubResp::Failed) => (),
            (_,_) => return false,
        };
        true
    }
}
