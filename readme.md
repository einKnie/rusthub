# Rusthub

Simple GUI to interact with specific BLE devices.

## Usage

## Development

### logging

`export RUST_LOG=warn,rusthub=debug` to set the loglevel to debug only for my crate.


## todos

- [ ] implement notification handling for sensor characteristic
    - kinda but not really
    - some notifications are received, most are not, and some thread (not directly mine) panics sometimes
    - not sure which side to blame yet
- [x] make button image work OR make button show name/value
    - changed interface to have multiple buttons per sensor
- [ ] make everything better and more robust
- [x] first automatic connection to peripheral always fails, why?
    - solved: on discover, peripheral was not added ot the list and when hub requested connect, the peripheral was therefore not found; fixed now
- [x] sensor data arrives wrong! (endianness?) - not anymore
- [x] make ui updatable on changes
