// Database Messages
use crate::cmdmgr::Command;
use chrono::{DateTime, Local};
use std::sync::atomic::{AtomicU16, Ordering};

#[derive(Debug, Clone)]
pub struct DatabaseEntry {
    pub sensor_id: String,
    // @todo add hwid
    pub ts: DateTime<Local>,
    pub value: u32,
}

#[derive(Debug, Clone)]
pub enum DatabaseQuery {
    SensorID(u64),
    Latest(u64),
    TsBefore(u64, DateTime<Local>),
    TsAfter(u64, DateTime<Local>),
    TsDuration(u64, DateTime<Local>, DateTime<Local>),
}

/// Database Resp
///
/// Sent from DatabaseMgr in Response to a DatabaseCmd
#[derive(Debug, Clone)]
pub enum DBResp {
    Data(Vec<DatabaseEntry>),
    SensorKnown(u64, String),
    SensorAdded(u64),
    SensorDeleted(u64),
    Success,
    Failed,
}

#[derive(Debug, Clone)]

pub enum DatabaseResp {
    Response(u16, DBResp),
}

/// Database Command
///
/// Sent from main to the DatabaseMgr
/// To be used for thread control as well as database commands)
#[derive(Debug, Clone)]
pub enum DBCmd {
    Ping,
    AddEntry(u64, DateTime<Local>, u32),
    AddSensor(u64, String),
    UpdateSensor(u64, String),
    DeleteSensor(u64),
    Get(DatabaseQuery),
    StopThread,
}

#[derive(Debug, Clone)]
pub struct DatabaseCmd {
    pub id: u16,
    pub msg: DBCmd,
}

impl DatabaseCmd {
    pub fn new(msg: DBCmd) -> DatabaseCmd {
        static CNT: AtomicU16 = AtomicU16::new(0);

        DatabaseCmd {
            id: CNT.fetch_add(1, Ordering::Relaxed),
            msg,
        }
    }

    pub fn validate_response(&self, _resp: &DBResp) -> bool {
        // check if HubResp is valid depending on HubCmd
        // match (self.msg.clone(), resp) {
        //     // only ReadFrom may return ReadData or Failed, all others return Success or Failed
        //     (_, DBResp::Success | DBResp::Failed) => (),
        //     (_, _) => return false,
        // };
        true
    }
}

impl Command for DatabaseCmd {
    type A = u16;
    type B = DBCmd;

    fn id(&self) -> u16 {
        self.id
    }

    fn msg(&self) -> DBCmd {
        self.msg.clone()
    }
}
