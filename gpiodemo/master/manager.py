import sys
import tkinter as tk
from tkinter import ttk

import customtkinter as ctk

__all__ = ["PinsFrame"]

MODES = ["INPUT", "IN_PULLUP", "OUTPUT"]
INPUT_MODES = {"INPUT", "IN_PULLUP"}

_DEFAULT_BUTTON_COLOR = ctk.ThemeManager.theme["CTkButton"]["fg_color"]
_BUTTON_TEXT_COLOR = ctk.ThemeManager.theme["CTkButton"]["text_color"]
_LABEL_TEXT_COLOR = ctk.ThemeManager.theme["CTkLabel"]["text_color"]
_PENDING_COLOR = "gray30"
_ACTIVE_COLOR = ("#2fa572", "#106a43")

_HALF_WIDTH = 450
_HEADER_HEIGHT = 36
_ROW_HEIGHT = 36
_COLUMN_X = (4, 96, 220, 300)


class PinsFrame(ctk.CTkFrame):
    """Scrollable GPIO pin table with asynchronous backend callbacks."""

    def __init__(
        self,
        master,
        pin_map,
        default_mode="INPUT",
        on_mode_change=None,
        on_read=None,
        on_listen=None,
        on_toggle=None,
        **kwargs,
    ):
        super().__init__(master, corner_radius=0, **kwargs)

        self.on_mode_change = on_mode_change
        self.on_read = on_read
        self.on_listen = on_listen
        self.on_toggle = on_toggle
        self.pin_map = pin_map
        self._rows = {}
        self._text_color = self._color(_LABEL_TEXT_COLOR)

        self.grid_rowconfigure(0, weight=1)
        self.grid_columnconfigure(0, weight=1)

        pin_names = list(pin_map)
        split = (len(pin_names) + 1) // 2
        left_pins = pin_names[:split]
        right_pins = pin_names[split:]

        self._build_canvas(max(len(left_pins), len(right_pins)))
        for row_index, pin in enumerate(left_pins):
            self._add_row(pin, default_mode, 0, row_index)
        for row_index, pin in enumerate(right_pins):
            self._add_row(pin, default_mode, 1, row_index)

    @staticmethod
    def _color(color):
        if isinstance(color, (tuple, list)):
            return color[ctk.get_appearance_mode() == "Dark"]
        return color

    def _build_canvas(self, row_count):
        background = self.cget("fg_color")
        if background == "transparent":
            background = tk.Frame.cget(self, "bg")
        else:
            background = self._apply_appearance_mode(background)

        self.canvas = tk.Canvas(
            self,
            bg=background,
            bd=0,
            highlightthickness=0,
            yscrollincrement=30,
        )
        scrollbar = ttk.Scrollbar(self, orient="vertical", command=self.canvas.yview)
        self.canvas.configure(yscrollcommand=scrollbar.set)
        self.canvas.grid(row=0, column=0, sticky="nsew", padx=(8, 0), pady=8)
        scrollbar.grid(row=0, column=1, sticky="ns", padx=(0, 8), pady=8)

        for half in range(2):
            half_x = half * _HALF_WIDTH
            for text, column_x in zip(("Pin", "Mode", "Status", "Action"), _COLUMN_X):
                self.canvas.create_text(
                    half_x + column_x,
                    _HEADER_HEIGHT / 2,
                    text=text,
                    anchor="w",
                    fill=self._text_color,
                    font=("TkDefaultFont", 10, "bold"),
                )

        self.canvas.configure(
            scrollregion=(
                0,
                0,
                _HALF_WIDTH * 2,
                _HEADER_HEIGHT + row_count * _ROW_HEIGHT,
            )
        )
        self._bind_scroll(self.canvas)
        self._bind_scroll(scrollbar)

    def _bind_scroll(self, widget):
        widget.bind("<MouseWheel>", self._on_mousewheel, add=True)
        widget.bind("<Button-4>", self._on_mousewheel, add=True)
        widget.bind("<Button-5>", self._on_mousewheel, add=True)

    def _on_mousewheel(self, event):
        if getattr(event, "num", None) == 4:
            amount = -1
        elif getattr(event, "num", None) == 5:
            amount = 1
        elif sys.platform == "darwin":
            amount = -event.delta
        else:
            amount = -int(event.delta / 120)

        if amount:
            self.canvas.yview_scroll(amount, "units")
        return "break"

    def _create_action_button(self, x, y, width, text, tag):
        rect = self.canvas.create_rectangle(
            x,
            y - 14,
            x + width,
            y + 14,
            fill=self._color(_DEFAULT_BUTTON_COLOR),
            outline="",
            tags=(tag,),
        )
        label = self.canvas.create_text(
            x + width / 2,
            y,
            text=text,
            fill=self._color(_BUTTON_TEXT_COLOR),
            tags=(tag,),
        )
        return rect, label

    def _add_row(self, pin, mode, half, row_index):
        half_x = half * _HALF_WIDTH
        y = _HEADER_HEIGHT + row_index * _ROW_HEIGHT + _ROW_HEIGHT / 2
        pin_id = self.pin_map[pin][0]

        self.canvas.create_text(
            half_x + _COLUMN_X[0],
            y,
            text=f"{pin} ({pin_id})",
            anchor="w",
            fill=self._text_color,
        )

        mode_var = ctk.StringVar(value=mode)
        mode_menu = ttk.Combobox(
            self.canvas,
            values=MODES,
            textvariable=mode_var,
            state="readonly",
            width=13,
        )
        mode_menu.bind(
            "<<ComboboxSelected>>",
            lambda _event, p=pin, var=mode_var: self._handle_mode_change(p, var.get()),
        )
        self._bind_scroll(mode_menu)
        self.canvas.create_window(
            half_x + _COLUMN_X[1],
            y,
            window=mode_menu,
            anchor="w",
        )

        status_item = self.canvas.create_text(
            half_x + _COLUMN_X[2],
            y,
            text="--",
            anchor="w",
            fill=self._text_color,
        )

        primary_tag = f"action-primary-{pin}"
        secondary_tag = f"action-secondary-{pin}"
        primary_rect, primary_text = self._create_action_button(
            half_x + _COLUMN_X[3], y, 52, "Read", primary_tag
        )
        secondary_rect, secondary_text = self._create_action_button(
            half_x + _COLUMN_X[3] + 58, y, 62, "Listen", secondary_tag
        )
        self.canvas.tag_bind(
            primary_tag,
            "<Button-1>",
            lambda _event, p=pin: self._handle_primary_action(p),
        )
        self.canvas.tag_bind(
            secondary_tag,
            "<Button-1>",
            lambda _event, p=pin: self._handle_secondary_action(p),
        )

        self._rows[pin] = {
            "mode_var": mode_var,
            "mode_menu": mode_menu,
            "status_item": status_item,
            "primary_rect": primary_rect,
            "primary_text": primary_text,
            "secondary_rect": secondary_rect,
            "secondary_text": secondary_text,
            "listening": False,
            "mode_pending": False,
            "read_pending": False,
            "listen_pending": False,
            "toggle_pending": False,
        }
        self._render_actions(pin, mode)

    def _set_action_button(self, row, which, text, color):
        self.canvas.itemconfigure(
            row[f"{which}_rect"],
            fill=self._color(color),
            state="normal",
        )
        self.canvas.itemconfigure(
            row[f"{which}_text"],
            text=text,
            state="normal",
        )

    def _render_actions(self, pin, mode):
        row = self._rows[pin]
        if mode in INPUT_MODES:
            self._set_action_button(
                row,
                "primary",
                "Reading..." if row["read_pending"] else "Read",
                _PENDING_COLOR if row["read_pending"] else _DEFAULT_BUTTON_COLOR,
            )
            if row["listen_pending"]:
                listen_text = "Sending..."
                listen_color = _PENDING_COLOR
            elif row["listening"]:
                listen_text = "Listening"
                listen_color = _ACTIVE_COLOR
            else:
                listen_text = "Listen"
                listen_color = _DEFAULT_BUTTON_COLOR
            self._set_action_button(row, "secondary", listen_text, listen_color)
        else:
            self._set_action_button(
                row,
                "primary",
                "Sending..." if row["toggle_pending"] else "Toggle",
                _PENDING_COLOR if row["toggle_pending"] else _DEFAULT_BUTTON_COLOR,
            )
            self.canvas.itemconfigure(row["secondary_rect"], state="hidden")
            self.canvas.itemconfigure(row["secondary_text"], state="hidden")

    def _refresh_read_button(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES:
            self._render_actions(pin, row["mode_var"].get())

    def _refresh_listen_button(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES:
            self._render_actions(pin, row["mode_var"].get())

    def _refresh_toggle_button(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() not in INPUT_MODES:
            self._render_actions(pin, row["mode_var"].get())

    def _refresh_mode_menu(self, pin):
        row = self._rows[pin]
        row["mode_menu"].configure(
            state="disabled" if row["mode_pending"] else "readonly"
        )

    def set_status(self, pin, status):
        row = self._rows.get(pin)
        if row is None:
            return

        if status == "HIGH":
            color = ("#2fa572", "#3ddc97")
        elif status == "LOW":
            color = ("gray10", "gray80")
        else:
            color = ("gray40", "gray60")
        self.canvas.itemconfigure(
            row["status_item"],
            text=status,
            fill=self._color(color),
        )

        row["read_pending"] = False
        row["toggle_pending"] = False
        self._refresh_read_button(pin)
        self._refresh_toggle_button(pin)

    def set_mode(self, pin, mode):
        row = self._rows.get(pin)
        if row is None:
            return

        row["mode_var"].set(mode)
        row["mode_pending"] = False
        self._render_actions(pin, mode)
        self._refresh_mode_menu(pin)

        if mode in INPUT_MODES:
            self._handle_read(pin)

    def set_listening(self, pin, listening):
        row = self._rows.get(pin)
        if row is None:
            return
        row["listening"] = listening
        row["listen_pending"] = False
        self._refresh_listen_button(pin)

    def fail_mode(self, pin):
        row = self._rows.get(pin)
        if row is None:
            return
        row["mode_pending"] = False
        self._refresh_mode_menu(pin)

    def fail_read(self, pin):
        row = self._rows.get(pin)
        if row is None:
            return
        row["read_pending"] = False
        self._refresh_read_button(pin)

    def fail_listen(self, pin):
        row = self._rows.get(pin)
        if row is None:
            return
        row["listen_pending"] = False
        self._refresh_listen_button(pin)

    def fail_toggle(self, pin):
        row = self._rows.get(pin)
        if row is None:
            return
        row["toggle_pending"] = False
        self._refresh_toggle_button(pin)

    def get_mode(self, pin):
        row = self._rows.get(pin)
        return row["mode_var"].get() if row else None

    def is_listening(self, pin):
        row = self._rows.get(pin)
        return bool(row and row["listening"])

    def _handle_primary_action(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES:
            if not row["read_pending"]:
                self._handle_read(pin)
        elif not row["toggle_pending"]:
            self._handle_toggle(pin)

    def _handle_secondary_action(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES and not row["listen_pending"]:
            self._handle_listen_toggle(pin)

    def _handle_mode_change(self, pin, mode):
        row = self._rows[pin]

        if mode not in INPUT_MODES and row["listening"]:
            row["listen_pending"] = True
            if self.on_listen:
                self.on_listen(pin, False)

        row["mode_pending"] = True
        self._refresh_mode_menu(pin)
        self.set_status(pin, "--")

        if self.on_mode_change:
            self.on_mode_change(pin, mode)

    def _handle_read(self, pin):
        row = self._rows[pin]
        row["read_pending"] = True
        self._refresh_read_button(pin)
        if self.on_read:
            self.on_read(pin)

    def _handle_listen_toggle(self, pin):
        row = self._rows[pin]
        desired_state = not row["listening"]
        row["listen_pending"] = True
        self._refresh_listen_button(pin)
        if self.on_listen:
            self.on_listen(pin, desired_state)

    def _handle_toggle(self, pin):
        row = self._rows[pin]
        row["toggle_pending"] = True
        self._refresh_toggle_button(pin)
        if self.on_toggle:
            self.on_toggle(pin)


if __name__ == "__main__":
    root = ctk.CTk()
    root.geometry("900x600")

    fake_latency_ms = 600
    pin_map = {f"PA{i}": [100 + i, i] for i in range(20)}

    def on_mode_change(pin, mode):
        print(f"{pin}: requesting mode -> {mode}")
        root.after(fake_latency_ms, lambda: pins_frame.set_mode(pin, mode))

    def on_read(pin):
        print(f"{pin}: requesting read")
        root.after(fake_latency_ms, lambda: pins_frame.set_status(pin, "HIGH"))

    def on_listen(pin, listening):
        print(f"{pin}: requesting listen -> {listening}")
        root.after(fake_latency_ms, lambda: pins_frame.set_listening(pin, listening))

    def on_toggle(pin):
        print(f"{pin}: requesting toggle")
        row = pins_frame._rows[pin]
        current = pins_frame.canvas.itemcget(row["status_item"], "text")
        new_status = "LOW" if current == "HIGH" else "HIGH"
        root.after(fake_latency_ms, lambda: pins_frame.set_status(pin, new_status))

    pins_frame = PinsFrame(
        root,
        pin_map=pin_map,
        on_mode_change=on_mode_change,
        on_read=on_read,
        on_listen=on_listen,
        on_toggle=on_toggle,
    )
    pins_frame.pack(fill="both", expand=True)

    pins_frame.set_mode("PA0", "IN_PULLUP")
    pins_frame.set_mode("PA1", "OUTPUT")
    pins_frame.set_status("PA1", "HIGH")

    root.mainloop()
