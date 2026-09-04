# Optical Endstops

There are 3 optical endstops on the printer. They share one connector labeled "Home Sensor".

## Pinout

| Pin Name  | MCU | Pin Desc | Verified? |
| --------- | --- | -------- | --------- |
| X Endstop | 113 | PD8      | ✅        |
| Y Endstop | 117 | PC19     | ✅        |
| Z Endstop | 110 | PD9      | ✅        |

They are high when triggered, low when idle. They have hardware pull-up resistors on them. I am unsure about software pull-up being redundant.

## Photos

| In this photo all the endstops are visible after removal.          |
| ------------------------------------------------------------------ |
| ![All endstops after removal (YT)](images/components/endstops.png) |
