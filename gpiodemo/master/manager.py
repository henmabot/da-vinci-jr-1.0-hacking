# manager.py
#
# Pins manager frame: a table of GPIO pins with mode dropdown, live status,
# and mode-dependent action buttons.
#
# Everything here is fire-and-forget: clicking a control disables it and
# shows a pending state ("Sending...", etc), fires the corresponding
# on_* callback, and then does nothing further. It is the backend's job
# to call the matching public set_*()/fail_*() method once it knows the
# real outcome (e.g. after a serial round-trip). The UI never assumes
# success on its own -- it only reflects what the backend confirms.
#
# Input-mode pins get Read (one-shot) and Listen (toggle -- arms/disarms
# an interrupt listener); output-mode pins get Toggle. Switching a pin
# into INPUT/IN_PULLUP automatically fires a read request so the status
# box isn't left empty.
#
# Pins are supplied as a pin_map, e.g.:
#     pin_map = {
#         "PA0": [102, 0x00],
#         "PA1": [99, 0x01],
#     }
# where the first element is the numeric pin id shown alongside the name
# (e.g. "PA0 (102)") and the second element (a hex code) is currently
# unused by this UI.
#
# With large pin counts, everything is laid out as two side-by-side
# column-groups (first half / second half) inside a single shared
# scrollable frame, so there's one scrollbar instead of two.

import tkinter as tk
from tkinter import ttk

import customtkinter as ctk

__all__ = ["PinsFrame"]

MODES = ["INPUT", "IN_PULLUP", "OUTPUT"]
INPUT_MODES = {"INPUT", "IN_PULLUP"}

_DEFAULT_BUTTON_COLOR = ctk.ThemeManager.theme["CTkButton"]["fg_color"]
_PENDING_COLOR = "gray30"
_ACTIVE_COLOR = ("#2fa572", "#106a43")
_LABEL_TEXT_COLOR = ctk.ThemeManager.theme["CTkLabel"]["text_color"]


