#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "firmata.h"
#include "gpio.h"
#include "usb_cdc.h"

#define DIGITAL_MESSAGE 0x90u
#define REPORT_ANALOG 0xc0u
#define REPORT_DIGITAL 0xd0u
#define ANALOG_MESSAGE 0xe0u
#define START_SYSEX 0xf0u
#define SET_PIN_MODE 0xf4u
#define SET_DIGITAL_PIN_VALUE 0xf5u
#define END_SYSEX 0xf7u
#define REPORT_VERSION 0xf9u
#define SYSTEM_RESET 0xffu

#define ANALOG_MAPPING_QUERY 0x69u
#define ANALOG_MAPPING_RESPONSE 0x6au
#define CAPABILITY_QUERY 0x6bu
#define CAPABILITY_RESPONSE 0x6cu
#define PIN_STATE_QUERY 0x6du
#define PIN_STATE_RESPONSE 0x6eu
#define REPORT_FIRMWARE 0x79u

#define PIN_MODE_INPUT 0x00u
#define PIN_MODE_OUTPUT 0x01u
#define PIN_MODE_PULLUP 0x0bu
#define PIN_MODE_IGNORE 0x7fu

#define FIRMATA_PROTOCOL_MAJOR 2u
#define FIRMATA_PROTOCOL_MINOR 8u
#define FIRMATA_FIRMWARE_MAJOR 3u
#define FIRMATA_FIRMWARE_MINOR 4u

#define FIRMATA_PIN_COUNT 128u
#define FIRMATA_PORT_COUNT (FIRMATA_PIN_COUNT / 8u)
#define FIRMATA_SYSEX_CAPACITY 64u
#define FIRMATA_TX_CAPACITY (FIRMATA_PIN_COUNT * 7u + 3u)

typedef struct {
    gpio_pin_t gpio;
    bool available;
} firmata_pin_mapping_t;

#define FIRMATA_GPIO(pin) [pin] = {.gpio = (gpio_pin_t)(pin), .available = true}

