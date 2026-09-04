import tkinter as tk
from tkinter import messagebox, ttk
from typing import TypedDict

import customtkinter as ctk

__all__ = ["PinsFrame"]


class _Row(TypedDict):
    mode_var: ctk.StringVar
    mode_menu: ttk.Combobox | None
    status: str
    status_item: int | None
    primary_rect: int | None
    primary_text: int | None
    secondary_rect: int | None
    secondary_text: int | None
    listening: bool
    mode_pending: bool
    read_pending: bool
    listen_pending: bool
    toggle_pending: bool


MODES = ["INPUT", "IN_PULLUP", "OUTPUT"]
INPUT_MODES = {"INPUT", "IN_PULLUP"}
_UNAVAILABLE_PINS = {"PB8", "PB9", "PB10", "PB11"}

_DEFAULT_BUTTON_COLOR = ctk.ThemeManager.theme["CTkButton"]["fg_color"]
_BUTTON_TEXT_COLOR = ctk.ThemeManager.theme["CTkButton"]["text_color"]
_LABEL_TEXT_COLOR = ctk.ThemeManager.theme["CTkLabel"]["text_color"]
_PENDING_COLOR = "gray30"
_ACTIVE_COLOR = ("#2fa572", "#106a43")

_HALF_WIDTH = 450
_HEADER_HEIGHT = 36
_ROW_HEIGHT = 36
_ROWS_PER_HALF = 18
_COLUMN_X = (4, 96, 220, 300)


