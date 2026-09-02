#ifndef SAM4E8E_USB_CDC_H
#define SAM4E8E_USB_CDC_H

#include <stdbool.h>
#include <stddef.h>

void usb_cdc_init(void);
bool usb_cdc_ready(void);
size_t usb_cdc_write(const void *data, size_t length);
size_t usb_cdc_available(void);
size_t usb_cdc_read(void *data, size_t length);

#endif