static const firmata_pin_mapping_t firmata_pins[FIRMATA_PIN_COUNT] = {
    /* PIOA */
    FIRMATA_GPIO(PIO_PA0_IDX),
    FIRMATA_GPIO(PIO_PA1_IDX),
    FIRMATA_GPIO(PIO_PA2_IDX),
    FIRMATA_GPIO(PIO_PA3_IDX),
    FIRMATA_GPIO(PIO_PA4_IDX),
    FIRMATA_GPIO(PIO_PA5_IDX),
    FIRMATA_GPIO(PIO_PA6_IDX),
    FIRMATA_GPIO(PIO_PA7_IDX),
    FIRMATA_GPIO(PIO_PA8_IDX),
    FIRMATA_GPIO(PIO_PA9_IDX),
    FIRMATA_GPIO(PIO_PA10_IDX),
    FIRMATA_GPIO(PIO_PA11_IDX),
    FIRMATA_GPIO(PIO_PA12_IDX),
    FIRMATA_GPIO(PIO_PA13_IDX),
    FIRMATA_GPIO(PIO_PA14_IDX),
    FIRMATA_GPIO(PIO_PA15_IDX),
    FIRMATA_GPIO(PIO_PA16_IDX),
    FIRMATA_GPIO(PIO_PA17_IDX),
    FIRMATA_GPIO(PIO_PA18_IDX),
    FIRMATA_GPIO(PIO_PA19_IDX),
    FIRMATA_GPIO(PIO_PA20_IDX),
    FIRMATA_GPIO(PIO_PA21_IDX),
    FIRMATA_GPIO(PIO_PA22_IDX),
    FIRMATA_GPIO(PIO_PA23_IDX),
    FIRMATA_GPIO(PIO_PA24_IDX),
    FIRMATA_GPIO(PIO_PA25_IDX),
    FIRMATA_GPIO(PIO_PA26_IDX),
    FIRMATA_GPIO(PIO_PA27_IDX),
    FIRMATA_GPIO(PIO_PA28_IDX),
    FIRMATA_GPIO(PIO_PA29_IDX),
    FIRMATA_GPIO(PIO_PA30_IDX),
    FIRMATA_GPIO(PIO_PA31_IDX),

    /* PIOB. PB8/PB9 are the crystal; PB10/PB11 are USB. */
    FIRMATA_GPIO(PIO_PB0_IDX),
    FIRMATA_GPIO(PIO_PB1_IDX),
    FIRMATA_GPIO(PIO_PB2_IDX),
    FIRMATA_GPIO(PIO_PB3_IDX),
    FIRMATA_GPIO(PIO_PB4_IDX),
    FIRMATA_GPIO(PIO_PB5_IDX),
    FIRMATA_GPIO(PIO_PB6_IDX),
    FIRMATA_GPIO(PIO_PB7_IDX),
    FIRMATA_GPIO(PIO_PB12_IDX),
    FIRMATA_GPIO(PIO_PB13_IDX),
    FIRMATA_GPIO(PIO_PB14_IDX),

    /* PIOC */
    FIRMATA_GPIO(PIO_PC0_IDX),
    FIRMATA_GPIO(PIO_PC1_IDX),
    FIRMATA_GPIO(PIO_PC2_IDX),
    FIRMATA_GPIO(PIO_PC3_IDX),
    FIRMATA_GPIO(PIO_PC4_IDX),
    FIRMATA_GPIO(PIO_PC5_IDX),
    FIRMATA_GPIO(PIO_PC6_IDX),
    FIRMATA_GPIO(PIO_PC7_IDX),
    FIRMATA_GPIO(PIO_PC8_IDX),
    FIRMATA_GPIO(PIO_PC9_IDX),
    FIRMATA_GPIO(PIO_PC10_IDX),
    FIRMATA_GPIO(PIO_PC11_IDX),
    FIRMATA_GPIO(PIO_PC12_IDX),
    FIRMATA_GPIO(PIO_PC13_IDX),
    FIRMATA_GPIO(PIO_PC14_IDX),
    FIRMATA_GPIO(PIO_PC15_IDX),
    FIRMATA_GPIO(PIO_PC16_IDX),
    FIRMATA_GPIO(PIO_PC17_IDX),
    FIRMATA_GPIO(PIO_PC18_IDX),
    FIRMATA_GPIO(PIO_PC19_IDX),
    FIRMATA_GPIO(PIO_PC20_IDX),
    FIRMATA_GPIO(PIO_PC21_IDX),
    FIRMATA_GPIO(PIO_PC22_IDX),
    FIRMATA_GPIO(PIO_PC23_IDX),
    FIRMATA_GPIO(PIO_PC24_IDX),
    FIRMATA_GPIO(PIO_PC25_IDX),
    FIRMATA_GPIO(PIO_PC26_IDX),
    FIRMATA_GPIO(PIO_PC27_IDX),
    FIRMATA_GPIO(PIO_PC28_IDX),
    FIRMATA_GPIO(PIO_PC29_IDX),
    FIRMATA_GPIO(PIO_PC30_IDX),
    FIRMATA_GPIO(PIO_PC31_IDX),

    /* PIOD */
    FIRMATA_GPIO(PIO_PD0_IDX),
    FIRMATA_GPIO(PIO_PD1_IDX),
    FIRMATA_GPIO(PIO_PD2_IDX),
    FIRMATA_GPIO(PIO_PD3_IDX),
    FIRMATA_GPIO(PIO_PD4_IDX),
    FIRMATA_GPIO(PIO_PD5_IDX),
    FIRMATA_GPIO(PIO_PD6_IDX),
    FIRMATA_GPIO(PIO_PD7_IDX),
    FIRMATA_GPIO(PIO_PD8_IDX),
    FIRMATA_GPIO(PIO_PD9_IDX),
    FIRMATA_GPIO(PIO_PD10_IDX),
    FIRMATA_GPIO(PIO_PD11_IDX),
    FIRMATA_GPIO(PIO_PD12_IDX),
    FIRMATA_GPIO(PIO_PD13_IDX),
    FIRMATA_GPIO(PIO_PD14_IDX),
    FIRMATA_GPIO(PIO_PD15_IDX),
    FIRMATA_GPIO(PIO_PD16_IDX),
    FIRMATA_GPIO(PIO_PD17_IDX),
    FIRMATA_GPIO(PIO_PD18_IDX),
    FIRMATA_GPIO(PIO_PD19_IDX),
    FIRMATA_GPIO(PIO_PD20_IDX),
    FIRMATA_GPIO(PIO_PD21_IDX),
    FIRMATA_GPIO(PIO_PD22_IDX),
    FIRMATA_GPIO(PIO_PD23_IDX),
    FIRMATA_GPIO(PIO_PD24_IDX),
    FIRMATA_GPIO(PIO_PD25_IDX),
    FIRMATA_GPIO(PIO_PD26_IDX),
    FIRMATA_GPIO(PIO_PD27_IDX),
    FIRMATA_GPIO(PIO_PD28_IDX),
    FIRMATA_GPIO(PIO_PD29_IDX),
    FIRMATA_GPIO(PIO_PD30_IDX),
    FIRMATA_GPIO(PIO_PD31_IDX),
};

