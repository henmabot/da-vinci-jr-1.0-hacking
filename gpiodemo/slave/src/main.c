#include "protocol.h"

int main(void)
{
    gpio_protocol_init();
    for (;;)
        gpio_protocol_task();
}
