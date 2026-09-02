/*
 * SAM4E8E linker layout derived from Klipper's src/generic/armcm_link.lds.S.
 *
 * Copyright (C) 2019 Kevin O'Connor <kevin@koconnor.net>
 *
 * This file may be distributed under the terms of the GNU GPLv3 license.
 */

OUTPUT_FORMAT("elf32-littlearm", "elf32-littlearm", "elf32-littlearm")
OUTPUT_ARCH(arm)
ENTRY(Reset_Handler)

MEMORY
{
    FLASH (rx)  : ORIGIN = 0x00400000, LENGTH = 0x00080000
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 0x00020000
}

STACK_SIZE = 0x1000;

SECTIONS
{
    .text :
    {
        . = ALIGN(4);
        KEEP(*(.vectors))
        *(.text .text.*)
        *(.rodata .rodata.*)
        . = ALIGN(4);
        _text_end = .;
    } > FLASH

    .data : AT (_text_end)
    {
        . = ALIGN(4);
        _data_start = .;
        *(.data .data.*)
        . = ALIGN(4);
        _data_end = .;
    } > RAM
    _data_load = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        _bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } > RAM

    _stack_end = ORIGIN(RAM) + LENGTH(RAM);
    _stack_start = _stack_end - STACK_SIZE;

    /DISCARD/ :
    {
        *(.ARM.extab*)
        *(.ARM.exidx*)
        *(.eh_frame*)
        *(.init)
        *(.fini)
    }
}

ASSERT(_bss_end <= _stack_start, "SAM4E8E RAM overflow")
