#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "gpio.h"
#include "protocol.h"
#include "usb_cdc.h"

#define WIRE_PIN_COUNT 117u
#define RX_CAPACITY 64u
#define TOKEN_CAPACITY 6u
#define TX_CAPACITY 64u

#define WIRE_PB8 40u
#define WIRE_PB9 41u
#define WIRE_PB10 42u
#define WIRE_PB11 43u
#define WIRE_PC0 47u
#define WIRE_PD0 79u
#define WIRE_PE0 111u

typedef enum {
    PIN_UNSET = 0,
    PIN_INPUT,
    PIN_OUTPUT,
} pin_direction_t;

typedef struct {
    pin_direction_t direction;
    bool pullup;
    bool listening;
    bool previous_value;
    uint16_t listener_id;
} pin_state_t;

static pin_state_t pins[WIRE_PIN_COUNT];

static char rx_buffer[RX_CAPACITY];
static size_t rx_length;
static bool rx_discarding;

static uint8_t tx_buffer[TX_CAPACITY];
static size_t tx_length;
static size_t tx_offset;

static bool text_equal(const char *left, const char *right)
{
    while (*left != '\0' && *right != '\0') {
        if (*left != *right)
            return false;
        ++left;
        ++right;
    }
    return *left == *right;
}

static bool parse_decimal(const char *text, uint16_t *value)
{
    uint16_t result = 0u;

    if (*text == '\0')
        return false;

    while (*text != '\0') {
        if (*text < '0' || *text > '9')
            return false;
        const uint16_t digit = (uint16_t)(*text - '0');
        if (result > 99u)
            return false;
        result = (uint16_t)(result * 10u + digit);
        ++text;
    }

    *value = result;
    return true;
}

static size_t split_tokens(char *line, char **tokens)
{
    size_t count = 0u;
    char *cursor = line;

    while (*cursor != '\0') {
        while (*cursor == ' ' || *cursor == '\t')
            ++cursor;
        if (*cursor == '\0')
            break;
        if (count == TOKEN_CAPACITY)
            return TOKEN_CAPACITY + 1u;

        tokens[count++] = cursor;
        while (*cursor != '\0' && *cursor != ' ' && *cursor != '\t')
            ++cursor;
        if (*cursor != '\0')
            *cursor++ = '\0';
    }

    return count;
}

static bool wire_pin_supported(uint8_t pin)
{
    if (pin >= WIRE_PIN_COUNT)
        return false;
    return pin != WIRE_PB8 && pin != WIRE_PB9 && pin != WIRE_PB10 &&
           pin != WIRE_PB11;
}

static gpio_pin_t wire_pin_to_gpio(uint8_t pin)
{
    if (pin < WIRE_PC0)
        return (gpio_pin_t)pin;
    if (pin < WIRE_PD0)
        return (gpio_pin_t)(PIO_PC0_IDX + (uint16_t)(pin - WIRE_PC0));
    if (pin < WIRE_PE0)
        return (gpio_pin_t)(PIO_PD0_IDX + (uint16_t)(pin - WIRE_PD0));
    return (gpio_pin_t)(PIO_PE0_IDX + (uint16_t)(pin - WIRE_PE0));
}

static bool parse_pin(const char *text, uint8_t *pin)
{
    uint16_t parsed;
    if (!parse_decimal(text, &parsed) || parsed >= WIRE_PIN_COUNT)
        return false;
    *pin = (uint8_t)parsed;
    return true;
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
    if (tx_length < TX_CAPACITY)
        tx_buffer[tx_length++] = value;
}

static void tx_text(const char *text)
{
    while (*text != '\0')
        tx_byte((uint8_t)*text++);
}

static void tx_decimal3(uint16_t value)
{
    tx_byte((uint8_t)('0' + (value / 100u) % 10u));
    tx_byte((uint8_t)('0' + (value / 10u) % 10u));
    tx_byte((uint8_t)('0' + value % 10u));
}

static void tx_begin(uint16_t packet_id)
{
    tx_clear();
    tx_decimal3(packet_id);
}

static void tx_finish(void)
{
    tx_text(" <3\n");
}

static void tx_flush(void)
{
    if (!tx_pending())
        return;

    tx_offset += usb_cdc_write(&tx_buffer[tx_offset], tx_length - tx_offset);
    if (tx_offset == tx_length)
        tx_clear();
}

static void queue_simple(uint16_t packet_id, const char *command)
{
    tx_begin(packet_id);
    tx_byte((uint8_t)' ');
    tx_text(command);
    tx_finish();
}

static void queue_ack(uint16_t packet_id)
{
    queue_simple(packet_id, "OKA");
}

static void queue_bad_packet(uint16_t packet_id)
{
    tx_begin(packet_id);
    tx_text(" UMM BAD_PACKET");
    tx_finish();
}

