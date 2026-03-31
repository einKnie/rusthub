//! Message types for DatabaseMgr
//!
use crate::cmdmgr::Command;
use crate::database_mgr::data::DatabaseEntry;
use chrono::{DateTime, Local};
use std::sync::atomic::{AtomicU16, Ordering};

/// Database Resp
///
/// Sent from DatabaseMgr in Response to a DatabaseCmd
#[derive(Debug, Clone)]
pub enum DBResp {
    /// Retrieved DatabaseEntries from DB
    Data(Vec<DatabaseEntry>),
    /// Sensor Id and addr of a sensor
    SensorId(u64, i32),
    /// Report a known sensor with addr, name and id
    SensorKnown(u64, String, i32),
    /// Sensor with addr was added to DB
    SensorAdded(u64),
    /// Sensor with id was deleted from DB
    SensorDeleted(i32),
    /// Generic Success
    Success,
    /// Generic Failure
    Failed,
}

/// DatabaseResp
///
/// Sent from Database mgr to caller, contains the cmd id and response
#[derive(Debug, Clone)]
pub enum DatabaseResp {
    /// Response with cmd id
    Response(u16, DBResp),
}

/// Database Command
///
/// Sent from main to the DatabaseMgr
/// To be used for thread control as well as database commands)
#[derive(Debug, Clone)]
pub enum DBCmd {
    /// Ping database mgr
    Ping,
    /// Add data entry (id, date, value)
    AddEntry(i32, DateTime<Local>, u32),
    /// Add sensor (addr, name)
    AddSensor(u64, String),
    /// update sensor name (id, new_name)
    UpdateSensor(i32, String),
    /// Delete sensor form db (id)
    DeleteSensor(i32),
    /// data query
    Get(DatabaseQuery),
    /// Stop the database_mgr thread
    StopThread,
}

/// DatabaseQuery
///
/// Represents a specific Get request to the DatabaseMgr
#[derive(Debug, Clone)]
pub enum DatabaseQuery {
    /// Request sensor id from sensor address
    SensorID(u64),
    /// Request latest datapoint for sensor
    Latest(i32),
    /// Request datapoints for sensor before a given date
    TsBefore(i32, DateTime<Local>),
    /// Request datapoints for sensor after a given date
    TsAfter(i32, DateTime<Local>),
    /// Request datapoints for sensor in a given time range
    TsDuration(i32, DateTime<Local>, DateTime<Local>),
}
/// DatabaseCmd
///
/// Sent from hub to Database, contains the cmd id and the command
#[derive(Debug, Clone)]
pub struct DatabaseCmd {
    /// command id
    pub id: u16,
    /// command
    pub msg: DBCmd,
}

impl DatabaseCmd {
    /// Create new DatabaseCmd from the given DBCmd
    pub fn new(msg: DBCmd) -> DatabaseCmd {
        static CNT: AtomicU16 = AtomicU16::new(0);

        DatabaseCmd {
            id: CNT.fetch_add(1, Ordering::Relaxed),
            msg,
        }
    }

    /// Validate a response
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
