// USB CDC ACM for ATSAM4E8E.
//
// The UDP register handling and endpoint-0 state machine are adapted from
// Klipper src/atsam/sam4_usb.c and src/generic/usb_cdc.c.
// Copyright (C) 2018 Kevin O'Connor <kevin@koconnor.net>
//
// This file may be distributed under the terms of the GNU GPLv3 license.

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "sam4e8e.h"
#include "usb_cdc.h"

#define USB_EP0 0u
#define USB_EP_BULK_IN 1u
#define USB_EP_BULK_OUT 2u
#define USB_EP_ACM 3u

#define USB_EP0_SIZE 16u
#define USB_BULK_SIZE 64u

#define CSR_EP0 (UDP_CSR_EPTYPE_CTRL | UDP_CSR_EPEDS)
#define CSR_ACM (UDP_CSR_EPTYPE_INT_IN | UDP_CSR_EPEDS)
#define CSR_BULK_OUT (UDP_CSR_EPTYPE_BULK_OUT | UDP_CSR_EPEDS)
#define CSR_BULK_IN (UDP_CSR_EPTYPE_BULK_IN | UDP_CSR_EPEDS)

#define USB_DIR_IN 0x80u
#define USB_REQ_GET_DESCRIPTOR 0x06u
#define USB_REQ_SET_ADDRESS 0x05u
#define USB_REQ_SET_CONFIGURATION 0x09u
#define USB_CDC_REQ_SET_LINE_CODING 0x20u
#define USB_CDC_REQ_GET_LINE_CODING 0x21u
#define USB_CDC_REQ_SET_CONTROL_LINE_STATE 0x22u
#define USB_DT_DEVICE 0x01u
#define USB_DT_CONFIG 0x02u

#define USB_BUFFER_SIZE 256u
#define USB_BUFFER_MASK (USB_BUFFER_SIZE - 1u)

struct usb_ctrlrequest {
    uint8_t bRequestType;
    uint8_t bRequest;
    uint16_t wValue;
    uint16_t wIndex;
    uint16_t wLength;
} __attribute__((packed));

struct usb_cdc_line_coding {
    uint32_t dwDTERate;
    uint8_t bCharFormat;
    uint8_t bParityType;
    uint8_t bDataBits;
} __attribute__((packed));

static const uint8_t device_descriptor[] = {
    18, USB_DT_DEVICE,
    0x00, 0x02,
    0x02, 0x00, 0x00,
    USB_EP0_SIZE,
    0x50, 0x1d,
    0x4e, 0x61,
    0x00, 0x01,
    0x00, 0x00, 0x00,
    0x01,
};

static const uint8_t config_descriptor[] = {
    9, USB_DT_CONFIG, 62, 0, 2, 1, 0, 0xc0, 50,
    9, 4, 0, 0, 1, 0x02, 0x02, 0x01, 0,
    5, 0x24, 0x00, 0x10, 0x01,
    4, 0x24, 0x02, 0x06,
    5, 0x24, 0x06, 0, 1,
    7, 5, 0x80 | USB_EP_ACM, 0x03, 8, 0, 255,
    9, 4, 1, 0, 2, 0x0a, 0x00, 0x00, 0,
    7, 5, USB_EP_BULK_OUT, 0x02, USB_BULK_SIZE, 0, 0,
    7, 5, 0x80 | USB_EP_BULK_IN, 0x02, USB_BULK_SIZE, 0, 0,
};

static uint8_t tx_buffer[USB_BUFFER_SIZE];
static volatile uint16_t tx_head;
static volatile uint16_t tx_tail;
static uint8_t rx_buffer[USB_BUFFER_SIZE];
static volatile uint16_t rx_head;
static volatile uint16_t rx_tail;
static uint32_t rx_next_bank = UDP_CSR_RX_DATA_BK0;
static volatile bool configured;

static struct usb_cdc_line_coding line_coding = {
    .dwDTERate = 115200u,
    .bCharFormat = 0u,
    .bParityType = 0u,
    .bDataBits = 8u,
};

