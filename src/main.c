#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "gpio.h"
#include "usb_cdc.h"

static void delay(void)
{
    for (uint32_t i = 0; i < 12000000u; ++i)
        __asm volatile("nop");
}

static size_t append_u32(char *buffer, uint32_t value)
{
    char reversed[10];
    size_t length = 0u;

    do {
        reversed[length++] = (char)('0' + (value % 10u));
        value /= 10u;
    } while (value != 0u);

    for (size_t i = 0; i < length; ++i)
        buffer[i] = reversed[length - i - 1u];
    return length;
}

static bool report_pin(uint32_t counter, bool high)
{
    static const char prefix[] = "hello world ";
    static const char high_suffix[] = ", pd8 is high\r\n";
    static const char low_suffix[] = ", pd8 is low\r\n";
    const char *suffix = high ? high_suffix : low_suffix;
    const size_t suffix_length = high ? sizeof(high_suffix) - 1u : sizeof(low_suffix) - 1u;
    char line[40];
    size_t length = 0u;

    for (size_t i = 0; i < sizeof(prefix) - 1u; ++i)
        line[length++] = prefix[i];
    length += append_u32(&line[length], counter);
    for (size_t i = 0; i < suffix_length; ++i)
        line[length++] = suffix[i];

    return usb_cdc_write(line, length) == length;
}

int main(void)
{
    gpio_output(GPIO_PD23, false);
    gpio_input(GPIO_PD8, GPIO_PULL_NONE);
    usb_cdc_init();

    uint32_t counter = 1u;
    for (;;) {
        gpio_toggle(GPIO_PD23);
        if (usb_cdc_ready() && report_pin(counter, gpio_read(GPIO_PD8)))
            ++counter;
        delay();
    }
}
