device := "/dev/tty.usbmodem101"
src_dir := "src"
conf_dir := "conf"
cross_compile := env("CROSS_COMPILE", "arm-none-eabi-")
cc := cross_compile + "gcc"
objcopy := cross_compile + "objcopy"
size := cross_compile + "size"

cpu_flags := "-mcpu=cortex-m4 -mthumb -mfloat-abi=soft"
cppflags := "-I" + conf_dir + " -Ivendor/sam4e/include -Ivendor/cmsis-core"
cflags := cpu_flags + " -std=gnu11 -ffreestanding -Os -g3 -Wall -Wextra -Werror -ffunction-sections -fdata-sections -fno-common -fno-unwind-tables -fno-asynchronous-unwind-tables"
ldflags := cpu_flags + " -nostdlib -T" + conf_dir + "/sam4e8e.ld -Wl,--gc-sections -Wl,--build-id=none -Wl,-Map,build/firmware.map"

default:
    @just --list

build:
    mkdir -p build
    {{ cc }} {{ cppflags }} {{ cflags }} -c {{ src_dir }}/main.c -o build/main.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c {{ conf_dir }}/startup.c -o build/startup.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c {{ conf_dir }}/clock.c -o build/clock.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c {{ conf_dir }}/gpio.c -o build/gpio.o
    {{ cc }} {{ cppflags }} {{ cflags }} -c {{ conf_dir }}/usb_cdc.c -o build/usb_cdc.o
    {{ cc }} {{ ldflags }} build/main.o build/startup.o build/clock.o build/gpio.o build/usb_cdc.o -lgcc -o build/firmware.elf
    {{ objcopy }} -O binary build/firmware.elf build/firmware.bin
    {{ size }} build/firmware.elf

flash port=device file="build/firmware.bin": build
    bossac --port={{ port }} -e -w -v -b {{ file }}

clean:
    rm -rf build
