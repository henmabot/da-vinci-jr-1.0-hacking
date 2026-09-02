#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "firmata.h"
#include "gpio.h"
#include "usb_cdc.h"

#define DIGITAL_MESSAGE 0x90u
#define REPORT_DIGITAL 0xd0u
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

#define TEST_BUFFER_SIZE 4096u
#define EXPECTED_CAPABILITY_LENGTH 773u

static uint8_t usb_rx[TEST_BUFFER_SIZE];
static size_t usb_rx_length;
static size_t usb_rx_offset;
static uint8_t usb_tx[TEST_BUFFER_SIZE];
static size_t usb_tx_length;
static bool usb_initialized;
static size_t usb_write_limit = SIZE_MAX;

static int gpio_direction[256];
static gpio_pull_t gpio_pull[256];
static bool gpio_level[256];
static unsigned gpio_configure_count[256];

bool gpio_valid(gpio_pin_t pin)
{
    const uint8_t port = pin / 32u;
    const uint8_t bit = pin % 32u;
    if (port == 0u || port == 2u || port == 3u)
        return true;
    if (port == 1u)
        return bit <= 14u;
    if (port == 4u)
        return bit <= 5u;
    return false;
}

void gpio_output(gpio_pin_t pin, bool initial_high)
{
    gpio_direction[pin] = 1;
    gpio_pull[pin] = GPIO_PULL_NONE;
    gpio_level[pin] = initial_high;
    ++gpio_configure_count[pin];
}

void gpio_input(gpio_pin_t pin, gpio_pull_t pull)
{
    gpio_direction[pin] = 0;
    gpio_pull[pin] = pull;
    ++gpio_configure_count[pin];
}

void gpio_write(gpio_pin_t pin, bool high)
{
    gpio_level[pin] = high;
}

bool gpio_read(gpio_pin_t pin)
{
    return gpio_level[pin];
}

void usb_cdc_init(void)
{
    usb_initialized = true;
}

size_t usb_cdc_write(const void *data, size_t length)
{
    if (length > usb_write_limit)
        length = usb_write_limit;
    assert(usb_tx_length + length <= sizeof(usb_tx));
    memcpy(&usb_tx[usb_tx_length], data, length);
    usb_tx_length += length;
    return length;
}

size_t usb_cdc_available(void)
{
    return usb_rx_length - usb_rx_offset;
}

size_t usb_cdc_read(void *data, size_t length)
{
    size_t available = usb_cdc_available();
    if (length > available)
        length = available;
    memcpy(data, &usb_rx[usb_rx_offset], length);
    usb_rx_offset += length;
    return length;
}

static void clear_tx(void)
{
    usb_tx_length = 0u;
}

static void feed(const uint8_t *data, size_t length)
{
    assert(length <= sizeof(usb_rx));
    memcpy(usb_rx, data, length);
    usb_rx_length = length;
    usb_rx_offset = 0u;
}

static void pump(void)
{
    for (unsigned i = 0u; i < 256u; ++i)
        firmata_task();
    assert(usb_cdc_available() == 0u);
}

static void expect_bytes(const uint8_t *expected, size_t length)
{
    assert(usb_tx_length == length);
    assert(memcmp(usb_tx, expected, length) == 0);
}

static size_t capability_pin_start(uint8_t pin)
{
    size_t offset = 2u;
    for (uint8_t current = 0u; current < pin; ++current) {
        while (usb_tx[offset] != 0x7fu)
            ++offset;
        ++offset;
    }
    return offset;
}

static void test_startup_reports(void)
{
    firmata_init();
    pump();

    assert(usb_initialized);
    assert(usb_tx_length > 8u);
    assert(usb_tx[0] == REPORT_VERSION);
    assert(usb_tx[1] == 2u);
    assert(usb_tx[2] == 8u);
    assert(usb_tx[3] == START_SYSEX);
    assert(usb_tx[4] == REPORT_FIRMWARE);
    assert(usb_tx[5] == 3u);
    assert(usb_tx[6] == 4u);
    assert(usb_tx[usb_tx_length - 1u] == END_SYSEX);

    assert(gpio_direction[GPIO_PD8] == 0);
    assert(gpio_pull[GPIO_PD8] == GPIO_PULL_NONE);
    assert(gpio_configure_count[GPIO_PB8] == 0u);
    assert(gpio_configure_count[GPIO_PB9] == 0u);
    assert(gpio_configure_count[GPIO_PB10] == 0u);
    assert(gpio_configure_count[GPIO_PB11] == 0u);
}

