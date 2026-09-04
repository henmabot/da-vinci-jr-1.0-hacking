# app.py
#
# Main entrypoint for the GPIO demo
#
# This module uses relative imports, so it normally needs to be run as
# part of the `gpiodemo.master` package (e.g. `python -m gpiodemo.master.app`,
# or via `gpiodemo/main.py`). The block below detects when it's instead
# been run directly as a script (e.g. `python gpiodemo/master/app.py`,
# or `uv run gpiodemo/master/app.py`) and patches `sys.path`/`__package__`
# so the relative imports still resolve.

if __name__ == "__main__" and __package__ in (None, ""):
    import pathlib
    import sys

    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2]))
    __package__ = "gpiodemo.master"

import argparse
import threading
from queue import Queue

from .connection import get_packet, prompt_port
from .mock_serial import MockSerial
from .parser import PacketParser
from .window import create_window


def ui_main(window, queue):
    # do some things here and pass the queue to the buttons so they push
    # data to the queue
    window.mainloop()


def serial_main(conn, queue):
    while True:
        packet = get_packet(conn)
        queue.put(
            {
                "serial": True,
                "packet": packet,
            }
        )


def parser_main(window, app_frame, logs_frame, conn, queue):
    def on_log(line):
        # log_packet touches Tk widgets, so it must run on the main thread
        window.after(0, lambda: logs_frame.log_packet(line))

    parser = PacketParser(conn, queue, on_log=on_log)
    parser.start()


def main(use_mock: bool = False):
    conn = MockSerial() if use_mock else prompt_port()

    queue = Queue()

    window, app_frame, logs_frame = create_window(queue)

    parser_thread = threading.Thread(
        target=parser_main,
        args=(
            window,
            app_frame,
            logs_frame,
            conn,
            queue,
        ),
        daemon=True,
    )
    serial_thread = threading.Thread(
        target=serial_main,
        args=(
            conn,
            queue,
        ),
        daemon=True,
    )

    parser_thread.start()
    serial_thread.start()

    ui_main(window, queue)


if __name__ == "__main__":
    arg_parser = argparse.ArgumentParser()
    arg_parser.add_argument(
        "--mock",
        action="store_true",
        help="use a fake in-memory serial device instead of prompting for a real port",
    )
    args = arg_parser.parse_args()

    main(use_mock=args.mock)
