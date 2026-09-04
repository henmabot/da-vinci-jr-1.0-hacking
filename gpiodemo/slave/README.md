# GPIO protocol firmware for SAM4E8E

This directory contains the SAM4E8E firmware used by the GPIO controller in `gpiodemo/master`. It keeps the existing bare-metal GPIO and USB CDC platform and implements the controller's small line-based request/response protocol directly.

## Build

Install `just` and an ARM embedded GCC toolchain that provides `arm-none-eabi-gcc`, then run:

```sh
just build
```

The build produces `build/firmware.elf`, `build/firmware.bin`, and `build/firmware.map`. `just flash` flashes `build/firmware.bin` with `bossac`.

## Protocol

Packets are newline-terminated ASCII. The host allocates a three-digit packet ID and the firmware uses the same ID for every response. A listener keeps the ID from its successful `LSN ... ON` request for later change notifications.

| Host request | Device response | Meaning |
| --- | --- | --- |
| `001 HAI` | `001 HII <3` | Connection check |
| `002 HRU` | `002 IAM SAM4E8E GPIO <3` | Device status |
| `003 DIR 000 IN OK?` | `003 OKA <3` | Set input direction |
| `004 DIR 000 OUT OK?` | `004 OKA <3` | Set output direction |
| `005 PLL 000 ON OK?` | `005 OKA <3` | Enable input pull-up |
| `006 SET 000 HIGH OK?` | `006 OKA <3` | Drive a pin high |
| `007 GET 000 OK?` | `007 HYG 000 HIGH <3` | Read a pin |
| `008 LSN 000 ON OK?` | `008 OKA <3` | Start change reporting |
| `009 LSN 000 OFF OK?` | `009 OKA <3` | Stop change reporting |
| `010 WYD 000 DIR` | `010 HYG 000 DIR IN <3` | Query direction |
| `011 WYD 000 PLL` | `011 HYG 000 PLL ON <3` | Query pull-up state |
| `012 WYD 000 LSN` | `012 HYG 000 LSN ON <3` | Query listener state |
| `013 BYE` | `013 CYA <3` | Clear pin/listener state |

A listener notification uses the ID from the request that enabled it, for example `008 HYG 000 LOW <3`. Invalid requests return `UMM`. Unknown commands return `IDK`.

Pins start in `UNSET`. `DIR` initializes a pin and resets its pull-up state. `GET`, `SET`, `PLL`, and `LSN` reject uninitialized pins. `BYE` releases every pin initialized through this protocol back to input/no-pull, marks it `UNSET`, and clears every listener.

## Pin numbering

Wire IDs pack the physical PIO pins rather than using the SAM4E register indices directly:

- PA0-PA31: `000`-`031`
- PB0-PB14: `032`-`046`
- PC0-PC31: `047`-`078`
- PD0-PD31: `079`-`110`
- PE0-PE5: `111`-`116`

The 12 MHz crystal uses PB8/PB9 (`040`/`041`). USB CDC uses PB10/PB11 (`042`/`043`). The firmware returns `UNAVAILABLE` rather than reconfiguring those pins.

## Layout

- `src/`: GPIO protocol engine and the minimal firmware entry point.
- `conf/`: SAM4E8E clock, startup, linker, GPIO, and USB CDC implementation.
- `vendor/`: the trimmed SAM4E8E and CMSIS headers needed by the build.

`Justfile` is the build entry point and lists the firmware sources explicitly.

## Sources

Klipper commit `f0892d82b0f1c1228454f09eb508eddde2250f4b` supplies the clock, Cortex-M startup/linker, GPIO register sequencing, and USB CDC/UDP foundation. ASF commit `68cddb46ae5ebc24ef8287a8d4c61a6efa5e2848` is the cross-reference for SAM4E8E device definitions and memory layout.

Klipper-derived files retain their GPLv3 notices. Imported Atmel and Arm CMSIS files retain their original license headers.