static void test_version_query(void)
{
    const uint8_t request[] = {REPORT_VERSION};
    const uint8_t expected[] = {REPORT_VERSION, 2u, 8u};
    clear_tx();
    feed(request, sizeof(request));
    pump();
    expect_bytes(expected, sizeof(expected));
}

static void test_firmware_query(void)
{
    const uint8_t request[] = {START_SYSEX, REPORT_FIRMWARE, END_SYSEX};
    clear_tx();
    feed(request, sizeof(request));
    pump();

    assert(usb_tx_length > 5u);
    assert(usb_tx[0] == START_SYSEX);
    assert(usb_tx[1] == REPORT_FIRMWARE);
    assert(usb_tx[2] == 3u);
    assert(usb_tx[3] == 4u);
    static const char expected_name[] = "ConfigurableFirmata";
    assert(usb_tx_length == 5u + 2u * (sizeof(expected_name) - 1u));
    for (size_t i = 0u; i < sizeof(expected_name) - 1u; ++i) {
        assert(usb_tx[4u + 2u * i] == ((uint8_t)expected_name[i] & 0x7fu));
        assert(usb_tx[5u + 2u * i] == ((uint8_t)expected_name[i] >> 7));
    }
    assert(usb_tx[usb_tx_length - 1u] == END_SYSEX);
}

static void test_pin_mode_and_writes(void)
{
    const uint8_t pullup_mode[] = {SET_PIN_MODE, GPIO_PD8, PIN_MODE_PULLUP};
    clear_tx();
    feed(pullup_mode, sizeof(pullup_mode));
    pump();
    assert(gpio_direction[GPIO_PD8] == 0);
    assert(gpio_pull[GPIO_PD8] == GPIO_PULL_UP);

    const uint8_t output_mode[] = {SET_PIN_MODE, GPIO_PD23, PIN_MODE_OUTPUT};
    clear_tx();
    feed(output_mode, sizeof(output_mode));
    pump();
    assert(gpio_direction[GPIO_PD23] == 1);
    assert(!gpio_level[GPIO_PD23]);

    const uint8_t set_high[] = {SET_DIGITAL_PIN_VALUE, GPIO_PD23, 1u};
    feed(set_high, sizeof(set_high));
    pump();
    assert(gpio_level[GPIO_PD23]);

    const uint8_t port_low[] = {(uint8_t)(DIGITAL_MESSAGE | 14u), 0u, 0u};
    feed(port_low, sizeof(port_low));
    pump();
    assert(!gpio_level[GPIO_PD23]);

    const uint8_t port_high[] = {(uint8_t)(DIGITAL_MESSAGE | 14u), 0u, 1u};
    feed(port_high, sizeof(port_high));
    pump();
    assert(gpio_level[GPIO_PD23]);

    const uint8_t reserved_mode[] = {SET_PIN_MODE, GPIO_PB10, PIN_MODE_OUTPUT};
    const unsigned before = gpio_configure_count[GPIO_PB10];
    feed(reserved_mode, sizeof(reserved_mode));
    pump();
    assert(gpio_configure_count[GPIO_PB10] == before);
}

static void test_pin_state_query(void)
{
    const uint8_t request[] = {START_SYSEX, PIN_STATE_QUERY, GPIO_PD23, END_SYSEX};
    const uint8_t expected[] = {
        START_SYSEX, PIN_STATE_RESPONSE, GPIO_PD23,
        PIN_MODE_OUTPUT, 1u, END_SYSEX,
    };
    clear_tx();
    feed(request, sizeof(request));
    pump();
    expect_bytes(expected, sizeof(expected));
}

static void test_digital_reporting(void)
{
    const uint8_t input_mode[] = {SET_PIN_MODE, GPIO_PD8, PIN_MODE_INPUT};
    feed(input_mode, sizeof(input_mode));
    pump();

    gpio_level[GPIO_PD8] = true;
    const uint8_t enable[] = {(uint8_t)(REPORT_DIGITAL | 13u), 1u};
    const uint8_t high_report[] = {(uint8_t)(DIGITAL_MESSAGE | 13u), 1u, 0u};
    clear_tx();
    feed(enable, sizeof(enable));
    pump();
    expect_bytes(high_report, sizeof(high_report));

    clear_tx();
    firmata_task();
    assert(usb_tx_length == 0u);

    gpio_level[GPIO_PD8] = false;
    firmata_task();
    const uint8_t low_report[] = {(uint8_t)(DIGITAL_MESSAGE | 13u), 0u, 0u};
    expect_bytes(low_report, sizeof(low_report));
}