class PinsFrame(ctk.CTkFrame):
    """
    A frame showing a table of GPIO pins: Pin | Mode | Status | Action,
    split into two side-by-side halves that share one scrollbar.

    Usage:
        pin_map = {
            "PA0": [102, 0x00],
            "PA1": [99, 0x01],
        }

        pins_frame = PinsFrame(
            left_container,
            pin_map=pin_map,
            on_mode_change=lambda pin, mode: ...,   # request only
            on_read=lambda pin: ...,                # request only
            on_listen=lambda pin, listening: ...,   # request only
            on_toggle=lambda pin: ...,               # request only
        )
        pins_frame.pack(fill="both", expand=True)

        # Backend calls these once it knows the real outcome, e.g. from
        # parser_main via window.after(0, lambda: ...):
        pins_frame.set_mode("PA0", "IN_PULLUP")
        pins_frame.set_status("PA0", "HIGH")
        pins_frame.set_listening("PA0", True)

        # And on failure/timeout, to un-stick a control:
        pins_frame.fail_mode("PA0")
        pins_frame.fail_read("PA0")
        pins_frame.fail_listen("PA0")
        pins_frame.fail_toggle("PA0")
    """

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
        self._pin_label_color = _LABEL_TEXT_COLOR[ctk.get_appearance_mode() == "Dark"]
        self._rows = {}  # pin_name -> row state dict

        self.grid_rowconfigure(0, weight=1)
        self.grid_columnconfigure(0, weight=1)

        self._build_scroll_area()

        pin_names = list(pin_map.keys())
        split = (len(pin_names) + 1) // 2  # first half gets the extra one if odd
        left_pins, right_pins = pin_names[:split], pin_names[split:]

        for pin in left_pins:
            self._add_row(self.left_table, pin, default_mode)
        for pin in right_pins:
            self._add_row(self.right_table, pin, default_mode)

    # ------------------------------------------------------------------
    # UI construction
    # ------------------------------------------------------------------

    def _build_scroll_area(self):
        # One shared scrollable frame containing two side-by-side
        # column-group tables, so both halves scroll together.
        self.scroll_frame = ctk.CTkScrollableFrame(self, fg_color="transparent")
        self.scroll_frame.grid(row=0, column=0, sticky="nsew", padx=8, pady=8)
        self.scroll_frame.grid_columnconfigure(0, weight=1)
        self.scroll_frame.grid_columnconfigure(1, weight=1)

        self.left_table = self._build_column_table(self.scroll_frame)
        self.left_table.grid(row=0, column=0, sticky="new", padx=(0, 8))

        self.right_table = self._build_column_table(self.scroll_frame)
        self.right_table.grid(row=0, column=1, sticky="new", padx=(8, 0))

    def _build_column_table(self, parent):
        table = ctk.CTkFrame(parent, fg_color="transparent")
        self._configure_columns(table)

        ctk.CTkLabel(table, text="Pin", font=ctk.CTkFont(weight="bold")).grid(
            row=0, column=0, sticky="w", padx=4, pady=4
        )
        ctk.CTkLabel(table, text="Mode", font=ctk.CTkFont(weight="bold")).grid(
            row=0, column=1, sticky="w", padx=4, pady=4
        )
        ctk.CTkLabel(table, text="Status", font=ctk.CTkFont(weight="bold")).grid(
            row=0, column=2, sticky="w", padx=4, pady=4
        )
        ctk.CTkLabel(table, text="Action", font=ctk.CTkFont(weight="bold")).grid(
            row=0, column=3, sticky="w", padx=4, pady=4
        )

        # row 0 is the header; pin rows start at row 1 and are tracked
        # per-table via a simple counter attribute.
        table._next_row = 1
        return table

    @staticmethod
    def _configure_columns(container):
        container.grid_columnconfigure(0, weight=1, minsize=90)
        container.grid_columnconfigure(1, weight=1, minsize=120)
        container.grid_columnconfigure(2, weight=1, minsize=80)
        container.grid_columnconfigure(3, weight=2, minsize=160)

    def _add_row(self, table, pin, mode):
        row_index = table._next_row
        table._next_row += 1

        pin_id = self.pin_map[pin][0]
        pin_label = tk.Label(
            table,
            text=f"{pin} ({pin_id})",
            anchor="w",
            bg=tk.Frame.cget(table, "bg"),
            fg=self._pin_label_color,
            bd=0,
        )
        pin_label.grid(row=row_index, column=0, sticky="w", padx=4, pady=4)

        mode_var = ctk.StringVar(value=mode)
        mode_menu = ttk.Combobox(
            table,
            values=MODES,
            textvariable=mode_var,
            state="readonly",
            width=13,
        )
        mode_menu.bind(
            "<<ComboboxSelected>>",
            lambda _event, p=pin, var=mode_var: self._handle_mode_change(p, var.get()),
        )
        mode_menu.grid(row=row_index, column=1, sticky="w", padx=4, pady=4)

        status_label = ctk.CTkLabel(table, text="--", anchor="w")
        status_label.grid(row=row_index, column=2, sticky="w", padx=4, pady=4)

        action_container = ctk.CTkFrame(table, fg_color="transparent", corner_radius=0)
        action_container.grid(row=row_index, column=3, sticky="w", padx=4, pady=4)

        self._rows[pin] = {
            "mode_var": mode_var,
            "mode_menu": mode_menu,
            "status_label": status_label,
            "action_container": action_container,
            "listening": False,
            "listen_btn": None,
            "read_btn": None,
            "toggle_btn": None,
            "mode_pending": False,
            "read_pending": False,
            "listen_pending": False,
            "toggle_pending": False,
        }

        self._render_actions(pin, mode)

    # ------------------------------------------------------------------
    # Actions per mode
    # ------------------------------------------------------------------

    def _render_actions(self, pin, mode):
        row = self._rows[pin]
        container = row["action_container"]
        for child in container.winfo_children():
            child.destroy()
        row["listen_btn"] = None
        row["read_btn"] = None
        row["toggle_btn"] = None

        if mode in INPUT_MODES:
            read_btn = ctk.CTkButton(
                container,
                text="Read",
                width=40,
                corner_radius=0,
                command=lambda p=pin: self._handle_read(p),
            )
            read_btn.pack(side="left", padx=(0, 6))
            row["read_btn"] = read_btn

            listen_btn = ctk.CTkButton(
                container,
                text="Listen",
                width=50,
                corner_radius=0,
                command=lambda p=pin: self._handle_listen_toggle(p),
            )
            listen_btn.pack(side="left")
            row["listen_btn"] = listen_btn
            if row["listen_pending"] or row["listening"]:
                self._refresh_listen_button(pin)
        else:  # OUTPUT
            toggle_btn = ctk.CTkButton(
                container,
                text="Toggle",
                width=50,
                corner_radius=0,
                command=lambda p=pin: self._handle_toggle(p),
            )
            toggle_btn.pack(side="left")
            row["toggle_btn"] = toggle_btn

    # ------------------------------------------------------------------
    # Button visual refreshers -- each reflects (pending, active) state
    # ------------------------------------------------------------------

    def _refresh_read_button(self, pin):
        row = self._rows[pin]
        btn = row["read_btn"]
        if btn is None:
            return
        if row["read_pending"]:
            btn.configure(text="Reading...", state="disabled", fg_color=_PENDING_COLOR)
        else:
            btn.configure(text="Read", state="normal", fg_color=_DEFAULT_BUTTON_COLOR)

    def _refresh_listen_button(self, pin):
        row = self._rows[pin]
        btn = row["listen_btn"]
        if btn is None:
            return
        if row["listen_pending"]:
            btn.configure(text="Sending...", state="disabled", fg_color=_PENDING_COLOR)
        elif row["listening"]:
            btn.configure(text="Listening", state="normal", fg_color=_ACTIVE_COLOR)
        else:
            btn.configure(text="Listen", state="normal", fg_color=_DEFAULT_BUTTON_COLOR)

    def _refresh_toggle_button(self, pin):
        row = self._rows[pin]
        btn = row["toggle_btn"]
        if btn is None:
            return
        if row["toggle_pending"]:
            btn.configure(text="Sending...", state="disabled", fg_color=_PENDING_COLOR)
        else:
            btn.configure(text="Toggle", state="normal", fg_color=_DEFAULT_BUTTON_COLOR)

    def _refresh_mode_menu(self, pin):
        row = self._rows[pin]
        row["mode_menu"].configure(
            state="disabled" if row["mode_pending"] else "readonly"
        )

    # ------------------------------------------------------------------
    # Public API -- backend calls these to confirm outcomes
    # ------------------------------------------------------------------

    def set_status(self, pin, status):
        """
        Confirm a read or toggle result and update the displayed status,
        e.g. set_status("PA0", "HIGH"). Also clears read/toggle pending
        state and re-enables those buttons. Safe to call from the
        main/UI thread only -- marshal via window.after(0, ...) if
        calling from a background thread.
        """
        row = self._rows.get(pin)
        if row is None:
            return

        label = row["status_label"]
        label.configure(text=status)
        if status == "HIGH":
            label.configure(text_color=("#2fa572", "#3ddc97"))
        elif status == "LOW":
            label.configure(text_color=("gray10", "gray80"))
        else:
            label.configure(text_color=("gray40", "gray60"))

        row["read_pending"] = False
        row["toggle_pending"] = False
        self._refresh_read_button(pin)
        self._refresh_toggle_button(pin)

    def set_mode(self, pin, mode):
        """
        Confirm a mode change (or set the initial/external mode). Clears
        mode-pending, re-enables the dropdown, and rebuilds the action
        buttons for the new mode. If the new mode is INPUT/IN_PULLUP,
        automatically fires a read request so status isn't left stale.
        """
        row = self._rows.get(pin)
        if row is None:
            return

        row["mode_var"].set(mode)
        row["mode_pending"] = False
        self._render_actions(pin, mode)  # rebuilds buttons, incl. mode_menu state
        self._refresh_mode_menu(pin)

        if mode in INPUT_MODES:
            self._handle_read(pin)

    def set_listening(self, pin, listening):
        """Confirm the interrupt listener's armed/disarmed state."""
        row = self._rows.get(pin)
        if row is None:
            return
        row["listening"] = listening
        row["listen_pending"] = False
        self._refresh_listen_button(pin)

    def fail_mode(self, pin):
        """Backend could not apply the requested mode change -- un-stick the dropdown."""
        row = self._rows.get(pin)
        if row is None:
            return
        row["mode_pending"] = False
        self._refresh_mode_menu(pin)

    def fail_read(self, pin):
        """Backend could not complete the read -- un-stick the Read button."""
        row = self._rows.get(pin)
        if row is None:
            return
        row["read_pending"] = False
        self._refresh_read_button(pin)

    def fail_listen(self, pin):
        """Backend could not arm/disarm the listener -- un-stick the Listen button."""
        row = self._rows.get(pin)
        if row is None:
            return
        row["listen_pending"] = False
        self._refresh_listen_button(pin)

    def fail_toggle(self, pin):
        """Backend could not complete the toggle -- un-stick the Toggle button."""
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

    # ------------------------------------------------------------------
    # Internal handlers -- fire request, go pending, wait for backend
    # ------------------------------------------------------------------

    def _handle_mode_change(self, pin, mode):
        row = self._rows[pin]

        if mode not in INPUT_MODES and row["listening"]:
            # Leaving input mode invalidates any armed interrupt listener.
            # This is also fire-and-forget -- backend should confirm via
            # set_listening(pin, False) same as a manual disarm would.
            row["listen_pending"] = True
            if self.on_listen:
                self.on_listen(pin, False)

        row["mode_pending"] = True
        self._refresh_mode_menu(pin)
        self.set_status(pin, "--")  # stale until backend confirms new mode + read

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
    # Quick manual test harness. Uses window.after(...) with a fake delay
    # to simulate a backend round-trip, matching how a real serial
    # backend would call these methods asynchronously.

    root = ctk.CTk()
    root.geometry("900x600")

    FAKE_LATENCY_MS = 600

    # Simulate a large pin_map like the real 120-pin case.
    pin_map = {f"PA{i}": [100 + i, i] for i in range(20)}

    def on_mode_change(pin, mode):
        print(f"{pin}: requesting mode -> {mode}")
        root.after(FAKE_LATENCY_MS, lambda: pins_frame.set_mode(pin, mode))

    def on_read(pin):
        print(f"{pin}: requesting read")
        root.after(FAKE_LATENCY_MS, lambda: pins_frame.set_status(pin, "HIGH"))

    def on_listen(pin, listening):
        print(f"{pin}: requesting listen -> {listening}")
        root.after(FAKE_LATENCY_MS, lambda: pins_frame.set_listening(pin, listening))

    def on_toggle(pin):
        print(f"{pin}: requesting toggle")
        current = pins_frame._rows[pin]["status_label"].cget("text")
        new_status = "LOW" if current == "HIGH" else "HIGH"
        root.after(FAKE_LATENCY_MS, lambda: pins_frame.set_status(pin, new_status))

    pins_frame = PinsFrame(
        root,
        pin_map=pin_map,
        on_mode_change=on_mode_change,
        on_read=on_read,
        on_listen=on_listen,
        on_toggle=on_toggle,
    )
    pins_frame.pack(fill="both", expand=True)

    # Simulate initial device-reported state (e.g. right after connecting).
    pins_frame.set_mode("PA0", "IN_PULLUP")
    pins_frame.set_mode("PA1", "OUTPUT")
    pins_frame.set_status("PA1", "HIGH")

    root.mainloop()
