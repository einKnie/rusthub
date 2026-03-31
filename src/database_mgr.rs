//! DatabaseMgr
//!
//! Facilitates database access

pub mod data;
pub mod error;
pub mod message;

/// Database
///
/// Module provides database access
pub mod database {
    use crate::database_mgr::{data::*, error::DatabaseError, message::*};

    use chrono::Local;
    use sqlx::{MySql, Pool};
    use std::time::Duration;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    const DATABASE: &str = "hubdb";
    const SENSORS_TABLE: &str = "sensors";
    const DATA_TABLE: &str = "data";

    /// Run the Database Manager
    ///
    /// Init and run the Mgr; this should be run as a separate thread
    pub async fn mgr_run(
        tx: UnboundedSender<DatabaseResp>,
        rx: UnboundedReceiver<DatabaseCmd>,
    ) -> u32 {
        // init the manager
        let mut mgr = Db::new(tx, rx);
        if mgr.init().await.is_err() {
            panic!("Initialisation failed!");
        }

        log::info!("Database Manager initialized!");
        match mgr.run().await {
            0 => 0,
            _ => panic!("Database Manager failed"),
        }
    }

    /// Database
    pub struct Db {
        tx: UnboundedSender<DatabaseResp>,
        rx: UnboundedReceiver<DatabaseCmd>,

        pool: Option<Pool<MySql>>,
    }

    impl Db {
        /// Generate new Db for database access
        pub fn new(tx: UnboundedSender<DatabaseResp>, rx: UnboundedReceiver<DatabaseCmd>) -> Self {
            Self { tx, rx, pool: None }
        }

        /// Initialisation
        ///
        /// Establish connection to database server and
        /// run basic setup
        pub async fn init(&mut self) -> Result<(), DatabaseError> {
            if self.pool.is_none() {
                dotenvy::dotenv().expect(".env file not found or not readable!");
                let url = &dotenvy::var("DATABASE_URL").expect("DATABASE_URL must be set!");

                self.pool = sqlx::mysql::MySqlPoolOptions::new()
                    .max_connections(5)
                    .acquire_timeout(Duration::from_millis(1000))
                    .connect(url)
                    .await
                    .ok();

                if self.pool.is_none() {
                    log::debug!("failed to connect to database");
                    return Err(DatabaseError::NoConnection);
                }

                // initialize database
                return self.init_database().await;
            }
            log::debug!("Database connection initialized");
            Ok(())
        }

        /// Database Initialisation
        ///
        /// Create the internal database with tables,
        /// in case they do not exist
        async fn init_database(&self) -> Result<(), DatabaseError> {
            // Basic initialisation, so we have the tables we need

            // Create database and use it
            if sqlx::query(format!("CREATE DATABASE IF NOT EXISTS {}", DATABASE).as_str())
                .execute(&self.pool.clone().unwrap())
                .await
                .is_ok()
            {
                if sqlx::query(format!("USE {}", DATABASE).as_str())
                    .execute(&self.pool.clone().unwrap())
                    .await
                    .is_err()
                {
                    log::debug!("failed to use database");
                    return Err(DatabaseError::Failed);
                }
            } else {
                log::debug!("failed to create database");
                return Err(DatabaseError::Failed);
            }

            // Create Sensor table
            if sqlx::query(
                format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                    id INT AUTO_INCREMENT NOT NULL,
                    name TEXT,
                    addr BIGINT UNSIGNED UNIQUE,
                    date_added TIMESTAMP,
                    PRIMARY KEY(id)
                )",
                    SENSORS_TABLE
                )
                .as_str(),
            )
            .execute(&self.pool.clone().unwrap())
            .await
            .is_err()
            {
                log::debug!("failed to create sensor table");
                return Err(DatabaseError::Failed);
            }

