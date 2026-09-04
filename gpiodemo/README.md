# GPIO controller

The GPIO controller is a Rust workspace with one shared wire-protocol crate and separate firmware and desktop programs:

```text
gpiodemo/
├── protocol/   no_std packet types plus ASCII encode/decode
├── firmware/   no_std SAM4E8E USB CDC + GPIO firmware
└── gui/        iced desktop controller
```

The desktop app and firmware both use `da-vinci-protocol`. Neither side has separate packet formatting or parsing code.

## Build and run

From the repository root:

```sh
just build   # build SAM4E8E firmware and emit build/firmware.bin
just gui     # run the desktop controller
just check   # formatting, tests, clippy, and firmware-target clippy
```

Install the Rust `thumbv7em-none-eabi` target before building firmware. `just build` also needs `arm-none-eabi-objcopy`. `just flash` needs BOSSA.

`just flash` flashes `build/firmware.bin` with BOSSA. Set `DEVICE` to override the default serial device.

The desktop controller also runs natively on macOS. From the repository root, run:

```sh
cargo run --manifest-path gpiodemo/Cargo.toml -p da-vinci-gui
```

Running the GUI does not require firmware cross-compilation tools. If you have `just`, `just gui` runs the same command.

## Protocol

Packets are newline-terminated ASCII. The host allocates request IDs from `001` through `999`, and the firmware echoes the same ID in responses. A successful `LSN ... ON` keeps its request ID for later listener notifications.

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

Malformed known requests return `UMM`. Unknown commands return `IDK`.

Pins start as `UNSET`. `DIR` initializes a pin and clears its pull-up state. `GET`, `SET`, `PLL`, and `LSN` reject uninitialized pins. `BYE` returns initialized pins to input/no-pull and clears listener state.

## Pin numbering

Wire IDs map directly to physical PIO pins:

- PA0-PA31: `000`-`031`
- PB0-PB14: `032`-`046`
- PC0-PC31: `047`-`078`
- PD0-PD31: `079`-`110`
- PE0-PE5: `111`-`116`

The 12 MHz crystal uses PB8/PB9 (`040`/`041`). USB CDC uses PB10/PB11 (`042`/`043`). The firmware returns `UNAVAILABLE` instead of reconfiguring them.
