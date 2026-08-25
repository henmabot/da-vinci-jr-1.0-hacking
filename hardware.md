# Davinci Jr. 1.0 Hardware

To start off, there are 3 boards in the Davinci Jr. 1.0:

- Main board
- LCD, buttons and SD card board (I will refer to this as the sub-board)
- Hotend board (I will refer to this as the hotend/hotend board)

## Main Board

Main components on the main board:

- 1x Atmel SAM4E8E MCU
- 1x NXP LPC1115 MCU
- 4x Stepper drivers
- 1x 4MB Flash memory

More info about the main board can be found in the [Main Board](hardware/board-main.md) section.

## Sub-board

Main components on the sub-board:

- 1x Character LCD 4x16
- 1x SD card reader
- 6x Buttons

More info about the sub-board can be found in the [Sub-Board](hardware/board-sub.md) section.

## Hotend

Components that I was able to identify on the hotend:

- 1x Heater
- 1x NTC
- 1x Fan
- 1x Filament sensor (?)
- 1x (?)KB Flash Memory

More info on these can be found in the [Hotend](hardware/board-hotend.md) section.

## Other

The remaining components that do not belong to a board are:

- 4x NEMA17 Stepper motors
- 3x Optical endstops
- 1x Filament sensor
- 1x RFID reader
- 1x Top light bar
- 1x Reflow fan

## Extras

I wasn't planning to identify the cables, but i do have to identify the flex cables and sensor cables as they are needed to figure out the pinouts for the sensors, sub-board and hotend.
More information about these can be found in the [connectors](hardware/connectors.md) section.

## Notes:

- I only identified the ones that need to be defined in the firmware, so no cable or passive components for now.
- I verified the pins by first attaching an external motor/sensor and then by plugging in the stock hardware and verifying the output is the same.
- Since hotend has its own board, and its tiny, I felt like it would be more appropriate to have it as its own group than dissect it into the other groups, and mention it from other docs.
- I will create a separate sources file later as the sources list keeps growing and duplicated per-document.
- My reflow fan controller is burnt, I may or may not be able to verify its pin.

## Sources:

Please refer to [SOURCES.md](SOURCES.md) for the full list of sources.
