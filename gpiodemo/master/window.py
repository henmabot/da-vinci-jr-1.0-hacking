# window.py
#
# Main Window setup for the GPIO Controller

import tkinter as tk

import customtkinter as ctk

__all__ = ["HEIGHT", "WIDTH", "create_window"]


WIDTH = 1200
HEIGHT = 800


def create_window():
    # Set color theme
    ctk.set_default_color_theme("green")

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
    app_frame = ctk.CTkFrame(left_container, corner_radius=0)
    logs_frame = ctk.CTkFrame(right_container, corner_radius=0)

    app_frame.pack(fill="both", expand=True)
    logs_frame.pack(fill="both", expand=True)

    # Let the sash render so we can set its position
    window.update_idletasks()

    # place the sash
    sash_pos = int(WIDTH * (3 / 4))
    main_window.sash_place(0, sash_pos, 0)

    # Return the window and frames
    return window, app_frame, logs_frame


if __name__ == "__main__":
    window, app_frame, logs_frame = create_window()
    window.mainloop()