static void test_multiport_reporting(void)
{
    const uint8_t enable_port0[] = {REPORT_DIGITAL, 1u};
    gpio_level[0] = false;
    feed(enable_port0, sizeof(enable_port0));
    pump();

    clear_tx();
    gpio_level[0] = true;
    gpio_level[GPIO_PD8] = true;
    firmata_task();

    const uint8_t expected[] = {
        DIGITAL_MESSAGE, 1u, 0u,
        (uint8_t)(DIGITAL_MESSAGE | 13u), 1u, 0u,
    };
    expect_bytes(expected, sizeof(expected));
}

static void test_capabilities(void)
{
    const uint8_t request[] = {START_SYSEX, CAPABILITY_QUERY, END_SYSEX};
    clear_tx();
    feed(request, sizeof(request));
    pump();

    assert(usb_tx[0] == START_SYSEX);
    assert(usb_tx_length == EXPECTED_CAPABILITY_LENGTH);
    assert(usb_tx[1] == CAPABILITY_RESPONSE);
    assert(usb_tx[usb_tx_length - 1u] == END_SYSEX);

    size_t offset = capability_pin_start(GPIO_PB8);
    assert(usb_tx[offset] == 0x7fu);

    offset = capability_pin_start(GPIO_PD23);
    const uint8_t modes[] = {
        PIN_MODE_INPUT, 1u,
        PIN_MODE_OUTPUT, 1u,
        PIN_MODE_PULLUP, 1u,
        0x7fu,
    };
    assert(memcmp(&usb_tx[offset], modes, sizeof(modes)) == 0);
}

static void test_analog_mapping(void)
{
    const uint8_t request[] = {START_SYSEX, ANALOG_MAPPING_QUERY, END_SYSEX};
    clear_tx();
    feed(request, sizeof(request));
    pump();

    assert(usb_tx_length == 131u);
    assert(usb_tx[0] == START_SYSEX);
    assert(usb_tx[1] == ANALOG_MAPPING_RESPONSE);
    for (size_t i = 2u; i < 130u; ++i)
        assert(usb_tx[i] == 0x7fu);
    assert(usb_tx[130] == END_SYSEX);
}

static void test_partial_usb_writes(void)
{
    const uint8_t request[] = {START_SYSEX, CAPABILITY_QUERY, END_SYSEX};
    clear_tx();
    usb_write_limit = 7u;
    feed(request, sizeof(request));
    pump();
    usb_write_limit = SIZE_MAX;
    assert(usb_tx_length == EXPECTED_CAPABILITY_LENGTH);

    assert(usb_tx[0] == START_SYSEX);
    assert(usb_tx[1] == CAPABILITY_RESPONSE);
    assert(usb_tx[usb_tx_length - 1u] == END_SYSEX);
}

static void test_system_reset(void)
{
    const uint8_t output_mode[] = {SET_PIN_MODE, GPIO_PD23, PIN_MODE_OUTPUT};
    feed(output_mode, sizeof(output_mode));
    pump();
    assert(gpio_direction[GPIO_PD23] == 1);

    const uint8_t reset[] = {SYSTEM_RESET};
    clear_tx();
    feed(reset, sizeof(reset));
    pump();
    assert(gpio_direction[GPIO_PD23] == 0);
    assert(gpio_pull[GPIO_PD23] == GPIO_PULL_NONE);
    assert(usb_tx_length == 0u);
}

int main(void)
{
    for (size_t i = 0u; i < 256u; ++i)
        gpio_direction[i] = -1;

    test_startup_reports();
    test_version_query();
    test_firmware_query();
    test_pin_mode_and_writes();
    test_pin_state_query();
    test_digital_reporting();
    test_multiport_reporting();
    test_capabilities();
    test_analog_mapping();
    test_partial_usb_writes();
    test_system_reset();

    puts("all firmata tests passed");
    return 0;
}
