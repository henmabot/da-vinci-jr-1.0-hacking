/* ---------------------------------------------------------------------------- */
/*                  Atmel Microcontroller Software Support                      */
/*                       SAM Software Package License                           */
/* ---------------------------------------------------------------------------- */
/* Copyright (c) %copyright_year%, Atmel Corporation                            */
/*                                                                              */
/* All rights reserved.                                                         */
/*                                                                              */
/* Redistribution and use in source and binary forms, with or without           */
/* modification, are permitted provided that the following condition is met:    */
/*                                                                              */
/* - Redistributions of source code must retain the above copyright notice,     */
/* this list of conditions and the disclaimer below.                            */
/*                                                                              */
/* Atmel's name may not be used to endorse or promote products derived from     */
/* this software without specific prior written permission.                     */
/*                                                                              */
/* DISCLAIMER:  THIS SOFTWARE IS PROVIDED BY ATMEL "AS IS" AND ANY EXPRESS OR   */
/* IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF */
/* MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT ARE   */
/* DISCLAIMED. IN NO EVENT SHALL ATMEL BE LIABLE FOR ANY DIRECT, INDIRECT,      */
/* INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT */
/* LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA,  */
/* OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF    */
/* LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING         */
/* NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, */
/* EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.                           */
/* ---------------------------------------------------------------------------- */

#ifndef SAM4E8E_MINIMAL_H
#define SAM4E8E_MINIMAL_H

/*
 * Target-specific subset of Atmel's SAM4E8E device header. Only definitions
 * required by this firmware are retained; peripheral register definitions
 * remain in their original component headers below.
 */

#include <stdint.h>

typedef volatile const uint32_t RoReg;
typedef volatile uint32_t WoReg;
typedef volatile uint32_t RwReg;

typedef enum IRQn {
    SysTick_IRQn = -1,
    UDP_IRQn = 35,
} IRQn_Type;

#define __CM4_REV              0x0000
#define __MPU_PRESENT          0
#define __FPU_PRESENT          1
#define __NVIC_PRIO_BITS       4
#define __Vendor_SysTickConfig 0

#include <core_cm4.h>

#include "component/efc.h"
#include "component/pio.h"
#include "component/pmc.h"
#include "component/udp.h"
#include "component/wdt.h"

#define ID_PIOA 9u
#define ID_PIOB 10u
#define ID_PIOC 11u
#define ID_PIOD 12u
#define ID_PIOE 13u
#define ID_UDP 35u

#define UDP  ((Udp *)0x40084000u)
#define PMC  ((Pmc *)0x400E0400u)
#define EFC  ((Efc *)0x400E0A00u)
#define PIOA ((Pio *)0x400E0E00u)
#define PIOB ((Pio *)0x400E1000u)
#define PIOC ((Pio *)0x400E1200u)
#define PIOD ((Pio *)0x400E1400u)
#define PIOE ((Pio *)0x400E1600u)
#define WDT  ((Wdt *)0x400E1850u)

#include "pio/sam4e8e.h"

#endif