#undef FIRMATA_GPIO

_Static_assert(PIO_PD31_IDX == FIRMATA_PIN_COUNT - 1u,
               "Firmata pin range must end at PD31");
_Static_assert(PIO_PE0_IDX == FIRMATA_PIN_COUNT,
               "PE pins must remain outside the 7-bit Firmata pin range");

static const char firmware_name[] = "ConfigurableFirmata";

static uint8_t pin_mode[FIRMATA_PIN_COUNT];
static uint8_t pin_state[FIRMATA_PIN_COUNT];
static uint8_t report_ports[FIRMATA_PORT_COUNT];
static uint8_t previous_ports[FIRMATA_PORT_COUNT];

static uint8_t pending_command;
static uint8_t pending_channel;
static uint8_t pending_data[2];
static uint8_t pending_received;
static uint8_t pending_needed;

static bool parsing_sysex;
static uint8_t sysex_data[FIRMATA_SYSEX_CAPACITY];
static size_t sysex_length;

static uint8_t tx_buffer[FIRMATA_TX_CAPACITY];
static size_t tx_length;
static size_t tx_offset;

static bool firmata_pin_available(uint8_t firmata_pin)
{
    return firmata_pins[firmata_pin].available;
}

static bool tx_pending(void)
{
    return tx_offset != tx_length;
}

static void tx_clear(void)
{
    tx_length = 0u;
    tx_offset = 0u;
}

static void tx_byte(uint8_t value)
{
    tx_buffer[tx_length++] = value;
}

static void tx_sysex_begin(uint8_t command)
{
    tx_clear();
    tx_byte(START_SYSEX);
    tx_byte(command);
}

static void tx_sysex_end(void)
{
    tx_byte(END_SYSEX);
}

static void tx_flush(void)
{
    if (!tx_pending())
        return;

    tx_offset += usb_cdc_write(&tx_buffer[tx_offset], tx_length - tx_offset);
    if (tx_offset == tx_length)
        tx_clear();
}

static void queue_protocol_version(void)
{
    tx_clear();
    tx_byte(REPORT_VERSION);
    tx_byte(FIRMATA_PROTOCOL_MAJOR);
    tx_byte(FIRMATA_PROTOCOL_MINOR);
}

static void append_firmware_report(void)
{
    tx_byte(START_SYSEX);
    tx_byte(REPORT_FIRMWARE);
    tx_byte(FIRMATA_FIRMWARE_MAJOR);
    tx_byte(FIRMATA_FIRMWARE_MINOR);
    for (size_t i = 0u; i < sizeof(firmware_name) - 1u; ++i) {
        const uint8_t value = (uint8_t)firmware_name[i];
        tx_byte(value & 0x7fu);
        tx_byte(value >> 7);
    }
    tx_byte(END_SYSEX);
}

static void queue_capability_response(void)
{
    tx_sysex_begin(CAPABILITY_RESPONSE);
    for (uint16_t pin = 0u; pin < FIRMATA_PIN_COUNT; ++pin) {
        if (firmata_pin_available((uint8_t)pin)) {
            tx_byte(PIN_MODE_INPUT);
            tx_byte(1u);
            tx_byte(PIN_MODE_OUTPUT);
            tx_byte(1u);
            tx_byte(PIN_MODE_PULLUP);
            tx_byte(1u);
        }
        tx_byte(0x7fu);
    }
    tx_sysex_end();
}

static void queue_analog_mapping_response(void)
{
    tx_sysex_begin(ANALOG_MAPPING_RESPONSE);
    for (uint16_t pin = 0u; pin < FIRMATA_PIN_COUNT; ++pin)
        tx_byte(0x7fu);
    tx_sysex_end();
}