class PinsFrame(ctk.CTkFrame):
    """
    Paginated GPIO pin table.

    Instead of flat on_* callbacks, this frame talks directly to the
    parser's queue (see parser.py for the task/result contract): every
    user action is turned into a `{"window": True, "action": ..., "pin":
    ..., "value": ..., "callback": ...}` task, and the per-task callback
    is invoked (from the parser's background thread) with the parsed
    result dict. Since Tk isn't thread-safe, every callback marshals its
    UI update back onto the main thread via `self.after(0, ...)`.
    """

    def __init__(
        self,
        master,
        pin_map,
        queue,
        default_mode="UNSET",
        **kwargs,
    ):
        super().__init__(master, corner_radius=0, **kwargs)

        self.queue = queue
        self.pin_map = pin_map
        self._text_color = self._color(_LABEL_TEXT_COLOR)
        self._visible_pins = set()
        self._pin_names = list(pin_map)
        page_size = _ROWS_PER_HALF * 2
        self._pages = [
            self._pin_names[index : index + page_size]
            for index in range(0, len(self._pin_names), page_size)
        ] or [[]]
        self._page_index = 0
        self._rows: dict[str, _Row] = {
            pin: _Row(
                mode_var=ctk.StringVar(value=default_mode),
                mode_menu=None,
                status="--",
                status_item=None,
                primary_rect=None,
                primary_text=None,
                secondary_rect=None,
                secondary_text=None,
                listening=False,
                mode_pending=False,
                read_pending=False,
                listen_pending=False,
                toggle_pending=False,
            )
            for pin in self._pin_names
        }

        self.grid_rowconfigure(0, weight=1)
        self.grid_columnconfigure(0, weight=1)

        self._build_canvas()
        self._build_pager()
        self._show_page(0)

    @staticmethod
    def _color(color):
        if isinstance(color, (tuple, list)):
            return color[ctk.get_appearance_mode() == "Dark"]
        return color

    def _build_canvas(self):
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
            height=_HEADER_HEIGHT + _ROWS_PER_HALF * _ROW_HEIGHT,
        )
        self.canvas.grid(row=0, column=0, sticky="nsew", padx=8, pady=8)

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

    def _build_pager(self):
        footer = ttk.Frame(self)
        footer.grid(row=1, column=0, sticky="ew", padx=8, pady=(0, 8))
        footer.grid_columnconfigure(0, weight=1)

        # Reboot uses the same gray as every other button, just with red
        # text to flag it as dangerous -- everything else (including
        # this one's shape) matches the plain ttk.Button look of the
        # pager buttons below (auto-sized to their text, no explicit
        # width, so they hug their labels with the theme's own padding).
        style = ttk.Style()
        style.configure("Reboot.TButton", foreground="#c0392b")
        style.map("Reboot.TButton", foreground=[("active", "#8e2a1b")])

        # Device-wide, non-stateful actions (fire-and-forget, no toggled
        # state to track -- see parser.py for the task contract each of
        # these enqueues). Reboot is kept apart on the far left (with
        # extra spacing) so it can't be fat-fingered while reaching for
        # the other buttons.
        actions = ttk.Frame(footer)
        actions.grid(row=0, column=0, sticky="w")
        ttk.Button(
            actions,
            text="Reboot",
            style="Reboot.TButton",
            command=self.reboot,
        ).pack(side="left", padx=(0, 20))
        ttk.Button(actions, text="Input All", command=self.input_all).pack(
            side="left", padx=(0, 4)
        )
        ttk.Button(actions, text="Read All", command=self.read_all_inputs).pack(
            side="left", padx=(0, 4)
        )
        ttk.Button(actions, text="Listen All", command=self.listen_all_inputs).pack(
            side="left", padx=(0, 4)
        )
        ttk.Button(actions, text="Handshake", command=self.handshake).pack(
            side="left", padx=(0, 4)
        )
        ttk.Button(actions, text="Status", command=self.get_status).pack(side="left")

        pager = ttk.Frame(footer)
        pager.grid(row=0, column=1, sticky="e")
        self._prev_button = ttk.Button(
            pager,
            text="Previous",
            command=lambda: self._show_page(self._page_index - 1),
        )
        self._prev_button.pack(side="left")
        self._page_label = ttk.Label(pager)
        self._page_label.pack(side="left", padx=10)
        self._next_button = ttk.Button(
            pager,
            text="Next",
            command=lambda: self._show_page(self._page_index + 1),
        )
        self._next_button.pack(side="left")

    def _show_page(self, page_index):
        if not 0 <= page_index < len(self._pages):
            return

        for pin in self._visible_pins:
            row = self._rows[pin]
            mode_menu = row["mode_menu"]
            if mode_menu is not None:
                mode_menu.destroy()
            for key in (
                "mode_menu",
                "status_item",
                "primary_rect",
                "primary_text",
                "secondary_rect",
                "secondary_text",
            ):
                row[key] = None
        self.canvas.delete("row")

        self._page_index = page_index
        page = self._pages[page_index]
        left_pins = page[:_ROWS_PER_HALF]
        right_pins = page[_ROWS_PER_HALF:]
        self._visible_pins = set(page)

        for row_index, pin in enumerate(left_pins):
            self._add_row(pin, 0, row_index)
        for row_index, pin in enumerate(right_pins):
            self._add_row(pin, 1, row_index)

        self._page_label.configure(text=f"Page {page_index + 1}/{len(self._pages)}")
        self._prev_button.configure(state="normal" if page_index else "disabled")
        self._next_button.configure(
            state="normal" if page_index + 1 < len(self._pages) else "disabled"
        )

    def _create_action_button(self, x, y, width, text, tag):
        rect = self.canvas.create_rectangle(
            x,
            y - 14,
            x + width,
            y + 14,
            fill=self._color(_DEFAULT_BUTTON_COLOR),
            outline="",
            tags=("row", tag),
        )
        label = self.canvas.create_text(
            x + width / 2,
            y,
            text=text,
            fill=self._color(_BUTTON_TEXT_COLOR),
            tags=("row", tag),
        )
        return rect, label

    def _add_row(self, pin, half, row_index):
        half_x = half * _HALF_WIDTH
        y = _HEADER_HEIGHT + row_index * _ROW_HEIGHT + _ROW_HEIGHT / 2
        pin_id = self.pin_map[pin][0]
        row = self._rows[pin]

        self.canvas.create_text(
            half_x + _COLUMN_X[0],
            y,
            text=f"{pin} ({pin_id})",
            anchor="w",
            fill=self._text_color,
            tags=("row",),
        )

        mode_menu = ttk.Combobox(
            self.canvas,
            values=MODES,
            textvariable=row["mode_var"],
            state="readonly",
            # ttk.Combobox renders wider than its character width due to
            # theme padding + the dropdown arrow, so keep this tight --
            # 9 chars is exactly enough for the longest mode, "IN_PULLUP".
            width=9,
        )
        mode_menu.bind(
            "<<ComboboxSelected>>",
            lambda _event, p=pin, var=row["mode_var"]: self._handle_mode_change(
                p, var.get()
            ),
        )
        self.canvas.create_window(
            half_x + _COLUMN_X[1],
            y,
            window=mode_menu,
            anchor="w",
            tags=("row",),
        )

        status_item = self.canvas.create_text(
            half_x + _COLUMN_X[2],
            y,
            text=row["status"],
            anchor="w",
            fill=self._color(self._status_color(row["status"])),
            tags=("row",),
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

        row.update(
            mode_menu=mode_menu,
            status_item=status_item,
            primary_rect=primary_rect,
            primary_text=primary_text,
            secondary_rect=secondary_rect,
            secondary_text=secondary_text,
        )
        self._render_actions(pin, row["mode_var"].get())
        self._refresh_mode_menu(pin)

    @staticmethod
    def _status_color(status):
        if status == "HIGH":
            return ("#2fa572", "#3ddc97")
        if status == "LOW":
            return ("gray10", "gray80")
        return ("gray40", "gray60")

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
        if pin not in self._visible_pins:
            return
        row = self._rows[pin]
        if mode == "UNSET":
            for which in ("primary", "secondary"):
                if row[f"{which}_rect"] is not None:
                    self.canvas.itemconfigure(row[f"{which}_rect"], state="hidden")
                if row[f"{which}_text"] is not None:
                    self.canvas.itemconfigure(row[f"{which}_text"], state="hidden")
        elif mode in INPUT_MODES:
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
            if row["secondary_rect"] is not None:
                self.canvas.itemconfigure(row["secondary_rect"], state="hidden")
            if row["secondary_text"] is not None:
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
        if row["mode_menu"] is not None:
            row["mode_menu"].configure(
                state="disabled" if row["mode_pending"] else "readonly"
            )

    def set_status(self, pin, status):
        row = self._rows.get(pin)
        if row is None:
            return

        row["status"] = status
        if pin in self._visible_pins and row["status_item"] is not None:
            self.canvas.itemconfigure(
                row["status_item"],
                text=status,
                fill=self._color(self._status_color(status)),
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

    def input_all(self):
        """Set every configurable pin to plain input mode."""
        for pin in self._pin_names:
            row = self._rows[pin]
            if (
                pin not in _UNAVAILABLE_PINS
                and row["mode_var"].get() != "INPUT"
                and not row["mode_pending"]
            ):
                self._handle_mode_change(pin, "INPUT")

    def read_all_inputs(self):
        """Fire a one-shot read for every pin currently in an input mode."""
        for pin in self._pin_names:
            row = self._rows[pin]
            if row["mode_var"].get() in INPUT_MODES:
                self._handle_read(pin)

    def listen_all_inputs(self):
        """Turn listening on for every pin currently in an input mode."""
        for pin in self._pin_names:
            row = self._rows[pin]
            if (
                row["mode_var"].get() in INPUT_MODES
                and not row["listening"]
                and not row["listen_pending"]
            ):
                row["listen_pending"] = True
                self._refresh_listen_button(pin)
                self._send_listen(pin, True)

    def _handle_primary_action(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES:
            if not row["read_pending"]:
                self._handle_read(pin)
        elif row["mode_var"].get() == "OUTPUT" and not row["toggle_pending"]:
            self._handle_toggle(pin)

    def _handle_secondary_action(self, pin):
        row = self._rows[pin]
        if row["mode_var"].get() in INPUT_MODES and not row["listen_pending"]:
            self._handle_listen_toggle(pin)

    def _send(self, action, pin=None, value=None, callback=None):
        """Enqueue a window task for the parser (see parser.py header docs)."""
        task = {
            "window": True,
            "action": action,
            "callback": callback,
        }
        if pin is not None:
            task["pin"] = self.pin_map[pin][1]
        if value is not None:
            task["value"] = value
        self.queue.put(task)

    def handshake(self):
        self._send("handshake")

    def get_status(self):
        self._send("get_status")

    def _reset_pin_states(self):
        for row in self._rows.values():
            row["mode_var"].set("UNSET")
            row["status"] = "--"
            row["listening"] = False
            row["mode_pending"] = False
            row["read_pending"] = False
            row["listen_pending"] = False
            row["toggle_pending"] = False
        self._show_page(self._page_index)

    def reboot(self):
        if messagebox.askyesno(
            "Reboot device",
            "Send BYE and reset the device? This will drop the connection.",
        ):

            def on_result(result):
                if result["type"] == "goodbye_ack":
                    self.after(0, self._reset_pin_states)

            self._send("goodbye", callback=on_result)

    def _handle_mode_change(self, pin, mode):
        row = self._rows[pin]

        if mode not in INPUT_MODES and row["listening"]:
            row["listen_pending"] = True
            self._refresh_listen_button(pin)
            self._send_listen(pin, False)

        row["mode_pending"] = True
        self._refresh_mode_menu(pin)
        self.set_status(pin, "--")

        direction = "OUT" if mode == "OUTPUT" else "IN"
        pullup = "ON" if mode == "IN_PULLUP" else "OFF"

        def on_pullup_result(result):
            if result["type"] == "ack":
                self.after(0, lambda: self.set_mode(pin, mode))
            else:
                self.after(0, lambda: self.fail_mode(pin))

        def on_direction_result(result):
            if result["type"] == "ack":
                self._send("set_pullup", pin, pullup, callback=on_pullup_result)
            else:
                self.after(0, lambda: self.fail_mode(pin))

        self._send("set_direction", pin, direction, callback=on_direction_result)

    def _handle_read(self, pin):
        row = self._rows[pin]
        row["read_pending"] = True
        self._refresh_read_button(pin)

        def on_result(result):
            if result["type"] == "data":
                value = result.get("value")
                self.after(0, lambda: self.set_status(pin, value))
            else:
                self.after(0, lambda: self.fail_read(pin))

        self._send("get_value", pin, callback=on_result)

    def _send_listen(self, pin, desired_state):
        value = "ON" if desired_state else "OFF"

        def on_result(result):
            if result["type"] == "ack":
                self.after(0, lambda: self.set_listening(pin, desired_state))
            elif result["type"] == "data":
                # unsolicited push from an active listener
                pushed_value = result.get("value")
                self.after(0, lambda: self.set_status(pin, pushed_value))
            else:
                self.after(0, lambda: self.fail_listen(pin))

        self._send("set_listen", pin, value, callback=on_result)

    def _handle_listen_toggle(self, pin):
        row = self._rows[pin]
        desired_state = not row["listening"]
        row["listen_pending"] = True
        self._refresh_listen_button(pin)
        self._send_listen(pin, desired_state)

    def _handle_toggle(self, pin):
        row = self._rows[pin]
        row["toggle_pending"] = True
        self._refresh_toggle_button(pin)

        current = row["status"] if row["status"] in ("HIGH", "LOW") else "LOW"
        new_value = "LOW" if current == "HIGH" else "HIGH"

        def on_result(result):
            if result["type"] == "ack":
                self.after(0, lambda: self.set_status(pin, new_value))
            else:
                self.after(0, lambda: self.fail_toggle(pin))

        self._send("set_value", pin, new_value, callback=on_result)


if __name__ == "__main__":
    # Stands in for the real parser thread: consumes window tasks from the
    # queue and fakes a response after a short delay, exercising the same
    # task/result contract described in parser.py.
    import threading
    import time
    from queue import Queue

    fake_latency_s = 0.6
    pin_map = {f"PA{i}": [100 + i, i] for i in range(20)}
    task_queue = Queue()

    def fake_parser():
        while True:
            task = task_queue.get()
            if not task.get("window"):
                continue

            action = task["action"]
            callback = task.get("callback")
            if callback is None:
                continue

            time.sleep(fake_latency_s)
            if action in ("set_direction", "set_pullup", "set_value"):
                callback({"type": "ack"})
            elif action == "get_value":
                callback({"type": "data", "value": "HIGH"})
            elif action == "set_listen":
                desired_on = task.get("value") == "ON"
                callback({"type": "ack"})
                if desired_on:
                    time.sleep(fake_latency_s)
                    callback({"type": "data", "value": "LOW"})

    threading.Thread(target=fake_parser, daemon=True).start()

    root = ctk.CTk()
    root.geometry("900x600")

    pins_frame = PinsFrame(root, pin_map=pin_map, queue=task_queue)
    pins_frame.pack(fill="both", expand=True)

    pins_frame.set_mode("PA0", "IN_PULLUP")
    pins_frame.set_mode("PA1", "OUTPUT")
    pins_frame.set_status("PA1", "HIGH")

    root.mainloop()
