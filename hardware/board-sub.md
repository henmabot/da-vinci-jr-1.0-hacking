# Sub-board

The sub-board is the board for the interactivity of the printer. It has **6 push buttons, a 16x4 LCD display, and a full-size SD card slot**. It also has a **16 pin connector** labeled "SD CARD SLOT CONNECTOR", and a **22 pin connector** labeled "LCM SLOT CONNECTOR". It has no visible MCU's, only some power related chips and components. It also has no visible unpopulated headers, connectors and chip sockets.

| Front view of the board                         | Back view of the board                        |
| ----------------------------------------------- | --------------------------------------------- |
| ![Front view](../images/hd/sub-board-front.jpg) | ![Back view](../images/hd/sub-board-back.jpg) |

The board is **62mm to 185mm** in size.

**The components that i was able to identify so far are:**

- **1x** Winstar WH1604A 16x04 LCD module
- **1x** Generic SD card reader module
- **6x** Generic 6mm push buttons

There are some more chips that i didnt see as important, as they are passive or hardware driven.

More information about the connectors on-board can be found in the [connectors](connectors.md) section.

## Winstar WH1604A 16x04 LCD module

The Winstar WH1604A 16x04 LCD module is a character LCD display that is used to display text on the sub-board. It has 16 columns and 4 rows of characters, and is connected to the main board via a 22 pin connector labeled "LCM SLOT CONNECTOR".

Refer to the [Winstar WH1604A 16x04 LCD Module Datasheet](../SOURCES.md#datasheet-winstar-wh1604a) for more information.

### Photos

| Close up photos | will be added |
| --------------- | ------------- |
|                 |               |

## SD Card Reader Module

The SD card reader module is used to read full size SD cards and is connected to the main board via a 16 pin connector labeled "SD CARD SLOT CONNECTOR".

It is a generic SD card reader module, so there is no specific datasheet available.

### Pinout

Pins start from P1 (on the right of the reader) and go to the left, up to P11.

| Pin | Function | Description        | Connector Pin | Verified? |
| --- | -------- | ------------------ | ------------- | --------- |
| 01  | CD/DAT3  | Chip Select        |               | ❌        |
| 02  | CMD      | MOSI               |               | ❌        |
| 03  | VSS1     | Ground             |               | ❌        |
| 04  | VDD      | Power              |               | ❌        |
| 05  | CLK      | SCK                |               | ❌        |
| 06  | VSS2     | Ground             |               | ❌        |
| 07  | DAT0     | MISO               |               | ❌        |
| 08  | DAT1     | -                  |               | ❌        |
| 09  | DAT2     | -                  |               | ❌        |
| 10  | WP       | Write Protect lock |               | ❌        |
| 11  | CD       | Card Detect        | 10            | ✅        |

### Photos

| Close up photos | will be added |
| --------------- | ------------- |
|                 |               |

## Push Buttons

The push buttons are used to control the sub-board and are connected to the main board via one of the existing connectors.

I doubt a datasheet for a button would be useful even.

### Photos

| Close up photos | will be added |
| --------------- | ------------- |
|                 |               |
