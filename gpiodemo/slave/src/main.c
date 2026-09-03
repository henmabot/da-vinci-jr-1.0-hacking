#include "firmata.h"

int main(void)
{
    firmata_init();
    for (;;)
        firmata_task();
}
