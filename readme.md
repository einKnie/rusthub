# Rusthub

Simple GUI to interact with specific BLE devices.

## Usage

## Development

### logging

`export RUST_LOG=warn,rusthub=debug` to set the loglevel to debug only for my crate.


## todos

### general

- [ ] improve logging
    - not too much, sensible loglevels

### UI

- [x] implement notification handling for sensor characteristic
    - kinda but not really
    - ~~some notifications are received, most are not~~
    - some thread (not directly mine) panics sometimes
        - update: this also happens when not subscribed, so likely unrelated?
- [x] make button image work OR make button show name/value
    - changed interface to have multiple buttons per sensor
- [x] make ui updatable on changes
- [ ] think of a good color scheme (dark and light themes)
- [ ] can i do a running statusbar?
- [x] let user change sensor name
- [ ] PERSISTENCE (remember known sensors)

### Peripheral Mgr

- [x] first automatic connection to peripheral always fails, why?
    - solved: on discover, peripheral was not added ot the list and when hub requested connect, the peripheral was therefore not found; fixed now
- [x] sensor data arrives wrong! (endianness?) - not anymore
- [ ] identify peripheral by id instead of hwAddr
- [ ] (maybe) split HubMsg ito two enums (flow ctrl & commands)
- [ ] improve errors
- [x] try to find out how to improve ble notification handling (some bluez thread (not my sources) keeps panicking from time to time)
    - update: this also happens when not subscribed (and when not connected to anything), so likely unrelated?
    - more update: might be circumstance, but it seems to fail way more with two sensors connected
    - FIXED with notification threads
- [x] event on device disconnect is not always received after disconnecting
    - actually event is received, but sensor does not always show the correct name -.- ("Arduino" instead of "MoistureSensor")
- [ ] make more async: msg thread should receive msgs but then spawn threads to handle requested actions
    - [ ] in doing that, rethink sensor handling in general, maybe don't store the actual Peripheral, but instead the id, so i can fetch it again from the context?
- [x] AT LEAST things like subscribing to notification should spawn a new thread to only handle that notification stream (should also minimize those panics, since they happen on drop of the messagestream in bluez-async -> if we don't drop it all the time, we can't have panics all the time *insert meme with guy tapping on forehead*)
    - basically, thread is spawned on susbscribe, killed on unsubscribe, listens on notificaiton stream al the time and sends received data to peripheral-mgr main thread
    - YES! works very well! ;)
