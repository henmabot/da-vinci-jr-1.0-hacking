default:
    @just --list

build:
    make

flash device file="build/firmware.bin": build
    bossac --port={{ device }} -e -w -v -b {{ file }}

clean:
    make clean
