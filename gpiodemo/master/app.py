# app.py
#
# Main entrypoint for the GPIO demo

import threading
from queue import Queue

from .connection import get_packet, prompt_port
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
    parser = PacketParser(conn, queue)


def main():
    conn = prompt_port()

    queue = Queue()

    window, app_frame, logs_frame = create_window()

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
    main()
