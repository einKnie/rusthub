//! CmdMgr
//!
//! Command handling with response verification and timeouts
//! Is generic and can manage any cmds that implement the Command trait

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;

/// Default timeout for all commands
const DEFAULT_CMD_TIMEOUT: Duration = Duration::from_secs(30);
/// Interval to check for timed-out commands
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Generic Command trait
/// for a command with an ID
pub trait Command: Clone + Debug + Send + 'static {
    /// Id, for comparison
    type A: PartialEq;
    /// Message
    type B;

    /// Return Id
    fn id(&self) -> Self::A;
    /// Return message
    fn msg(&self) -> Self::B;
}

/// CmdMgr
///
/// Manager for pending Commands
/// where T = The command type to manage
#[derive(Debug, Clone)]
pub struct CmdMgr<T> {
    pending: Arc<Mutex<HashMap<SystemTime, T>>>,
    timeout: Duration,
    cancel_token: CancellationToken,
}

impl<T> Drop for CmdMgr<T> {
    fn drop(&mut self) {
        // pretty nice way to stop thread without any additional channels
        self.cancel_token.cancel();
    }
}

impl<T: Command> Default for CmdMgr<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Implement CmdMgr
///
/// This requires T to satisfy several bounds, encompassed by the Command trait
/// T: Debug + Clone + Send + 'static
/// T::A: PartialEq
impl<T: Command> CmdMgr<T> {
    /// Create a new CommandMgr with default cmd timeout
    pub fn new() -> Self {
        CmdMgr {
            pending: Arc::new(Mutex::<HashMap<SystemTime, T>>::new(HashMap::new())),
            timeout: DEFAULT_CMD_TIMEOUT,
            cancel_token: CancellationToken::new(),
        }
    }

    /// Set an arbitrary command timeout in seconds
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout
    }

    /// Add a pending command
    ///
    /// On add, a timestamp is generated and stored alongside the cmd
    /// also returns the command, so it can be further used by the caller
    pub fn add(&mut self, cmd: T) -> T {
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
    pub fn pop(&mut self, id: T::A) -> Option<T> {
        let mut found: Vec<_> = self
            .pending
            .lock()
            .unwrap()
            .extract_if(|_, x| x.id() == id)
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
    pub fn get_current(&self) -> Vec<T> {
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
