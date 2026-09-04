device := env("DEVICE", "/dev/tty.usbmodem101")
manifest := "gpiodemo/Cargo.toml"
target := "thumbv7em-none-eabi"
firmware_elf := "gpiodemo/target/" + target + "/firmware/da-vinci-firmware"
objcopy := env("OBJCOPY", "arm-none-eabi-objcopy")
size := env("SIZE", "arm-none-eabi-size")

default:
    @just --list

build:
    cargo build --manifest-path {{ manifest }} -p da-vinci-firmware --profile firmware --target {{ target }}
    mkdir -p build
    cp {{ firmware_elf }} build/firmware.elf
    {{ objcopy }} -O binary build/firmware.elf build/firmware.bin
    {{ size }} build/firmware.elf

gui:
    cargo run --manifest-path {{ manifest }} -p da-vinci-gui

gui-release:
    cargo run --manifest-path {{ manifest }} -p da-vinci-gui --release

check:
    cargo fmt --manifest-path {{ manifest }} --all -- --check
    cargo test --manifest-path {{ manifest }} -p da-vinci-protocol
    cargo test --manifest-path {{ manifest }} -p da-vinci-firmware
    cargo test --manifest-path {{ manifest }} -p da-vinci-gui
    cargo clippy --manifest-path {{ manifest }} --workspace --all-targets -- -D warnings
    cargo clippy --manifest-path {{ manifest }} -p da-vinci-firmware --profile firmware --target {{ target }} -- -D warnings

flash file="build/firmware.bin":
    bossac --port={{ device }} -e -w -v -b {{ file }}

clean:
    cargo clean --manifest-path {{ manifest }}
    rm -rf build
