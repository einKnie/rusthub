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
- [x] database
    - [ ] impl database storage and loading in own module
        - access data via hwaddr (unique key)
        - on connection: check if addr in db and load stored name
        - on disconnect: save current name in db for addr (or on name change, tbd)
    - [x] use THIS: https://lib.rs/crates/sqlx
        - also to read: https://kerkour.com/rust-postgres-everything

#### sensor identification

    - would like a good way to identify sensors by something other than their hw addr
    - unique id given to sensor when first ...what? found/added to database?
    - in that scenario, peripheral-mgr would need to handle the translation layer from hwaddr to unique id internally

what do we have?
when sensor is detected by bt adapter -> give unique id?
but db must know if sensor is already known, otherwise we cannot recognize it. so we must have some unique value
- if i just use BDAddr converted to u64 as the unique id overall?
    - for sure easier (and more generic) to use u64 than BDAddr
    - but in that case i need a fn to convert back to BDAddr first
        - i have that now, but: not sure if i want it
        - since the db *must* known some actually uniquer identifier, the addr converted to u64 is good for db identification.
- but i cannot store a u64 in the (slint) ui struct -.-
    - so i would e.g. have to add sensor to db with addr as u64 but use e.g. db_id for identification program-wide.
        - this requires that all sensors are in database
            - which is actually a reasonable requirement (sensor is useless (in terms of this app) without the database)
        - but in that case peripheralmgr must have a way to translate back to addr, which defeats the point. peripheral and db mgrs should be independent from each other



### Database

- [ ] fetch list of known sensors on startup, give to gui, but unconnected
- [ ] when sensor is deleted from database, it should be removed from gui as well
    - [ ] make "delete from db" button only available when sensor is disconnected
    - [ ] removing from db should also trigger removing from periph?

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
    - on startup, check for sensors in db
        - if found, maybe also add a sensor entry but show as disconnected (until it is)
            - would be nice long-term, to e.g. be able to remove broken/missing sensors from db
            - also nice to see if any known sensors are offline
- [x] add command timeouts: if a command is not resolved after x amount of time, remove from pending list
- [ ] maybe add some sort of guistate in an Arc<Mutex> so i could e.g. have a popup when a command failed or timed out (but since the gui is immediate, i need some sort of flag (command_failed -> if true, show popup, when popup Ok button clicked -> set command_failed=false, something like that))

- [x] make sensor view scrollable
    - if windows size changes (btw super tiny in floating, find a way to change that)
    - or if many active sensors
- [x] add button to unselect all sensors
    - always show but make unclickable when none are selected
- [ ] display charts in gui
    - looks to be more complex that i thought
    - i want to display a generated chart in a window, and the window should stay open until closed
    - ideally, maybe have imgs per sensor and redraw regularly with current data?

#### Ui retained mode

- [ ] try retained-mode gui with [iced](https://docs.rs/iced/latest/iced/index.html)
    - [ ] BIG TODOS:
        - [ ] find a way to handle the threads (in a process sense). currently works but does not await thread handles. any attempt so far failed
        - [ ] disconnect ui logic from thread handling somehow


- [x] new try: slint
    - looks like i can run the thread handling in a separate thread but call callbacks of the ui from there, thread-safe, see: https://docs.slint.dev/latest/docs/rust/slint/fn.invoke_from_event_loop
    - worth a try, but setting everything up again is so much tedious work -.-
    - ok, the main event loop (managers and now also the "ui_mgr") are running in separate threads and do their thing, while the ui sits there. so far, so good. now i need to see how i can affect the ui from the mgr
    - and also, actually potential major issue -> can i affect the mgrs from the ui this way -> actually, i think i need the whole bidirectional channels for the ui_mgr now, like the others..

##### slint ui

basic functionality is provided, though minimal.
todos:
- [x] handle exit. a bit convoluted but it works
    - interval timer runs and checks if all threads are stopped; when yes a clean_exit signal is sent to main ui
    - if main ui clean_exit callback is called, we check if exit was in progress, if yes exit of no, warning
    - on exit button clicked: ui-mgr thread receives stop cmd, which then stops the other mgrs. also starts a backup timer to force kill if not yet stopped after 10 sec
    - [x] todo: handle kill signals
        - have added a callback handler when the main window is closed
- [ ] handle ui, basically make an actual user interface
    - basic interface stands, though unstyled
    - issues with scaling layout since i can't generate a grid dynamically (via for loop) #todo
- [ ] ui feedback on pending events (i.e. connecting etc..)
    - actually two topics: general 'in progress' and sensor-specific in-progress
    - sensor-specific, meaning the connect button should not be clickable while sensor is already connecting
    - general, meaning show some sort of progress icon while any connecting action (for any sensor) is in pending (just to give some user feddback, especially at startup)
    - on the other hand, since i have pendng actions per button (SpinnerButton) maybe i can somehow map this
- [x] generate sensor_id from atomic counter
    - but this actually maps back to general todos: sensor identification




### Peripheral Mgr

- [x] first automatic connection to peripheral always fails, why?
    - solved: on discover, peripheral was not added ot the list and when hub requested connect, the peripheral was therefore not found; fixed now
- [x] sensor data arrives wrong! (endianness?) - not anymore
- [ ] identify peripheral by id instead of hwAddr
    - [ ] move sensor_id handling to peripheral_mgr and communicate with the outside only via id
        - basically, when peripheral-mgr finds a new peripheral, a unique id should be given and provided instead of the address
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
- [ ] think about preparing the peripheralmgr to handle different types of sensors
    - ideally generic, so i can uilize the same base
    - best option would be to have mgr fully generic
        and have a trait for peripheral/sensor a la Command/CmdMgr
    - form a first look, it seems the only sensor-specific functions are read/write because of amount of data.. shouldn't be too hard, but my brain does not work atm