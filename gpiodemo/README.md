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

Packets are newline-terminated ASCII. The host allocates request IDs from `001` through `999`, and the firmware echoes the same ID in responses. Request IDs are only correlation IDs. GPIO targets use symbolic PIO names. A successful `LSN ... ON` keeps its request ID for later listener notifications.

| Host request | Device response | Meaning |
| --- | --- | --- |
| `001 HAI` | `001 HII <3` | Connection check |
| `002 HRU` | `002 IAM SAM4E8E GPIO <3` | Device status |
| `003 DIR PA00 IN OK?` | `003 OKA <3` | Set input direction |
| `004 DIR PA00 OUT OK?` | `004 OKA <3` | Set output direction |
| `005 PLL PA00 ON OK?` | `005 OKA <3` | Enable input pull-up |
| `006 SET PA00 HIGH OK?` | `006 OKA <3` | Drive a pin high |
| `007 GET PA00 OK?` | `007 HYG PA00 HIGH <3` | Read a pin |
| `008 LSN PA00 ON OK?` | `008 OKA <3` | Start change reporting |
| `009 LSN PA00 OFF OK?` | `009 OKA <3` | Stop change reporting |
| `010 WYD PA00 DIR` | `010 HYG PA00 DIR IN <3` | Query direction |
| `011 WYD PA00 PLL` | `011 HYG PA00 PLL ON <3` | Query pull-up state |
| `012 WYD PA00 LSN` | `012 HYG PA00 LSN ON <3` | Query listener state |
| `013 BYE` | `013 CYA <3` | Clear pin/listener state |
| `014 DIR ALL IN OK?` | `014 OKA <3` | Set every available pin to input |
| `015 GET PIOC OK?` | `015 HYG ...`, then `015 OKA <3` | Read initialized pins in PIOC |
| `016 WYD ALL DIR` | `016 HYG ...`, then `016 OKA <3` | Query every available pin |

Malformed known requests return `UMM`. Unknown commands return `IDK`.

Pins start as `UNSET`. Every target-taking command accepts an individual pin (`PA00`), a PIO bank (`PIOA`), or `ALL`. Grouped operations skip unavailable pins. `GET`, `PLL`, and `LSN` operate on initialized pins in the selected scope. Grouped `SET` drives initialized outputs in that scope. `GET` and `WYD` with a bank or `ALL` send one `HYG` response per matching pin under the same request ID, followed by `OKA`. Individual operations still report the normal `UNSET` or `UNAVAILABLE` error. Legacy numeric GPIO targets are rejected. `BYE` returns initialized pins to input/no-pull and clears listener state.

## Pin naming

Individual wire targets use a PIO letter and a zero-padded two-digit bit number: `PA00` through `PA31`, `PB00` through `PB14`, `PC00` through `PC31`, `PD00` through `PD31`, and `PE00` through `PE05`. The desktop UI omits the zero padding and adds the physical package pin, for example `PB12 (87)`.

The 12 MHz crystal uses PB8/PB9, and USB CDC uses PB10/PB11. The firmware returns `UNAVAILABLE` instead of reconfiguring those pins. Grouped `PIOB` and `ALL` operations skip them automatically.
