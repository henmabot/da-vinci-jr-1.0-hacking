device := "/dev/tty.usbmodem101"
cross_compile := env_var_or_default("CROSS_COMPILE", "arm-none-eabi-")
cc := cross_compile + "gcc"
objcopy := cross_compile + "objcopy"
size := cross_compile + "size"

cpu_flags := "-mcpu=cortex-m4 -mthumb -mfloat-abi=soft"
cppflags := "-Iinclude -Ivendor/sam4e/include -Ivendor/cmsis-core"
cflags := cpu_flags + " -std=gnu11 -ffreestanding -Os -g3 -Wall -Wextra -Werror -ffunction-sections -fdata-sections -fno-common -fno-unwind-tables -fno-asynchronous-unwind-tables"
ldflags := cpu_flags + " -nostdlib -Tlinker/sam4e8e.ld -Wl,--gc-sections -Wl,--build-id=none -Wl,-Map,build/firmware.map"

default:
    @just --list

build:
    mkdir -p build/src build/platform
    {{ cc }} {{ cppflags }} {{ cflags }} -c src/main.c -o build/src/main.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c platform/startup.c -o build/platform/startup.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c platform/clock.c -o build/platform/clock.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c platform/gpio.c -o build/platform/gpio.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c platform/usb_cdc.c -o build/platform/usb_cdc.o
    {{ cc }} {{ ldflags }} build/src/main.o build/platform/startup.o build/platform/clock.o build/platform/gpio.o build/platform/usb_cdc.o -lgcc -o build/firmware.elf
    {{ objcopy }} -O binary build/firmware.elf build/firmware.bin
    {{ size }} build/firmware.elf

flash port=device file="build/firmware.bin": build
    bossac --port={{ port }} -e -w -v -b {{ file }}

clean:
    rm -rf build
