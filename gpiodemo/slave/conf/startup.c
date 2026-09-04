// ARM Cortex-M startup adapted from Klipper src/generic/armcm_boot.c.
//
// Copyright (C) 2019 Kevin O'Connor <kevin@koconnor.net>
//
// This file may be distributed under the terms of the GNU GPLv3 license.

#include <stdint.h>
#include "sam4e8e.h"

extern uint32_t _data_load;
extern uint32_t _data_start;
extern uint32_t _data_end;
extern uint32_t _bss_start;
extern uint32_t _bss_end;
extern uint32_t _stack_end;

extern int main(void);
void SystemInit(void);
void UDP_Handler(void);
void Reset_Handler(void);

static void Default_Handler(void)
{
    for (;;)
        ;
}

__attribute__((used, section(".vectors")))
const uintptr_t vector_table[16u + 46u] = {
    [0] = (uintptr_t)&_stack_end,
    [1] = (uintptr_t)Reset_Handler,
    [2] = (uintptr_t)Default_Handler,
    [3] = (uintptr_t)Default_Handler,
    [4] = (uintptr_t)Default_Handler,
    [5] = (uintptr_t)Default_Handler,
    [6] = (uintptr_t)Default_Handler,
    [11] = (uintptr_t)Default_Handler,
    [12] = (uintptr_t)Default_Handler,
    [14] = (uintptr_t)Default_Handler,
    [15] = (uintptr_t)Default_Handler,
    [16 ... 50] = (uintptr_t)Default_Handler,
    [16 + UDP_IRQn] = (uintptr_t)UDP_Handler,
    [52 ... 61] = (uintptr_t)Default_Handler,
};

static void __attribute__((used, noreturn)) reset_handler_c(void)
{
    for (uint32_t i = 0; i < (sizeof(NVIC->ICER) / sizeof(NVIC->ICER[0])); ++i) {
        NVIC->ICER[i] = 0xffffffffu;
        NVIC->ICPR[i] = 0xffffffffu;
    }
    SysTick->CTRL = 0u;

    uint32_t *src = &_data_load;
    for (uint32_t *dst = &_data_start; dst < &_data_end;)
        *dst++ = *src++;
    for (uint32_t *dst = &_bss_start; dst < &_bss_end;)
        *dst++ = 0u;

    SCB->VTOR = (uint32_t)&vector_table;
    SystemInit();
    WDT->WDT_MR = WDT_MR_WDDIS;

    __DSB();
    __ISB();
    __enable_irq();

    (void)main();
    for (;;)
        ;
}

void __attribute__((naked, noreturn)) Reset_Handler(void)
{
    __asm volatile(
        "ldr r0, =_stack_end\n"
        "mov sp, r0\n"
        "b reset_handler_c\n"
    );
}