static void queue_pin_error(uint16_t packet_id, uint8_t pin, const char *reason)
{
    tx_begin(packet_id);
    tx_text(" UMM ");
    tx_decimal3(pin);
    tx_byte((uint8_t)' ');
    tx_text(reason);
    tx_finish();
}

static void queue_pin_value(uint16_t packet_id, uint8_t pin, bool high)
{
    tx_begin(packet_id);
    tx_text(" HYG ");
    tx_decimal3(pin);
    tx_byte((uint8_t)' ');
    tx_text(high ? "HIGH" : "LOW");
    tx_finish();
}

static void queue_pin_state(uint16_t packet_id, uint8_t pin, const char *kind,
                            const char *value)
{
    tx_begin(packet_id);
    tx_text(" HYG ");
    tx_decimal3(pin);
    tx_byte((uint8_t)' ');
    tx_text(kind);
    tx_byte((uint8_t)' ');
    tx_text(value);
    tx_finish();
}

static bool valid_write_suffix(char **tokens, size_t count, size_t expected)
{
    return count == expected && text_equal(tokens[expected - 1u], "OK?");
}

static bool require_supported_pin(uint16_t packet_id, const char *text,
                                  uint8_t *pin)
{
    if (!parse_pin(text, pin)) {
        queue_bad_packet(packet_id);
        return false;
    }
    if (!wire_pin_supported(*pin)) {
        queue_pin_error(packet_id, *pin, "UNAVAILABLE");
        return false;
    }
    return true;
}

static bool require_initialized_pin(uint16_t packet_id, const char *text,
                                    uint8_t *pin)
{
    if (!require_supported_pin(packet_id, text, pin))
        return false;
    if (pins[*pin].direction == PIN_UNSET) {
        queue_pin_error(packet_id, *pin, "UNSET");
        return false;
    }
    return true;
}

static bool require_initialized_write(uint16_t packet_id, char **tokens,
                                      size_t count, size_t expected,
                                      uint8_t *pin)
{
    if (!valid_write_suffix(tokens, count, expected)) {
        queue_bad_packet(packet_id);
        return false;
    }
    return require_initialized_pin(packet_id, tokens[2], pin);
}

static void set_direction(uint8_t pin, pin_direction_t direction)
{
    const gpio_pin_t gpio = wire_pin_to_gpio(pin);

    if (direction == PIN_OUTPUT)
        gpio_output(gpio, false);
    else
        gpio_input(gpio, GPIO_PULL_NONE);

    pins[pin].direction = direction;
    pins[pin].pullup = false;
    pins[pin].previous_value = gpio_read(gpio);
}

static void set_pullup(uint8_t pin, bool enabled)
{
    pins[pin].pullup = enabled;
    if (pins[pin].direction == PIN_INPUT) {
        gpio_input(wire_pin_to_gpio(pin), enabled ? GPIO_PULL_UP : GPIO_PULL_NONE);
        pins[pin].previous_value = gpio_read(wire_pin_to_gpio(pin));
    }
}

static void reset_pins(void)
{
    for (uint8_t pin = 0u; pin < WIRE_PIN_COUNT; ++pin) {
        if (wire_pin_supported(pin) && pins[pin].direction != PIN_UNSET)
            gpio_input(wire_pin_to_gpio(pin), GPIO_PULL_NONE);
        pins[pin].direction = PIN_UNSET;
        pins[pin].pullup = false;
        pins[pin].listening = false;
        pins[pin].previous_value = false;
        pins[pin].listener_id = 0u;
    }
}

static void handle_direction(uint16_t packet_id, char **tokens, size_t count)
{
    uint8_t pin;

    if (!valid_write_suffix(tokens, count, 5u)) {
        queue_bad_packet(packet_id);
        return;
    }
    if (!require_supported_pin(packet_id, tokens[2], &pin))
        return;

    if (text_equal(tokens[3], "IN"))
        set_direction(pin, PIN_INPUT);
    else if (text_equal(tokens[3], "OUT"))
        set_direction(pin, PIN_OUTPUT);
    else {
        queue_bad_packet(packet_id);
        return;
    }

    queue_ack(packet_id);
}

static void handle_get(uint16_t packet_id, char **tokens, size_t count)
{
    uint8_t pin;
    if (!require_initialized_write(packet_id, tokens, count, 4u, &pin))
        return;

    queue_pin_value(packet_id, pin, gpio_read(wire_pin_to_gpio(pin)));
}

static void handle_set(uint16_t packet_id, char **tokens, size_t count)
{
    uint8_t pin;
    if (!require_initialized_write(packet_id, tokens, count, 5u, &pin))
        return;

    if (text_equal(tokens[3], "HIGH"))
        gpio_write(wire_pin_to_gpio(pin), true);
    else if (text_equal(tokens[3], "LOW"))
        gpio_write(wire_pin_to_gpio(pin), false);
    else {
        queue_bad_packet(packet_id);
        return;
    }

    queue_ack(packet_id);
}