static uint32_t pending_address;

enum {
    UX_READ = 1u << 0,
    UX_SEND = 1u << 1,
    UX_SEND_ZLP = 1u << 2,
};

static uint8_t *control_data;
static uint16_t control_size;
static uint8_t control_flags;

static uint32_t irq_save(void)
{
    const uint32_t state = __get_PRIMASK();
    __disable_irq();
    return state;
}

static void irq_restore(uint32_t state)
{
    __set_PRIMASK(state);
}

static void usb_write_packet(uint32_t endpoint, const uint8_t *data, uint32_t length)
{
    while (length-- != 0u)
        UDP->UDP_FDR[endpoint] = *data++;
}

static uint32_t usb_packet_length(uint32_t csr)
{
    return (csr & UDP_CSR_RXBYTECNT_Msk) >> UDP_CSR_RXBYTECNT_Pos;
}

static uint32_t usb_read_packet(uint32_t endpoint, uint32_t csr, uint8_t *data, uint32_t max_length)
{
    const uint32_t packet_length = usb_packet_length(csr);
    const uint32_t length = packet_length < max_length ? packet_length : max_length;

    for (uint32_t i = 0; i < length; ++i)
        data[i] = UDP->UDP_FDR[endpoint];
    return length;
}

static int usb_read_ep0(void *data, uint32_t max_length)
{
    const uint32_t other_irqs = UDP_CSR_RXSETUP | UDP_CSR_STALLSENT |
                                UDP_CSR_TXCOMP | UDP_CSR_RX_DATA_BK1;
    const uint32_t csr = UDP->UDP_CSR[USB_EP0];

    if ((csr & other_irqs) != 0u)
        return -2;
    if ((csr & UDP_CSR_RX_DATA_BK0) == 0u) {
        UDP->UDP_IER = 1u << USB_EP0;
        return -1;
    }

    const uint32_t length = usb_read_packet(USB_EP0, csr, data, max_length);
    if ((UDP->UDP_CSR[USB_EP0] & other_irqs) != 0u)
        return -2;
    UDP->UDP_CSR[USB_EP0] = CSR_EP0 | other_irqs;
    return (int)length;
}

static int usb_read_ep0_setup(void *data, uint32_t max_length)
{
    const uint32_t other_irqs = UDP_CSR_STALLSENT | UDP_CSR_TXCOMP |
                                UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1;
    const uint32_t csr = UDP->UDP_CSR[USB_EP0];

    if ((csr & UDP_CSR_RXSETUP) == 0u) {
        if ((csr & other_irqs) != 0u)
            UDP->UDP_CSR[USB_EP0] = CSR_EP0 | UDP_CSR_RXSETUP;
        UDP->UDP_IER = 1u << USB_EP0;
        return -1;
    }

    const uint32_t length = usb_read_packet(USB_EP0, csr, data, max_length);
    const uint32_t direction = (*(uint8_t *)data & USB_DIR_IN) != 0u ? UDP_CSR_DIR : 0u;
    UDP->UDP_CSR[USB_EP0] = CSR_EP0 | direction;
    return (int)length;
}

static int usb_send_ep0(const void *data, uint32_t length)
{
    const uint32_t other_irqs = UDP_CSR_RXSETUP | UDP_CSR_STALLSENT |
                                UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1;
    const uint32_t csr = UDP->UDP_CSR[USB_EP0];

    if ((csr & other_irqs) != 0u)
        return -2;
    if ((csr & UDP_CSR_TXPKTRDY) != 0u) {
        UDP->UDP_IER = 1u << USB_EP0;
        return -1;
    }

    usb_write_packet(USB_EP0, data, length);
    UDP->UDP_CSR[USB_EP0] = CSR_EP0 | (csr & UDP_CSR_DIR) |
                            UDP_CSR_TXPKTRDY | other_irqs;
    return (int)length;
}

static void usb_stall_ep0(void)
{
    UDP->UDP_CSR[USB_EP0] = CSR_EP0 | UDP_CSR_FORCESTALL;
    UDP->UDP_IER = 1u << USB_EP0;
    control_flags = 0u;
}

