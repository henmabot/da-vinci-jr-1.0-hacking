#ifndef SAM4E8E_GPIO_H
#define SAM4E8E_GPIO_H

#include <stdbool.h>
#include <stdint.h>

typedef uint8_t gpio_pin_t;

#define GPIO_PIN(port, number) ((gpio_pin_t)((((port) - 'A') * 32) + (number)))
#define GPIO_PD8 GPIO_PIN('D', 8)
#define GPIO_PD23 GPIO_PIN('D', 23)

typedef enum {
    GPIO_PULL_NONE = 0,
    GPIO_PULL_UP = 1,
} gpio_pull_t;

void gpio_output(gpio_pin_t pin, bool initial_high);
void gpio_input(gpio_pin_t pin, gpio_pull_t pull);
void gpio_write(gpio_pin_t pin, bool high);
bool gpio_read(gpio_pin_t pin);

#endif