static void handle_pullup(uint16_t packet_id, char **tokens, size_t count)
{
    uint8_t pin;
    if (!require_initialized_write(packet_id, tokens, count, 5u, &pin))
        return;

    if (text_equal(tokens[3], "ON"))
        set_pullup(pin, true);
    else if (text_equal(tokens[3], "OFF"))
        set_pullup(pin, false);
    else {
        queue_bad_packet(packet_id);
        return;
    }

    queue_ack(packet_id);
}

static void handle_listen(uint16_t packet_id, char **tokens, size_t count)
{
    uint8_t pin;
    if (!require_initialized_write(packet_id, tokens, count, 5u, &pin))
        return;

    if (text_equal(tokens[3], "ON")) {
        pins[pin].listening = true;
        pins[pin].listener_id = packet_id;
        pins[pin].previous_value = gpio_read(wire_pin_to_gpio(pin));
    } else if (text_equal(tokens[3], "OFF")) {
        pins[pin].listening = false;
        pins[pin].listener_id = 0u;
    } else {
        queue_bad_packet(packet_id);
        return;
    }

    queue_ack(packet_id);
}

static void handle_what_are_you_doing(uint16_t packet_id, char **tokens,
                                      size_t count)
{
    uint8_t pin;
    if (count != 4u || !require_supported_pin(packet_id, tokens[2], &pin)) {
        if (count != 4u)
            queue_bad_packet(packet_id);
        return;
    }

    if (text_equal(tokens[3], "DIR")) {
        const char *value = "UNSET";
        if (pins[pin].direction == PIN_INPUT)
            value = "IN";
        else if (pins[pin].direction == PIN_OUTPUT)
            value = "OUT";
        queue_pin_state(packet_id, pin, "DIR", value);
    } else if (text_equal(tokens[3], "PLL")) {
        queue_pin_state(packet_id, pin, "PLL",
                        pins[pin].direction == PIN_UNSET
                            ? "UNSET"
                            : pins[pin].pullup ? "ON" : "OFF");
    } else if (text_equal(tokens[3], "LSN")) {
        queue_pin_state(packet_id, pin, "LSN",
                        pins[pin].direction == PIN_UNSET
                            ? "UNSET"
                            : pins[pin].listening ? "ON" : "OFF");
    } else {
        queue_bad_packet(packet_id);
    }
}

static void process_line(char *line)
{
    char *tokens[TOKEN_CAPACITY];
    const size_t count = split_tokens(line, tokens);
    uint16_t packet_id;

    if (count < 2u || !parse_decimal(tokens[0], &packet_id))
        return;

    if (text_equal(tokens[1], "HAI")) {
        if (count == 2u)
            queue_simple(packet_id, "HII");
        else
            queue_bad_packet(packet_id);
    } else if (text_equal(tokens[1], "HRU")) {
        if (count == 2u) {
            tx_begin(packet_id);
            tx_text(" IAM SAM4E8E GPIO");
            tx_finish();
        } else {
            queue_bad_packet(packet_id);
        }
    } else if (text_equal(tokens[1], "DIR")) {
        handle_direction(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "GET")) {
        handle_get(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "SET")) {
        handle_set(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "PLL")) {
        handle_pullup(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "LSN")) {
        handle_listen(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "WYD")) {
        handle_what_are_you_doing(packet_id, tokens, count);
    } else if (text_equal(tokens[1], "BYE")) {
        if (count == 2u) {
            reset_pins();
            queue_simple(packet_id, "CYA");
        } else {
            queue_bad_packet(packet_id);
        }
    } else {
        queue_simple(packet_id, "IDK");
    }
}

static void parse_byte(uint8_t input)
{
    if (input == (uint8_t)'\r')
        return;

    if (input == (uint8_t)'\n') {
        if (!rx_discarding && rx_length != 0u) {
            rx_buffer[rx_length] = '\0';
            process_line(rx_buffer);
        }
        rx_length = 0u;
        rx_discarding = false;
        return;
    }

    if (rx_discarding)
        return;

    if (rx_length + 1u >= RX_CAPACITY) {
        rx_length = 0u;
        rx_discarding = true;
        return;
    }

    rx_buffer[rx_length++] = (char)input;
}

static void report_listener_change(void)
{
    for (uint8_t pin = 0u; pin < WIRE_PIN_COUNT; ++pin) {
        if (!pins[pin].listening)
            continue;

        const bool value = gpio_read(wire_pin_to_gpio(pin));
        if (value == pins[pin].previous_value)
            continue;

        pins[pin].previous_value = value;
        queue_pin_value(pins[pin].listener_id, pin, value);
        return;
    }
}

void gpio_protocol_init(void)
{
    usb_cdc_init();
    reset_pins();
    rx_length = 0u;
    rx_discarding = false;
    tx_clear();
}

void gpio_protocol_task(void)
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
        report_listener_change();
    tx_flush();
}