            // Create Data table
            if sqlx::query(
                format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                    id INT,
                    ts TIMESTAMP,
                    value INT UNSIGNED,
                    CONSTRAINT fk_id FOREIGN KEY (id) REFERENCES {} (id)
                )",
                    DATA_TABLE, SENSORS_TABLE
                )
                .as_str(),
            )
            .execute(&self.pool.clone().unwrap())
            .await
            .is_err()
            {
                log::debug!("failed to create data table");
                return Err(DatabaseError::Failed);
            }
            Ok(())
        }

        /// Run the database manager
        ///
        /// continuously receive and handle commands from main
        pub async fn run(&mut self) -> u32 {
            if self.pool.is_none() {
                log::error!("No connection to database server");
                return 1;
            }

            log::debug!("starting database thread");

            loop {
                tokio::select! {
                    // rx.recv()returns None when the channel is closed,
                    // so use that to stop the thread if the ui for some reason exits
                    result = self.rx.recv() => {
                        if let Some(msg) = result {
                            match msg.msg {
                                DBCmd::StopThread => {
                                    log::debug!("Received thread stop command");
                                    break;
                                },
                                ref _other => {
                                    self.handle_cmd(msg).await;
                                }
                            }
                        } else {
                            // channel was closed, better stop
                            log::info!("channel to main is closed");
                            break;
                        }
                    }
                }
            }

            // gracefully close connection pool to database server
            log::debug!("Gracefully shutting down connection pool to database server");
            if let Some(pool) = self.pool.take() {
                pool.close().await;
            };
            0
        }

        /// Handle commands
        async fn handle_cmd(&mut self, cmd: DatabaseCmd) {
            let task_tx = self.tx.clone();

            match cmd.msg {
                DBCmd::Ping => {
                    log::debug!("DB: Ping received");
                    // #todo do something
                    task_tx
                        .send(DatabaseResp::Response(cmd.id, DBResp::Success))
                        .unwrap();
                }
                DBCmd::AddEntry(id, ts, value) => {
                    log::debug!("adding entry to database for sensor: {id:?} ({ts:?}, {value:?})");
                    match self.add_datapoint(id, ts, value).await {
                        Err(_) => {
                            log::warn!("Failed to add entry to database!");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            log::debug!("added entry to database");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Success))
                                .unwrap();
                        }
                    }
                }
                DBCmd::AddSensor(addr, name) => {
                    log::debug!("adding new sensor: {name:?}");

                    if let Ok((server_id, server_name)) = self.get_id_and_name(addr).await {
                        // we know that the sensor is in the database now
                        log::warn!("sensor is already in database!");
                        task_tx
                            .send(DatabaseResp::Response(
                                cmd.id,
                                DBResp::SensorKnown(addr, server_name, server_id),
                            ))
                            .unwrap();
                        return;
                    }

                    match self.add_sensor(addr, name).await {
                        Err(_) => {
                            log::warn!("Failed to add sensor to database!");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            log::debug!("added sensor to database");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::SensorAdded(addr)))
                                .unwrap();
                        }
                    }
                }
                DBCmd::UpdateSensor(id, name) => {
                    log::debug!("Updating sensor with new name");
                    match self.update_sensor(id, name).await {
                        Err(_) => {
                            log::warn!("Failed to update sensor data!");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            log::debug!("updated sensor data");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Success))
                                .unwrap();
                        }
                    }
                }
                DBCmd::DeleteSensor(id) => {
                    log::debug!("DELETING SENSOR from database: {id:?}");
                    match self.delete_sensor(id).await {
                        Err(_) => {
                            log::warn!("Failed to remove sensor from database!");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                .unwrap();
                        }
                        Ok(_) => {
                            log::debug!("removed sensor from database");
                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::SensorDeleted(id)))
                                .unwrap();
                        }
                    }
                }
                DBCmd::Get(query) => {
                    log::debug!("database query received");

                    match query {
                        DatabaseQuery::SensorID(addr) => {
                            log::debug!("SensorId query for {addr:?}");
                            let id = match self.get_id(addr).await {
                                Ok(id) => {
                                    log::debug!("got sensor id: {id:?}");
                                    id
                                }
                                Err(_) => {
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                        .unwrap();
                                    return;
                                }
                            };

                            task_tx
                                .send(DatabaseResp::Response(cmd.id, DBResp::SensorId(addr, id)))
                                .unwrap();
                        }
                        DatabaseQuery::Latest(id) => {
                            log::debug!("get latest value for sensor {id:?}")
                        }
                        DatabaseQuery::TsBefore(id, ts) => {
                            log::debug!("requested entries for {id:?} before {ts:?}");
                            match self.get_datapoints(id, None, Some(ts)).await {
                                Ok(res) => {
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Data(res)))
                                        .unwrap();
                                }
                                Err(e) => {
                                    log::debug!("failed to fetch TsBefore: {e}");
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                        .unwrap();
                                }
                            }
                        }
                        DatabaseQuery::TsAfter(id, ts) => {
                            log::debug!("requested entries for {id:?} after {ts:?}");
                            match self.get_datapoints(id, Some(ts), None).await {
                                Ok(res) => {
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Data(res)))
                                        .unwrap();
                                }
                                Err(e) => {
                                    log::debug!("failed to fetch TsAfter: {e}");
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                        .unwrap();
                                }
                            }
                        }
                        DatabaseQuery::TsDuration(id, ts, ts2) => {
                            log::debug!("requested entries for {id:?} between {ts:?} and {ts2:?}");
                            match self.get_datapoints(id, Some(ts), Some(ts2)).await {
                                Ok(res) => {
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Data(res)))
                                        .unwrap();
                                }
                                Err(e) => {
                                    log::debug!("failed to fetch TsDuration: {e}");
                                    task_tx
                                        .send(DatabaseResp::Response(cmd.id, DBResp::Failed))
                                        .unwrap();
                                }
                            }
                        }
                    }
                }
                DBCmd::StopThread => (), // handled elsewhere
            }
        }

        /// Add new named Sensor
        async fn add_sensor(&mut self, addr: u64, name: String) -> Result<(), DatabaseError> {
            let pool = self.pool.clone().unwrap();

            match sqlx::query(
                format!(
                    "INSERT INTO {} (name, addr, date_added) VALUES (?, ?, ?)",
                    SENSORS_TABLE
                )
                .as_str(),
            )
            .bind(name)
            .bind(addr)
            .bind(Local::now())
            .execute(&pool)
            .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    log::debug!("failed to add sensor: {e:?}");

                    match e.into_database_error() {
                        Some(dberr) => match dberr.kind() {
                            sqlx::error::ErrorKind::UniqueViolation => {
                                log::debug!("Sensor already in database!");
                                Err(DatabaseError::Duplicate)
                            }
                            _ => Err(DatabaseError::GeneralError(Box::new(dberr))),
                        },
                        None => Err(DatabaseError::Failed),
                    }
                }
            }
        }

        /// Update sensor data (actually just name)
        async fn update_sensor(&mut self, id: i32, name: String) -> Result<(), DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor addr
            let addr = match self.get_addr(id).await {
                Ok(a) => {
                    log::debug!("got sensor id: {id:?}");
                    a
                }
                Err(e) => return Err(e),
            };
            // update the db entry
            match sqlx::query(
                format!(
                    "INSERT INTO {} (name, addr) VALUES (?, ?)
                    ON DUPLICATE KEY UPDATE name=?",
                    SENSORS_TABLE
                )
                .as_str(),
            )
            .bind(&name)
            .bind(addr)
            .bind(&name)
            .execute(&pool)
            .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    log::debug!("failed to update sensor data: {e:?}");
                    Err(DatabaseError::GeneralError(Box::new(e)))
                }
            }
        }

        /// Delete a sensor (and all associated data) from database
        async fn delete_sensor(&mut self, id: i32) -> Result<(), DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // first, delete associated data entries
            match sqlx::query(format!("DELETE FROM {} WHERE id = ?", DATA_TABLE).as_str())
                .bind(id)
                .execute(&pool)
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    log::debug!("failed to delete sensor data: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            }

            match sqlx::query(format!("DELETE FROM {} WHERE id = ?", SENSORS_TABLE).as_str())
                .bind(id)
                .execute(&pool)
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    log::debug!("failed to delete sensor: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            }
            Ok(())
        }

        /// Add datapoint for sensor
        async fn add_datapoint(
            &mut self,
            id: i32,
            ts: chrono::DateTime<Local>,
            val: u32,
        ) -> Result<(), DatabaseError> {
            let pool = self.pool.clone().unwrap();

            match sqlx::query(
                format!(
                    "INSERT INTO {}
                    SET id = ?, ts = ?, value = ?",
                    DATA_TABLE
                )
                .as_str(),
            )
            .bind(id)
            .bind(ts)
            .bind(val)
            .execute(&pool)
            .await
            {
                Ok(_) => (),
                Err(e) => {
                    log::debug!("failed to insert into data table: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            }
            Ok(())
        }

        /// Get datapoints from database
        async fn get_datapoints(
            &mut self,
            id: i32,
            first: Option<chrono::DateTime<Local>>,
            last: Option<chrono::DateTime<Local>>,
        ) -> Result<Vec<DatabaseEntry>, DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor name
            let sensor_name = match self.get_name(id).await {
                Ok(id) => {
                    log::debug!("got sensor id: {id:?}");
                    id
                }
                Err(e) => return Err(e),
            };

            let res = match (first, last) {
                (None, None) => {
                    // @todo maybe use this case to request _all_ data for a sensor?
                    // currently doing this by requesting all data before _now_, but this might be cleaner
                    Err(DatabaseError::Failed)
                }
                (Some(start), None) => {
                    // get entries AFTER start
                    sqlx::query_as(
                        format!(
                            "SELECT ts, value FROM {} WHERE id = ? AND ts > ?",
                            DATA_TABLE
                        )
                        .as_str(),
                    )
                    .bind(id)
                    .bind(start)
                    .fetch_all(&pool)
                    .await
                    .map_err(|_| DatabaseError::Failed)
                }
                (None, Some(end)) => {
                    // get entried BEFORE end
                    sqlx::query_as(
                        format!(
                            "SELECT ts, value FROM {} WHERE id = ? AND ts < ?",
                            DATA_TABLE
                        )
                        .as_str(),
                    )
                    .bind(id)
                    .bind(end)
                    .fetch_all(&pool)
                    .await
                    .map_err(|_| DatabaseError::Failed)
                }
                (Some(start), Some(end)) => {
                    // get entried BETWEEN start and end
                    sqlx::query_as(
                        format!(
                            "SELECT ts, value FROM {} WHERE id = ? AND ts < ? AND ts > ?",
                            DATA_TABLE
                        )
                        .as_str(),
                    )
                    .bind(id)
                    .bind(start)
                    .bind(end)
                    .fetch_all(&pool)
                    .await
                    .map_err(|_| DatabaseError::Failed)
                }
            };

            // map received data to DatabaseEntries
            let result: Vec<DatabaseEntry> = match res {
                Ok(datapoints) => {
                    log::debug!("got data");
                    datapoints
                        .iter()
                        .map(|(x, y)| DatabaseEntry {
                            sensor_id: id,
                            sensor_name: sensor_name.clone(),
                            ts: *x,
                            value: *y,
                        })
                        .collect()
                }
                Err(e) => {
                    log::debug!("failed to fetch datapoints from database");
                    return Err(e);
                }
            };
            Ok(result)
        }

        /// Get sensor ID
        async fn get_id(&mut self, addr: u64) -> Result<i32, DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor id
            let res: (i32, chrono::DateTime<Local>) = match sqlx::query_as(
                format!(
                    "SELECT id, date_added FROM {} WHERE addr = ?",
                    SENSORS_TABLE
                )
                .as_str(),
            )
            .bind(addr)
            .fetch_one(&pool)
            .await
            {
                Ok(id) => {
                    log::debug!("got sensor ID: {id:?}");
                    id
                }
                Err(e) => {
                    log::debug!("sensor with addr {addr:?} not found: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            };
            Ok(res.0)
        }

        /// Get sensor name
        async fn get_name(&mut self, id: i32) -> Result<String, DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor id
            let res: (u64, String) = match sqlx::query_as(
                format!("SELECT addr, name FROM {} WHERE id = ?", SENSORS_TABLE).as_str(),
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            {
                Ok((addr, name)) => {
                    log::debug!("got sensor name: {name:?}");
                    (addr, name)
                }
                Err(e) => {
                    log::debug!("sensor with id {id:?} not found: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            };
            Ok(res.1)
        }

        /// Get sensor address
        async fn get_addr(&mut self, id: i32) -> Result<u64, DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor id
            let res: (u64, String) = match sqlx::query_as(
                format!("SELECT addr, name FROM {} WHERE id = ?", SENSORS_TABLE).as_str(),
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            {
                Ok((addr, name)) => {
                    log::debug!("got sensor name: {name:?}");
                    (addr, name)
                }
                Err(e) => {
                    log::debug!("sensor with id {id:?} not found: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            };
            Ok(res.0)
        }

        /// Get sensor name
        async fn get_id_and_name(&mut self, addr: u64) -> Result<(i32, String), DatabaseError> {
            let pool = self.pool.clone().unwrap();

            // find sensor id
            let res: (i32, String) = match sqlx::query_as(
                format!("SELECT id, name FROM {} WHERE addr = ?", SENSORS_TABLE).as_str(),
            )
            .bind(addr)
            .fetch_one(&pool)
            .await
            {
                Ok(res) => {
                    log::debug!("got sensor ID: {res:?}");
                    res
                }
                Err(e) => {
                    log::debug!("sensor with addr {addr:?} not found: {e:?}");
                    return Err(DatabaseError::GeneralError(Box::new(e)));
                }
            };
            Ok(res)
        }
    }
}