static void queue_pin_state_response(uint8_t firmata_pin)
{
    tx_sysex_begin(PIN_STATE_RESPONSE);
    tx_byte(firmata_pin);
    tx_byte(pin_mode[firmata_pin]);
    tx_byte(pin_state[firmata_pin] & 0x7fu);
    tx_sysex_end();
}

static uint8_t read_port(uint8_t port)
{
    uint8_t value = 0u;
    const uint8_t first_pin = (uint8_t)(port * 8u);

    for (uint8_t bit = 0u; bit < 8u; ++bit) {
        const uint8_t firmata_pin = (uint8_t)(first_pin + bit);
        if (!firmata_pin_available(firmata_pin))
            continue;
        if (pin_mode[firmata_pin] != PIN_MODE_INPUT &&
            pin_mode[firmata_pin] != PIN_MODE_PULLUP)
            continue;
        if (gpio_read(firmata_pins[firmata_pin].gpio))
            value |= (uint8_t)(1u << bit);
    }
    return value;
}

static void append_digital_port(uint8_t port, uint8_t value)
{
    tx_byte((uint8_t)(DIGITAL_MESSAGE | port));
    tx_byte(value & 0x7fu);
    tx_byte(value >> 7);
}

static void set_pin_mode(uint8_t firmata_pin, uint8_t mode)
{
    if (!firmata_pin_available(firmata_pin))
        return;

    const gpio_pin_t gpio_pin = firmata_pins[firmata_pin].gpio;

    switch (mode) {
    case PIN_MODE_INPUT:
        gpio_input(gpio_pin, GPIO_PULL_NONE);
        pin_mode[firmata_pin] = PIN_MODE_INPUT;
        pin_state[firmata_pin] = 0u;
        break;
    case PIN_MODE_OUTPUT:
        gpio_output(gpio_pin, false);
        pin_mode[firmata_pin] = PIN_MODE_OUTPUT;
        pin_state[firmata_pin] = 0u;
        break;
    case PIN_MODE_PULLUP:
        gpio_input(gpio_pin, GPIO_PULL_UP);
        pin_mode[firmata_pin] = PIN_MODE_PULLUP;
        pin_state[firmata_pin] = 1u;
        break;
    default:
        break;
    }
}

static void set_pin_value(uint8_t firmata_pin, uint8_t value)
{
    if (!firmata_pin_available(firmata_pin))
        return;
    if (pin_mode[firmata_pin] != PIN_MODE_OUTPUT)
        return;

    const bool high = value != 0u;
    gpio_write(firmata_pins[firmata_pin].gpio, high);
    pin_state[firmata_pin] = high ? 1u : 0u;
}

static void write_digital_port(uint8_t port, uint16_t value)
{
    const uint8_t first_pin = (uint8_t)(port * 8u);
    for (uint8_t bit = 0u; bit < 8u; ++bit) {
        const uint8_t firmata_pin = (uint8_t)(first_pin + bit);
        if (pin_mode[firmata_pin] == PIN_MODE_OUTPUT)
            set_pin_value(firmata_pin, (uint8_t)((value >> bit) & 1u));
    }
}

static void set_report_digital(uint8_t port, uint8_t enabled)
{
    report_ports[port] = enabled != 0u ? 1u : 0u;
    if (report_ports[port] != 0u) {
        const uint8_t value = read_port(port);
        previous_ports[port] = value;
        tx_clear();
        append_digital_port(port, value);
    }
}

static void reset_system(void)
{
    pending_command = 0u;
    pending_channel = 0u;
    pending_received = 0u;
    pending_needed = 0u;
    parsing_sysex = false;
    sysex_length = 0u;
    for (uint16_t firmata_pin = 0u; firmata_pin < FIRMATA_PIN_COUNT; ++firmata_pin) {
        if (firmata_pin_available((uint8_t)firmata_pin)) {
            gpio_input(firmata_pins[firmata_pin].gpio, GPIO_PULL_NONE);
            pin_mode[firmata_pin] = PIN_MODE_INPUT;
        } else {
            pin_mode[firmata_pin] = PIN_MODE_IGNORE;
        }
        pin_state[firmata_pin] = 0u;
    }
    for (uint8_t port = 0u; port < FIRMATA_PORT_COUNT; ++port) {
        report_ports[port] = 0u;
        previous_ports[port] = 0u;
    }
}

