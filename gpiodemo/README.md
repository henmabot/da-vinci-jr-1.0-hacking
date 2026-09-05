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
just build       # build SAM4E8E firmware and emit build/firmware.bin
just gui         # run the desktop controller
just gui-release # run an optimized desktop build for performance testing
just check       # formatting, tests, clippy, and firmware-target clippy
```

Install the Rust `thumbv7em-none-eabi` target before building firmware. `just build` also needs `arm-none-eabi-objcopy`. `just flash` needs BOSSA.

`just flash` flashes `build/firmware.bin` with BOSSA. Set `DEVICE` to override the default serial device.

The desktop controller also runs natively on macOS. From the repository root, run:

```sh
cargo run --manifest-path gpiodemo/Cargo.toml -p da-vinci-gui
```

Running the GUI does not require firmware cross-compilation tools. If you have `just`, `just gui` runs the same command. Use `just gui-release` when measuring desktop performance. The firmware uses its own size-optimized release profile.

## Protocol

Packets are newline-delimited ASCII with a host-allocated three-digit request ID and an explicit route/source token. For example, the host sends `001 SAM HAI` and the SAM node replies `001 SAM HII <3`.

See [`protocol.md`](protocol.md) for the complete wire contract, including commands, responses, GPIO target syntax, grouped operations, listener lifetime, errors, and reset behavior.