static void usb_tx_kick(void)
{
    uint32_t csr = UDP->UDP_CSR[USB_EP_BULK_IN];

    if (tx_tail == tx_head) {
        if ((csr & UDP_CSR_TXCOMP) != 0u)
            UDP->UDP_CSR[USB_EP_BULK_IN] = CSR_BULK_IN;
        return;
    }
    if ((csr & UDP_CSR_TXPKTRDY) != 0u) {
        UDP->UDP_IER = 1u << USB_EP_BULK_IN;
        return;
    }

    uint32_t count = (tx_head - tx_tail) & USB_BUFFER_MASK;
    if (count > USB_BULK_SIZE)
        count = USB_BULK_SIZE;

    for (uint32_t i = 0; i < count; ++i) {
        UDP->UDP_FDR[USB_EP_BULK_IN] = tx_buffer[tx_tail];
        tx_tail = (tx_tail + 1u) & USB_BUFFER_MASK;
    }
    UDP->UDP_CSR[USB_EP_BULK_IN] = CSR_BULK_IN | UDP_CSR_TXPKTRDY;

    if (tx_tail != tx_head)
        UDP->UDP_IER = 1u << USB_EP_BULK_IN;
}

static void usb_rx_kick(void)
{
    const uint32_t other_irqs = UDP_CSR_RXSETUP | UDP_CSR_STALLSENT |
                                UDP_CSR_TXCOMP;

    for (;;) {
        const uint32_t csr = UDP->UDP_CSR[USB_EP_BULK_OUT];
        uint32_t bank = csr & (UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1);

        if (bank == 0u) {
            if ((csr & other_irqs) != 0u) {
                UDP->UDP_CSR[USB_EP_BULK_OUT] = CSR_BULK_OUT |
                    UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1;
            }
            UDP->UDP_IER = 1u << USB_EP_BULK_OUT;
            return;
        }

        const uint32_t length = usb_packet_length(csr);
        if (length > ((rx_tail - rx_head - 1u) & USB_BUFFER_MASK))
            return;

        for (uint32_t i = 0; i < length; ++i) {
            rx_buffer[rx_head] = UDP->UDP_FDR[USB_EP_BULK_OUT];
            rx_head = (rx_head + 1u) & USB_BUFFER_MASK;
        }

        if (bank == (UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1))
            bank = rx_next_bank;
        rx_next_bank = bank ^ (UDP_CSR_RX_DATA_BK0 | UDP_CSR_RX_DATA_BK1);
        UDP->UDP_CSR[USB_EP_BULK_OUT] = CSR_BULK_OUT | rx_next_bank;
    }
}

static void usb_set_address(uint16_t address)
{
    pending_address = address | UDP_FADDR_FEN;
    (void)usb_send_ep0(0, 0u);
    UDP->UDP_IER = 1u << USB_EP0;
}

static void usb_set_configuration(void)
{
    UDP->UDP_CSR[USB_EP_ACM] = CSR_ACM;
    UDP->UDP_CSR[USB_EP_BULK_OUT] = CSR_BULK_OUT;
    UDP->UDP_CSR[USB_EP_BULK_IN] = CSR_BULK_IN;
    UDP->UDP_GLB_STAT |= UDP_GLB_STAT_CONFG;
    configured = true;
    usb_rx_kick();
    usb_tx_kick();
}

static void usb_do_xfer(void *data, uint16_t size, uint8_t flags)
{
    uint8_t *position = data;

    for (;;) {
        uint16_t packet_size = size;
        if (packet_size > USB_EP0_SIZE)
            packet_size = USB_EP0_SIZE;

        int result;
        if ((flags & UX_READ) != 0u)
            result = usb_read_ep0(position, packet_size);
        else
            result = usb_send_ep0(position, packet_size);

        if (result == (int)packet_size) {
            position += packet_size;
            size -= packet_size;
            if (size == 0u) {
                if ((flags & UX_READ) != 0u) {
                    flags = UX_SEND;
                    continue;
                }
                if (packet_size == USB_EP0_SIZE && (flags & UX_SEND_ZLP) != 0u)
                    continue;
                control_flags = 0u;
                UDP->UDP_IER = 1u << USB_EP0;
                return;
            }
            continue;
        }

        if (result == -1) {
            control_data = position;
            control_size = size;
            control_flags = flags;
            return;
        }

        usb_stall_ep0();
        return;
    }
}

