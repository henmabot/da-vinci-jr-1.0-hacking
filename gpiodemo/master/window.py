# window.py
#
# Main Window setup for the GPIO Controller

import tkinter as tk

import customtkinter as ctk

from .logs import LogsFrame
from .manager import PinsFrame
from .pins import pin_map

__all__ = ["HEIGHT", "WIDTH", "create_window"]


WIDTH = 1250
HEIGHT = 800


def create_window(queue):
    # Create the window
    window = ctk.CTk()
    window.title("GPIO Controller")

    # Get screen dimensions
    screen_width = window.winfo_screenwidth()
    screen_height = window.winfo_screenheight()

    # Calculate the offset to center the window
    x = (screen_width // 2) - (WIDTH // 2)
    y = (screen_height // 2) - (HEIGHT // 2)

    # Set the window geometry to center it on the screen
    window.geometry(f"{WIDTH}x{HEIGHT}+{x}+{y}")

    # Create the PanedWindow
    main_window = tk.PanedWindow(
        window,
        orient="horizontal",
        sashwidth=5,
        bg="gray20",  # divider color
        bd=0,
        relief="flat",
    )

    # Use all available space
    main_window.pack(fill="both", expand=True)

    # Create the containers
    left_container = tk.Frame(main_window)
    right_container = tk.Frame(main_window)

    # Place the containers in the PanedWindow
    main_window.add(left_container, minsize=600)
    main_window.add(right_container, minsize=300)

    # Create the actual frames and make them fill the containers
    app_frame = PinsFrame(
        left_container,
        pin_map=pin_map,
        queue=queue,
    )
    logs_frame = LogsFrame(
        right_container,
        send_callback=lambda line: queue.put({"raw": True, "packet": line}),
    )

    app_frame.pack(fill="both", expand=True)
    logs_frame.pack(fill="both", expand=True)

    # Let the sash render so we can set its position
    window.update_idletasks()

    # place the sash
    sash_pos = int(WIDTH * (11 / 15))
    main_window.sash_place(0, sash_pos, 0)

    # Return the window and frames
    return window, app_frame, logs_frame


if __name__ == "__main__":
    from queue import Queue

    window, app_frame, logs_frame = create_window(Queue())
    window.mainloop()
