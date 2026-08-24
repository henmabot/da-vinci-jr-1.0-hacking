## Davinci Jr. 1.0 Hardware

Components that I was able to identify:

### Controllers and Processing

- 1x Atmel SAM4E8E MCU
- 1x NXP LPC1115 MCU
- 4x Toshiba TB62269FTG Stepper driver
- 1x Flash memory (no info)

### Movement

- 4x Stepper motors
- 3x Optical endstops
- 1x Filament sensor

### Interactivity

- 6x buttons
- 1x Character LCD 4x20
- 1x Buzzer
-

### Other

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
