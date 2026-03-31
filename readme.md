# Rusthub

Simple GUI to interact with specific BLE devices.

## Usage

Note: This only makes sense in combination with the Sensor peripherals.

### Required Setup

The UI expects a mariadb server running on the pc. initial config:
1. `$ mariadb-install-db --user=mysql --basedir=/usr --datadir=/var/lib/mysql`
2. `# systemctl start mariadb`
3. `# mariadb-secure-installation` is interactive just use defaults as told

The above should only be necessary once. Just make sure the service is running on subsequent boots (`systemctl mariadb enable`) and all should be fine.

server config must be done through cli:
`# mariadb`
and in there
create a user:
`MariaDB [(none)]> CREATE USER '$user'@'localhost' IDENTIFIED BY '$password$';`

create database:
`MariaDB [(none)]> CREATE DATABASE $database_name;`

allow user access to database:
`MariaDB [(none)]> GRANT ALL PRIVILEGES ON $database_name.* TO '$user'@'localhost';`

After this is all done, create a `.env` file in the repo, containing:
`DATABASE_URL="mysql://$user:$password@localhost/$database_name"`

## Development

### database



### logging

`export RUST_LOG=warn,rusthub=debug` to set the loglevel to debug only for my crate.


## todos

for me: when looking for functionality, check crates according to these resources:
- https://blessed.rs/crates
- https://github.com/rust-unofficial/awesome-rust (though this is less crates and more fully-fledged applications)
- https://lib.rs/ (everything, but thus obvs not manually curated like belssed)

apparently (shows how much i know, lol), a mysql server must run so my rust program can connec tot the db
how? see: https://wiki.archlinux.org/title/MariaDB
`mariadb-install-db --user=mysql --basedir=/usr --datadir=/var/lib/mysql` and then start the mariadb.service

### general

- [ ] improve logging
    - not too much, sensible loglevels
- [ ] ~~data storage~~
    - [ ] ~~define formatting~~
        - json?
        - store address (for id) and given name (for persistence)
        - when should path be checked?
    - [ ] how to handle the data long-term?
        - makes no sense to keep everything in memory, this could get massive after years of running
        - database might be better after all
            - have all data in database (nothing in memory, maybe even?)
                and load data when it is needed (e.g. for displaying statistics)
- [ ] database
    - [ ] impl database storage and loading in own module
        - access data via hwaddr (unique key)
        - on connection: check if addr in db and load stored name
        - on disconnect: save current name in db for addr (or on name change, tbd)
    - [ ] use THIS: https://lib.rs/crates/sqlx
        - also to read: https://kerkour.com/rust-postgres-everything


### UI

- [x] implement notification handling for sensor characteristic
    - ~~kinda but not really~~
    - ~~some notifications are received, most are not~~
    - some thread (not directly mine) panics sometimes
    - update: all issues fixed by running separate tasks and thus not taking and dropping handle all time (see PeripheralMgr todos)
- [x] make button image work OR make button show name/value
    - changed interface to have multiple buttons per sensor
- [x] make ui updatable on changes
- [ ] think of a good color scheme (dark and light themes)
- [ ] can i do a running statusbar?
- [x] let user change sensor name
- [ ] PERSISTENCE (remember known sensors)
- [x] add command timeouts: if a command is not resolved after x amount of time, remove from pending list
- [ ] maybe add some sort of guistate in an Ary<Mutex> so i could e.g. have a popup when a command failed or timed out (but since the gui is immediate, i need some sort of flag (command_failed -> if true, show popup, when popup Ok button clicked -> set command_failed=false, something like that))
- [ ] on startup, check for sensors in db
    - if found, maybe also add a sensor entry but show as disconnected (until it is)
        - would be nice long-term, to e.g. be able to remove broken/missing sensors from db
        - also nice to see if any known sensors are offline
- [x] make sensor view scrollable
    - if windows size changes (btw super tiny in floating, find a way to change that)
    - or if many active sensors
- [ ] add button to unselect all sensors
    - always show but make unclickable when none are selected

### Peripheral Mgr

- [x] first automatic connection to peripheral always fails, why?
    - solved: on discover, peripheral was not added ot the list and when hub requested connect, the peripheral was therefore not found; fixed now
- [x] sensor data arrives wrong! (endianness?) - not anymore
- [ ] identify peripheral by id instead of hwAddr
- [ ] (maybe) split HubMsg into two enums (flow ctrl & commands)
- [x] improve errors
- [x] try to find out how to improve ble notification handling (some bluez thread (not my sources) keeps panicking from time to time)
    - update: this also happens when not subscribed (and when not connected to anything), so likely unrelated?
    - more update: might be circumstance, but it seems to fail way more with two sensors connected
    - FIXED with notification threads
- [x] event on device disconnect is not always received after disconnecting
    - actually event is received, but sensor does not always show the correct name -.- ("Arduino" instead of "MoistureSensor")
- [x] make more async: msg thread should receive msgs but then spawn threads to handle requested actions
- [x] AT LEAST things like subscribing to notification should spawn a new thread to only handle that notification stream (should also minimize those panics, since they happen on drop of the messagestream in bluez-async -> if we don't drop it all the time, we can't have panics all the time *insert meme with guy tapping on forehead*)
    - basically, thread is spawned on susbscribe, killed on unsubscribe, listens on notificaiton stream all the time and sends received data to peripheral-mgr main thread
    - YES! works very well! ;)