static void process_sysex(void)
{
    if (sysex_length == 0u)
        return;

    const uint8_t command = sysex_data[0];
    switch (command) {
    case REPORT_FIRMWARE:
        tx_clear();
        append_firmware_report();
        break;
    case CAPABILITY_QUERY:
        queue_capability_response();
        break;
    case ANALOG_MAPPING_QUERY:
        queue_analog_mapping_response();
        break;
    case PIN_STATE_QUERY:
        if (sysex_length >= 2u)
            queue_pin_state_response(sysex_data[1]);
        break;
    default:
        break;
    }
}

static void execute_pending(void)
{
    switch (pending_command) {
    case DIGITAL_MESSAGE:
        write_digital_port(pending_channel,
                           (uint16_t)pending_data[0] |
                           ((uint16_t)pending_data[1] << 7));
        break;
    case SET_PIN_MODE:
        set_pin_mode(pending_data[0], pending_data[1]);
        break;
    case SET_DIGITAL_PIN_VALUE:
        set_pin_value(pending_data[0], pending_data[1]);
        break;
    case REPORT_DIGITAL:
        set_report_digital(pending_channel, pending_data[0]);
        break;
    default:
        break;
    }
    pending_command = 0u;
    pending_received = 0u;
    pending_needed = 0u;
}

static void begin_pending(uint8_t command, uint8_t channel, uint8_t needed)
{
    pending_command = command;
    pending_channel = channel;
    pending_received = 0u;
    pending_needed = needed;
}

static void parse_command(uint8_t input)
{
    uint8_t command = input;
    uint8_t channel = 0u;
    if (input < 0xf0u) {
        command = input & 0xf0u;
        channel = input & 0x0fu;
    }

    switch (command) {
    case DIGITAL_MESSAGE:
    case ANALOG_MESSAGE:
        begin_pending(command, channel, 2u);
        break;
    case REPORT_DIGITAL:
    case REPORT_ANALOG:
        begin_pending(command, channel, 1u);
        break;
    case SET_PIN_MODE:
    case SET_DIGITAL_PIN_VALUE:
        begin_pending(command, 0u, 2u);
        break;
    case START_SYSEX:
        parsing_sysex = true;
        sysex_length = 0u;
        break;
    case REPORT_VERSION:
        queue_protocol_version();
        break;
    default:
        break;
    }
}

static void parse_byte(uint8_t input)
{
    if (input == SYSTEM_RESET) {
        reset_system();
        return;
    }

    if (parsing_sysex) {
        if (input == END_SYSEX) {
            parsing_sysex = false;
            process_sysex();
            sysex_length = 0u;
            return;
        }
        if (input == START_SYSEX) {
            sysex_length = 0u;
            return;
        }
        if ((input & 0x80u) != 0u) {
            parsing_sysex = false;
            sysex_length = 0u;
            parse_command(input);
            return;
        }
        if (sysex_length < FIRMATA_SYSEX_CAPACITY)
            sysex_data[sysex_length++] = input;
        else {
            parsing_sysex = false;
            sysex_length = 0u;
        }
        return;
    }

    if (pending_needed != 0u && input < 0x80u) {
        pending_data[pending_received++] = input;
        if (pending_received == pending_needed)
            execute_pending();
        return;
    }

    if ((input & 0x80u) != 0u) {
        pending_command = 0u;
        pending_received = 0u;
        pending_needed = 0u;
        parse_command(input);
    }
}

static void report_digital_inputs(void)
{
    tx_clear();
    for (uint8_t port = 0u; port < FIRMATA_PORT_COUNT; ++port) {
        if (report_ports[port] == 0u)
            continue;

        const uint8_t value = read_port(port);
        if (value != previous_ports[port]) {
            previous_ports[port] = value;
            append_digital_port(port, value);
        }
    }
}

void firmata_init(void)
{
    usb_cdc_init();
    reset_system();
    queue_protocol_version();
    append_firmware_report();
}

void firmata_task(void)
{
    tx_flush();
    if (tx_pending())
        return;

    while (usb_cdc_available() != 0u && !tx_pending()) {
        uint8_t input;
        if (usb_cdc_read(&input, 1u) != 1u)
            break;
        parse_byte(input);
    }

    if (!tx_pending())
        report_digital_inputs();
    tx_flush();
}
