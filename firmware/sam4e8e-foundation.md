# SAM4E8E firmware foundation

This repository contains a small bare-metal firmware base for the ATSAM4E8E on the Da Vinci Jr. main board. It provides the platform layer for a later Firmata port. The firmware does not include Firmata itself.

## Build

Install `just` and an ARM embedded GCC toolchain that provides `arm-none-eabi-gcc`, then run:

```sh
just build
```

The build produces:

- `build/firmware.elf`
- `build/firmware.bin`
- `build/firmware.map`

Run `just clean` to remove generated files.

The build is deliberately explicit: `Justfile` lists every compiled source, include directory, CPU flag, and the `conf/sam4e8e.ld` linker script. There is no UART build path.

## Example

`src/main.c`:

- configures PD23 as an output.
- configures PD8 as a digital input.
- initializes USB CDC ACM.
- toggles PD23 continuously.
- reports PD8 over USB CDC with a monotonically increasing counter.

The output is of the form:

```text
hello world 1, pd8 is high
hello world 2, pd8 is low
```

USB output is non-blocking. If the host stops consuming data, the example keeps blinking PD23 instead of blocking on serial output.

## Platform interfaces

Firmware code uses two small interfaces:

- `conf/gpio.h`: configure input/output, enable input pull-up, write, and read GPIOs.
- `conf/usb_cdc.h`: initialize CDC, write bytes, query received bytes, and read bytes.

These cover the immediate low-level needs found in ConfigurableFirmata's digital input/output modules and its byte-stream transport. A later Firmata adapter can add Arduino-style `Stream` glue without changing the SAM4E8E GPIO or USB implementation.

## Implementation sources

The implementation was intentionally reduced to SAM4E8E + USB CDC rather than importing a complete MCU framework.

- Klipper commit `f0892d82b0f1c1228454f09eb508eddde2250f4b` was the primary working reference.
  - `conf/usb_cdc.c` is substantially adapted from `src/atsam/sam4_usb.c` and the CDC control/descriptor logic in `src/generic/usb_cdc.c`.
  - `conf/gpio.c` follows the SAM4 register sequences in `src/atsam/gpio.c`.
  - `conf/startup.c` and `conf/sam4e8e.ld` adapt Klipper's generic Cortex-M startup/linker path.
  - `conf/clock.c` reduces Klipper's vendored `lib/sam4e/gcc/system_sam4e.c` to the 120 MHz `SystemInit` path used here.
  - `vendor/sam4e/` contains a SAM4E8E-specific trimmed device header plus only the Atmel PIO/PMC/UDP/WDT/EFC and pin-definition headers this build reaches. `vendor/cmsis-core/` contains only the CMSIS Cortex-M4 transitive headers reached by the compiler.
- ASF commit `68cddb46ae5ebc24ef8287a8d4c61a6efa5e2848` provides cross-checks for the SAM4E8E memory map, device definitions, and linker layout. The build does not compile the ASF driver/service tree.
- The local `reference/` project supplied board-specific evidence, especially PD23 and prior USB CDC bring-up. This implementation does not copy its project structure or ASF driver stack.
- ConfigurableFirmata commit `3734757348263e890d276f7e4fbc1f7e2bf5f2b9` and Firmata protocol commit `7908873e8faae33111143aa6cc236148b12118f2` provide the platform requirements used here.

Klipper-derived source files retain their GPLv3 copyright/license notices. Imported Atmel and Arm CMSIS files retain their original license headers.

## Clock and USB path

The clock setup uses Klipper's SAM4E path: the board pinout documents its 12 MHz crystal on PB8/XOUT and PB9/XIN, which feeds PLLA at 240 MHz. The master clock divides PLLA by two to 120 MHz, while USB divides PLLA by five to the required 48 MHz device clock.

USB CDC is the only serial transport. Klipper reserves PB10/PB11 for SAM4E USB. The SAM4E device definitions identify those system pins as DDM/DDP when firmware does not reassign them to GPIO. This firmware leaves them on the USB system function. It contains no UART/USART initialization or logging code.

The USB descriptor uses Klipper's development VID/PID pair, `1d50:614e`. Choose a product-specific USB identity before distributing this as a distinct USB product.

## Verification status

ARM GCC compiles and links the firmware successfully. Static ELF/map/disassembly checks show that:

- the vector table starts at flash address `0x00400000`.
- the linker includes `Reset_Handler`, `SystemInit`, and `UDP_Handler`.
- the linker uses 512 KiB flash at `0x00400000` and 128 KiB RAM at `0x20000000`.
- the USB CDC implementation is present.
- PD8 and PD23 compile to SAM4E8E pin indices 104 and 119, matching `PIO_PD8_IDX` and `PIO_PD23_IDX` from the imported device definitions.

No SAM4E8E board or debug probe was available in the build environment. USB enumeration, actual CDC output, PD23 blinking, and PD8 electrical state changes therefore still require physical-board validation.
