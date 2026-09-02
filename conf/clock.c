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

#include "sam4e8e.h"

#define OSC_STARTUP CKGR_MOR_MOSCXTST(0x8u)
#define PLLA_240MHZ (CKGR_PLLAR_ONE | CKGR_PLLAR_MULA(0x13u) | \
                     CKGR_PLLAR_PLLACOUNT(0x3fu) | CKGR_PLLAR_DIVA(0x1u))
#define MCK_120MHZ (PMC_MCKR_PRES_CLK_2 | PMC_MCKR_CSS_PLLA_CLK)
#define MOR_KEY CKGR_MOR_KEY(0x37u)

void SystemInit(void)
{
    EFC->EEFC_FMR = EEFC_FMR_FWS(5u);

    if ((PMC->CKGR_MOR & CKGR_MOR_MOSCSEL) == 0u) {
        PMC->CKGR_MOR = MOR_KEY | OSC_STARTUP | CKGR_MOR_MOSCRCEN | CKGR_MOR_MOSCXTEN;
        while ((PMC->PMC_SR & PMC_SR_MOSCXTS) == 0u)
            ;
    }

    PMC->CKGR_MOR = MOR_KEY | OSC_STARTUP | CKGR_MOR_MOSCRCEN |
                    CKGR_MOR_MOSCXTEN | CKGR_MOR_MOSCSEL;
    while ((PMC->PMC_SR & PMC_SR_MOSCSELS) == 0u)
        ;

    PMC->PMC_MCKR = (PMC->PMC_MCKR & ~PMC_MCKR_CSS_Msk) | PMC_MCKR_CSS_MAIN_CLK;
    while ((PMC->PMC_SR & PMC_SR_MCKRDY) == 0u)
        ;

    PMC->CKGR_PLLAR = PLLA_240MHZ;
    while ((PMC->PMC_SR & PMC_SR_LOCKA) == 0u)
        ;

    PMC->PMC_MCKR = (MCK_120MHZ & ~PMC_MCKR_CSS_Msk) | PMC_MCKR_CSS_MAIN_CLK;
    while ((PMC->PMC_SR & PMC_SR_MCKRDY) == 0u)
        ;

    PMC->PMC_MCKR = MCK_120MHZ;
    while ((PMC->PMC_SR & PMC_SR_MCKRDY) == 0u)
        ;

}
