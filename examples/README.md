# Examples

This folder contains example firmware builds that you can download and flash.

The available examples are:

- [Blink](blink.bin) ([source](https://github.com/itsyumiki/da-vinci-jr-1.0-hacking/tree/bca9b8ce5da0e177e398c52942f69a1dbfcbf0b6))

## Blink

The Blink example is a simple firmware that blinks the top light of the board. It also has USB CDC test, and repeatedly prints the status of the X axis endstop to the serial console.

If its triggered (hotend on left side), it will report high, and if not, it will report low.

Example output:

```
hello world 1, pd8 is high
hello world 2, pd8 is high
hello world 3, pd8 is high
hello world 4, pd8 is high
hello world 5, pd8 is high
hello world 6, pd8 is high
hello world 7, pd8 is high
hello world 8, pd8 is high
hello world 9, pd8 is low
hello world 10, pd8 is low
hello world 11, pd8 is low
```

> Its source code is available [here](https://github.com/itsyumiki/da-vinci-jr-1.0-hacking/tree/bca9b8ce5da0e177e398c52942f69a1dbfcbf0b6).
