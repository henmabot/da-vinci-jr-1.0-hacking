# Firmata for SAM4E8E

This directory contains the SAM4E8E Firmata firmware for the Da Vinci Jr. main board. The port maps ConfigurableFirmata's digital I/O behavior to the repository's bare-metal GPIO and USB CDC interfaces. It does not bring in Arduino or a second hardware abstraction layer.

## Build and test

Install `just` and an ARM embedded GCC toolchain that provides `arm-none-eabi-gcc`, then run:

```sh
just test
just build
```

The build produces `build/firmware.elf`, `build/firmware.bin`, and `build/firmware.map`. `just flash` builds and flashes `build/firmware.bin` with `bossac`.

## Supported Firmata features

The firmware reports Firmata protocol 2.8 and ConfigurableFirmata firmware version 3.4. It supports:

- digital input, output, and input pull-up pin modes.
- digital port writes and single-pin writes.
- digital input reporting, including an immediate report when the host enables reporting.
- protocol and firmware version queries.
- capability, pin-state, and analog-mapping queries.
- system reset.

The analog-mapping response marks every pin as non-analog. Analog I/O, pulse-width modulation, servos, I2C, serial peripheral interface, OneWire, hardware UART/USART, and the other optional ConfigurableFirmata modules are not included yet. USB CDC is the only Firmata transport.

## Pin numbering

Firmata pin numbers are the SAM4E PIO indices used by `gpio_pin_t`:

- PA0-PA31: 0-31
- PB0-PB14: 32-46
- PC0-PC31: 64-95
- PD0-PD31: 96-127

The SAM4E8E package has no PB15-PB31 pins, so Firmata marks those indices as unsupported. The 12 MHz crystal uses PB8/PB9. USB uses PB10/PB11, so Firmata also marks those pins as unsupported. PE0-PE5 use indices outside Firmata's 0-127 pin range, so this port does not expose them.

A system reset puts every exposed Firmata pin into high-impedance digital input mode. This is safer for the printer board than driving all digital pins during reset. The host can then select output or pull-up modes explicitly.

## Layout

- `src/`: Firmata protocol engine and the minimal firmware entry point.
- `conf/`: SAM4E8E clock, startup, linker, GPIO, and USB CDC implementation.
- `vendor/`: the trimmed SAM4E8E and CMSIS headers needed by the build.
- `tests/`: host-side protocol tests with fake GPIO and USB implementations.

`Justfile` remains the single build entry point and lists the firmware sources explicitly.

## Sources

The protocol behavior follows ConfigurableFirmata commit `3734757348263e890d276f7e4fbc1f7e2bf5f2b9`, especially its digital input/output modules and parser behavior, and Firmata protocol commit `7908873e8faae33111143aa6cc236148b12118f2`.

The earlier firmware foundation provides the SAM4E8E platform layer. Klipper commit `f0892d82b0f1c1228454f09eb508eddde2250f4b` remains the primary source for the clock, Cortex-M startup/linker path, GPIO register sequencing, and USB CDC/UDP implementation. ASF commit `68cddb46ae5ebc24ef8287a8d4c61a6efa5e2848` is the cross-reference for SAM4E8E device definitions and memory layout.

Klipper-derived files retain their GPLv3 notices. Imported Atmel and Arm CMSIS files retain their original license headers.

## Verification

`just test` exercises the Firmata protocol on the host. It covers startup reporting, version queries, pin modes, writes, digital input reporting, discovery queries, reserved pins, partial USB writes, and system reset. `just build` cross-compiles and links the same Firmata engine into the SAM4E8E firmware.

The project maintainer previously tested the merged GPIO and USB CDC foundation on the physical board. The new protocol layer still needs an end-to-end client test on the board.
