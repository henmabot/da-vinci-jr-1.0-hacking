# Connectors

There are 3 main data connectors on the Da Vinci Jr 1.0 board.

- 51-pin hotend connector
- 22-pin LCD connector
- 16-pin SD card connector

There are of course a lot more connectors on board, but these are the main ones needing identification.

## 22-pin LCD connector

The 22-pin LCD connector connects the main board with the sub-board, and carries the LCD display data.

### Photos

To be added

### Pinout

| Pin | MCU     | Pin Desc   | Verified? | Connects To | Description | Verified? |
| --- | ------- | ---------- | --------- | ----------- | ----------- | --------- |
| 01  | +5V     | Power      | ❌        |             |             | ❌        |
| 02  | GND     | Ground     | ❌        |             |             | ❌        |
| 03  | 111     | PC18       | ❌        |             |             | ❌        |
| 04  | 82      | PC8        | ❌        |             |             | ❌        |
| 05  | U1_Pin1 | ???        | ❌        |             |             | ❌        |
| 06  | 11      | PC0        | ❌        |             |             | ❌        |
| 07  | 38      | PC1        | ❌        |             |             | ❌        |
| 08  | 39      | PC2        | ❌        |             |             | ❌        |
| 09  | 40      | PC3        | ❌        |             |             | ❌        |
| 10  | 41      | PC4        | ❌        |             |             | ❌        |
| 11  | 58      | PC5        | ❌        |             |             | ❌        |
| 12  | 54      | PC6        | ❌        |             |             | ❌        |
| 13  | 48      | PC7        | ❌        |             |             | ❌        |
| 14  | 90      | PC10       | ❌        |             |             | ❌        |
| 15  | 34      | VDDCORE    | ❌        |             |             | ❌        |
| 16  | 32      | PA21/PGMD9 | ❌        |             |             | ❌        |
| 17  | 31      | PB3        | ❌        |             |             | ❌        |
| 18  | 28      | PE5        | ❌        |             |             | ❌        |
| 19  | 27      | PE4        | ❌        |             |             | ❌        |
| 20  | 25      | PA17/PGMD5 | ❌        |             |             | ❌        |
| 21  | GND     | Ground     | ❌        |             |             | ❌        |
| 22  | +5V     | Power      | ❌        |             |             | ❌        |

## 16-pin SD card connector

The 16-pin LCD connector connects the main board with the sub-board, and carries the SD card data.

### Photos

To be added

### Pinout

| Pin | MCU   | Pin Desc          | Verified? | Connects To | Description | Verified? |
| --- | ----- | ----------------- | --------- | ----------- | ----------- | --------- |
| 01  | -     | NC                | ❌        |             |             | ❌        |
| 02  | GND   | Ground            | ❌        |             |             | ❌        |
| 03  | 118   | PA31/MCDA1        | ❌        |             |             | ❌        |
| 04  | GND   | Ground            | ❌        |             |             | ❌        |
| 05  | 116   | PA30/MCDA0        | ❌        |             |             | ❌        |
| 06  | GND   | Ground            | ❌        |             |             | ❌        |
| 07  | 129   | PA29/MCCK         | ❌        |             |             | ❌        |
| 08  | +3.3V | Power             | ❌        |             |             | ❌        |
| 09  | +3.3V | Power             | ❌        |             |             | ❌        |
| 10  | 59    | PA25/PGMD13/CTS1  | ❌        |             |             | ❌        |
| 11  | GND   | Ground            | ❌        |             |             | ❌        |
| 12  | 112   | PA28/MCCDA        | ❌        |             |             | ❌        |
| 13  | GND   | Ground            | ❌        |             |             | ❌        |
| 14  | 70    | PA27/PGMD15/MCDA3 | ❌        |             |             | ❌        |
| 15  | GND   | Ground            | ❌        |             |             | ❌        |
| 16  | 62    | PA28/PGMD14/MCDA2 | ❌        |             |             | ❌        |