static void usb_req_get_descriptor(const struct usb_ctrlrequest *request)
{
    const uint8_t *descriptor = 0;
    uint16_t size = 0u;

    if (request->bRequestType != USB_DIR_IN || request->wIndex != 0u) {
        usb_stall_ep0();
        return;
    }

    if (request->wValue == (USB_DT_DEVICE << 8)) {
        descriptor = device_descriptor;
        size = sizeof(device_descriptor);
    } else if (request->wValue == (USB_DT_CONFIG << 8)) {
        descriptor = config_descriptor;
        size = sizeof(config_descriptor);
    } else {
        usb_stall_ep0();
        return;
    }

    uint8_t flags = UX_SEND;
    if (size > request->wLength)
        size = request->wLength;
    else if (size < request->wLength)
        flags |= UX_SEND_ZLP;
    usb_do_xfer((void *)descriptor, size, flags);
}

static void usb_req_set_address(const struct usb_ctrlrequest *request)
{
    if (request->bRequestType != 0u || request->wIndex != 0u || request->wLength != 0u) {
        usb_stall_ep0();
        return;
    }
    usb_set_address(request->wValue);
}

static void usb_req_set_configuration(const struct usb_ctrlrequest *request)
{
    if (request->bRequestType != 0u || request->wValue != 1u ||
        request->wIndex != 0u || request->wLength != 0u) {
        usb_stall_ep0();
        return;
    }
    usb_set_configuration();
    usb_do_xfer(0, 0u, UX_SEND);
}

static void usb_req_set_line_coding(const struct usb_ctrlrequest *request)
{
    if (request->bRequestType != 0x21u || request->wValue != 0u ||
        request->wIndex != 0u || request->wLength != sizeof(line_coding)) {
        usb_stall_ep0();
        return;
    }
    usb_do_xfer(&line_coding, sizeof(line_coding), UX_READ);
}

static void usb_req_get_line_coding(const struct usb_ctrlrequest *request)
{
    if (request->bRequestType != 0xa1u || request->wValue != 0u ||
        request->wIndex != 0u || request->wLength < sizeof(line_coding)) {
        usb_stall_ep0();
        return;
    }
    usb_do_xfer(&line_coding, sizeof(line_coding), UX_SEND);
}

static void usb_req_set_control_line_state(const struct usb_ctrlrequest *request)
{
    if (request->bRequestType != 0x21u || request->wIndex != 0u || request->wLength != 0u) {
        usb_stall_ep0();
        return;
    }
    usb_do_xfer(0, 0u, UX_SEND);
}

static void usb_control_ready(void)
{
    struct usb_ctrlrequest request;
    if (usb_read_ep0_setup(&request, sizeof(request)) != (int)sizeof(request))
        return;

    switch (request.bRequest) {
    case USB_REQ_GET_DESCRIPTOR:
        usb_req_get_descriptor(&request);
        break;
    case USB_REQ_SET_ADDRESS:
        usb_req_set_address(&request);
        break;
    case USB_REQ_SET_CONFIGURATION:
        usb_req_set_configuration(&request);
        break;
    case USB_CDC_REQ_SET_LINE_CODING:
        usb_req_set_line_coding(&request);
        break;
    case USB_CDC_REQ_GET_LINE_CODING:
        usb_req_get_line_coding(&request);
        break;
    case USB_CDC_REQ_SET_CONTROL_LINE_STATE:
        usb_req_set_control_line_state(&request);
        break;
    default:
        usb_stall_ep0();
        break;
    }
}

