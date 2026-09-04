# GPIO controller

`master/` contains the Python GPIO controller UI and serial client. `slave/` contains the SAM4E8E USB CDC firmware that serves the same request-ID protocol on the printer board.

See [`slave/README.md`](slave/README.md) for the wire protocol, pin numbering, and firmware build instructions.
