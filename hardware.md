# Davinci Jr. 1.0 Hardware

Components that I was able to identify:

## Controllers and Processing

- 1x Atmel SAM4E8E MCU
- 1x NXP LPC1115 MCU
- 4x Toshiba TB62269FTG Stepper driver
- 1x Macronix MX25L3206E Flash memory

More info on the controllers and processing can be found in the [controllers](hardware/controllers.md) section.

## Movement

- 4x NEMA17 Stepper motors
- 3x Optical endstops
- 1x Filament sensor

More info on movement can be found in the [movement](hardware/movement.md) section.

## Interactivity

- 6x Buttons
- 1x Character LCD 4x20
- 1x AC-1203D RP1 Buzzer
- 1x SD card reader

More info on interactivity can be found in the [interactivity](hardware/interactivity.md) section.

## Hotend

- 1x Heater
- 1x NTC
- 1x Fan
- 1x Filament sensor (?)
- 1x Atmel AT24C02D Flash Memory

## Other

- 1x RFID reader
- 1x Top light bar
- 1x Reflow fan

I only identified the ones that need to be defined in the firmware, so no cable or passive components.

I verified the pins by first attaching an external motor/sensor and then by plugging in the stock hardware and verifying the output is the same.

## Sources:

- [Teardown (YouTube)](https://www.youtube.com/watch?v=cn2mYWmanlk) for most of the photos
- [Luc (Soliforum)](https://www.soliforum.com/post/131637/#p131637) for the original pinouts
- [julialongtin (GitHub)](https://github.com/julialongtin/Davinci_Jr_Hacking) for organizing the original pinouts
- My own disassembly, testing and photos of my unit