static void usb_control_handle(void)
{
    if (control_flags != 0u)
        usb_do_xfer(control_data, control_size, control_flags);
    else
        usb_control_ready();
}

static void usb_handle_bus_reset(void)
{
    UDP->UDP_ICR = UDP_ISR_ENDBUSRES;

    configured = false;
    pending_address = 0u;
    control_flags = 0u;
    tx_head = 0u;
    tx_tail = 0u;
    rx_head = 0u;
    rx_tail = 0u;
    rx_next_bank = UDP_CSR_RX_DATA_BK0;

    UDP->UDP_CSR[USB_EP0] = CSR_EP0;
    UDP->UDP_IER = 1u << USB_EP0;
    UDP->UDP_TXVC = UDP_TXVC_PUON;
}

void UDP_Handler(void)
{
    const uint32_t status = UDP->UDP_ISR;
    UDP->UDP_IDR = status;

    if ((status & UDP_ISR_ENDBUSRES) != 0u)
        usb_handle_bus_reset();
    if ((status & UDP_ISR_RXRSM) != 0u)
        UDP->UDP_ICR = UDP_ISR_RXRSM;

    if ((status & (1u << USB_EP0)) != 0u) {
        if (pending_address != 0u && (UDP->UDP_CSR[USB_EP0] & UDP_CSR_TXCOMP) != 0u) {
            UDP->UDP_FADDR = pending_address;
            UDP->UDP_GLB_STAT |= UDP_GLB_STAT_FADDEN;
            pending_address = 0u;
        }
        usb_control_handle();
    }
    if ((status & (1u << USB_EP_BULK_OUT)) != 0u)
        usb_rx_kick();
    if ((status & (1u << USB_EP_BULK_IN)) != 0u)
        usb_tx_kick();
}

void usb_cdc_init(void)
{
    configured = false;
    tx_head = 0u;
    tx_tail = 0u;
    rx_head = 0u;
    rx_tail = 0u;

    PMC->PMC_PCER1 = 1u << (ID_UDP - 32u);
    PMC->PMC_USB = PMC_USB_USBDIV(5u - 1u);
    PMC->PMC_SCER = PMC_SCER_UDP;

    UDP->UDP_TXVC = UDP_TXVC_PUON | UDP_TXVC_TXVDIS;
    UDP->UDP_ICR = 0xffffffffu;

    NVIC_ClearPendingIRQ(UDP_IRQn);
    NVIC_SetPriority(UDP_IRQn, 1u);
    NVIC_EnableIRQ(UDP_IRQn);
}

bool usb_cdc_ready(void)
{
    return configured;
}

size_t usb_cdc_write(const void *data, size_t length)
{
    if (!configured)
        return 0u;

    const uint8_t *bytes = data;
    const uint32_t irq_state = irq_save();

    usb_tx_kick();
    if (length > ((tx_tail - tx_head - 1u) & USB_BUFFER_MASK)) {
        irq_restore(irq_state);
        return 0u;
    }
    for (size_t i = 0; i < length; ++i) {
        tx_buffer[tx_head] = bytes[i];
        tx_head = (tx_head + 1u) & USB_BUFFER_MASK;
    }
    usb_tx_kick();
    irq_restore(irq_state);
    return length;
}

size_t usb_cdc_available(void)
{
    const uint32_t irq_state = irq_save();
    if (configured)
        usb_rx_kick();
    const size_t available = (rx_head - rx_tail) & USB_BUFFER_MASK;
    irq_restore(irq_state);
    return available;
}

size_t usb_cdc_read(void *data, size_t length)
{
    uint8_t *bytes = data;
    const uint32_t irq_state = irq_save();
    size_t read = 0u;

    if (configured)
        usb_rx_kick();
    while (read < length && rx_tail != rx_head) {
        bytes[read++] = rx_buffer[rx_tail];
        rx_tail = (rx_tail + 1u) & USB_BUFFER_MASK;
    }
    if (configured)
        usb_rx_kick();
    irq_restore(irq_state);
    return read;
}
