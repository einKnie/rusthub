use btleplug::api::BDAddr;
use std::sync::atomic::{AtomicU16, Ordering};

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
#[derive(Debug, Clone, Copy)]
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
            msg,
        }
    }

    pub fn validate_response(&self, resp: &HubResp) -> bool {
        // check if HubResp is valid depending on HubCmd
        match (self.msg, resp) {
            // only ReadFrom may return ReadData or Failed, all others return Success or Failed
            (HubCmd::ReadFrom(_), HubResp::ReadData(_, _) | HubResp::Failed) => (),
            (_, HubResp::Success | HubResp::Failed) => (),
            (_, _) => return false,
        };
        true
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;

/// Default timeout for all commands
const DEFAULT_CMD_TIMEOUT: Duration = Duration::from_secs(30);
/// Interval to check for timed-out commands
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// CmdMgr
///
/// Manager for pending  PeripheralCmds
#[derive(Debug, Clone)]
pub struct CmdMgr {
    pending: Arc<Mutex<HashMap<SystemTime, PeripheralCmd>>>,
    timeout: Duration,
    cancel_token: CancellationToken,
}

impl Drop for CmdMgr {
    fn drop(&mut self) {
        // pretty nice way to stop thread without any additional channels
        self.cancel_token.cancel();
    }
}

impl Default for CmdMgr {
    fn default() -> Self {
        Self::new()
    }
}

impl CmdMgr {
    /// Create a new CommandMgr with default cmd timeout (10sec)
    pub fn new() -> Self {
        CmdMgr {
            pending: Arc::new(Mutex::<HashMap<SystemTime, PeripheralCmd>>::new(
                HashMap::new(),
            )),
            timeout: DEFAULT_CMD_TIMEOUT,
            cancel_token: CancellationToken::new(),
        }
    }

    /// Set an arbitrary command timeout
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout
    }

    /// Add a pending command
    ///
    /// On add, a timestamp is generated and stored alongside the cmd
    pub fn add(&mut self, cmd: PeripheralCmd) -> PeripheralCmd {
        self.pending
            .lock()
            .unwrap()
            .insert(SystemTime::now(), cmd.clone());
        cmd
    }

    /// Remove and return a pending command by id
    ///
    /// If a pending command with the given id is found,
    /// remove it from the pending list and return
    pub fn pop(&mut self, id: u16) -> Option<PeripheralCmd> {
        let mut found: Vec<_> = self
            .pending
            .lock()
            .unwrap()
            .extract_if(|_, x| x.id == id)
            .collect::<HashMap<_, _>>()
            .into_values()
            .collect();
        found.pop()
    }

    /// Check if pending list is empty
    pub fn is_empty(&self) -> bool {
        self.pending.lock().unwrap().len() == 0
    }

    /// Return a copy of all currently pending commands
    pub fn get_current(&self) -> Vec<PeripheralCmd> {
        self.pending.lock().unwrap().clone().into_values().collect()
    }

    /// Start CommandMgr thread
    ///
    /// Check pending commands in `CHECK_INTERVAL` interevals
    /// and remove any messages that have exceeded the timeout
    pub fn start_handler(&self) {
        let token = self.cancel_token.clone();
        let p = Arc::clone(&self.pending);
        let t = self.timeout;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(CHECK_INTERVAL) => {
                        let removed: HashMap<_, _> = p
                            .lock()
                            .unwrap()
                            .extract_if(|timestamp, _| match timestamp.elapsed() {
                                Ok(delta) if delta > t => true,
                                Ok(_) => false,
                                Err(e) => {
                                    // @todo is this an issue at summertime/wintertime switch?
                                    log::warn!(
                                        "Invalid timestamp detected: time difference: {:?}",
                                        e.duration()
                                    );
                                    true
                                }
                            })
                            .collect::<HashMap<_, _>>();
                        if !removed.is_empty() {
                            log::debug!("Removed timed out commands from pending list:");
                            dbg!(removed);
                        }
                    }
                }
            }
            log::debug!("CommandMgr thread stopping");
        });
    }
}
