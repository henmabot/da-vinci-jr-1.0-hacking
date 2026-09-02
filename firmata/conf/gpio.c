// GPIO access for ATSAM4E8E.
//
// The register sequence follows Klipper src/atsam/gpio.c.
// Copyright (C) 2016-2018 Kevin O'Connor <kevin@koconnor.net>
//
// This file may be distributed under the terms of the GNU GPLv3 license.

#include "gpio.h"
#include "sam4e8e.h"

static Pio *const gpio_ports[] = {PIOA, PIOB, PIOC, PIOD, PIOE};
static const uint8_t gpio_clock_ids[] = {ID_PIOA, ID_PIOB, ID_PIOC, ID_PIOD, ID_PIOE};
static Pio *gpio_port(gpio_pin_t pin)
{
    return gpio_ports[pin / 32u];
}

static uint32_t gpio_mask(gpio_pin_t pin)
{
    return 1u << (pin % 32u);
}

static void gpio_enable_clock(gpio_pin_t pin)
{
    PMC->PMC_PCER0 = 1u << gpio_clock_ids[pin / 32u];
}

static void gpio_set_pull(Pio *port, uint32_t mask, gpio_pull_t pull)
{
    if (pull == GPIO_PULL_UP) {
        port->PIO_PPDDR = mask;
        port->PIO_PUER = mask;
    } else {
        port->PIO_PUDR = mask;
        port->PIO_PPDDR = mask;
    }
}

void gpio_output(gpio_pin_t pin, bool initial_high)
{
    Pio *const port = gpio_port(pin);
    const uint32_t mask = gpio_mask(pin);

    gpio_enable_clock(pin);
    gpio_set_pull(port, mask, GPIO_PULL_NONE);
    if (initial_high)
        port->PIO_SODR = mask;
    else
        port->PIO_CODR = mask;
    port->PIO_OER = mask;
    port->PIO_OWER = mask;
    port->PIO_PER = mask;
}

void gpio_input(gpio_pin_t pin, gpio_pull_t pull)
{
    Pio *const port = gpio_port(pin);
    const uint32_t mask = gpio_mask(pin);

    gpio_enable_clock(pin);
    gpio_set_pull(port, mask, pull);
    port->PIO_ODR = mask;
    port->PIO_PER = mask;
}

void gpio_write(gpio_pin_t pin, bool high)
{
    Pio *const port = gpio_port(pin);
    const uint32_t mask = gpio_mask(pin);

    if (high)
        port->PIO_SODR = mask;
    else
        port->PIO_CODR = mask;
}

bool gpio_read(gpio_pin_t pin)
{
    Pio *const port = gpio_port(pin);
    return (port->PIO_PDSR & gpio_mask(pin)) != 0u;
}
