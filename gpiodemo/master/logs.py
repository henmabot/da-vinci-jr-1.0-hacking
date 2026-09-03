# logs.py
#
# Prints logs to console and the logs frame


from datetime import datetime

import customtkinter as ctk

__all__ = ["LogsFrame"]


class LogsFrame(ctk.CTkFrame):
    """
    A frame containing:
      - a scrollable, read-only, text-selectable log of packets
      - a toggle button to enable/disable auto-scroll
      - a text entry (with history) + send button for manual commands

    Usage:
        logs_frame = LogsFrame(right_container, send_callback=on_send)
        logs_frame.pack(fill="both", expand=True)

        # elsewhere, e.g. from parser_main via window.after(...):
        logs_frame.log_packet("001 GET 005")
    """

    MAX_HISTORY = 200

    def __init__(self, master, send_callback=None, **kwargs):
        super().__init__(master, corner_radius=0, **kwargs)

        self.send_callback = send_callback
        self.autoscroll = True
        self.show_timestamps = True

        self._entries = []  # list of (timestamp_str, text) for re-rendering
        self._command_history = []
        self._history_index = None  # None means "not currently browsing history"

        mono_family = self._find_monospace_family()
        self._log_font = ctk.CTkFont(family=mono_family, size=13)
        self._entry_font = ctk.CTkFont(family=mono_family, size=13)

        self._build_log_view()
        self._build_command_bar()

    @staticmethod
    def _find_monospace_family():
        # Pick the first available monospace font from a preference list,
        # since font availability differs across Windows/macOS/Linux.
        import tkinter.font as tkfont

        candidates = [
            "Consolas",  # Windows
            "Menlo",  # macOS
            "SF Mono",  # macOS
            "DejaVu Sans Mono",  # common on Linux
            "Liberation Mono",  # common on Linux
            "Courier New",
            "Courier",
        ]
        available = set(tkfont.families())
        for name in candidates:
            if name in available:
                return name
        return "TkFixedFont"  # Tk's built-in fixed-width font, always present

    # ------------------------------------------------------------------
    # UI construction
    # ------------------------------------------------------------------

    def _build_log_view(self):
        # Top bar: title + autoscroll toggle
        top_bar = ctk.CTkFrame(self, fg_color="transparent")
        top_bar.pack(fill="x", padx=8, pady=(8, 4))

        self.autoscroll_button = ctk.CTkButton(
            top_bar,
            text="Auto-scroll",
            width=80,
            command=self._toggle_autoscroll,
        )
        self.autoscroll_button.pack(side="right")

        self.timestamps_button = ctk.CTkButton(
            top_bar,
            text="Timestamps",
            width=80,
            command=self._toggle_timestamps,
        )
        self.timestamps_button.pack(side="right", padx=(0, 8))

        clear_button = ctk.CTkButton(
            top_bar,
            text="Clear",
            width=60,
            fg_color="gray30",
            hover_color="gray20",
            command=self.clear,
        )
        clear_button.pack(side="left", padx=(0, 8))

        # The log itself. CTkTextbox is scrollable and text is
        # selectable/copyable by default; we just disable editing.
        self.textbox = ctk.CTkTextbox(
            self, wrap="none", activate_scrollbars=True, font=self._log_font
        )
        self.textbox.pack(fill="both", expand=True, padx=8, pady=(0, 4))
        self.textbox.configure(state="disabled")

        # Detect manual scrolling so we can auto-disable autoscroll
        # if the user scrolls up to read something.
        self.textbox.bind("<MouseWheel>", self._on_manual_scroll)  # Windows/macOS
        self.textbox.bind("<Button-4>", self._on_manual_scroll)  # Linux scroll up
        self.textbox.bind("<Button-5>", self._on_manual_scroll)  # Linux scroll down

    def _build_command_bar(self):
        bottom_bar = ctk.CTkFrame(self, fg_color="transparent")
        bottom_bar.pack(fill="x", padx=8, pady=(0, 8))

        self.command_entry = ctk.CTkEntry(
            bottom_bar,
            placeholder_text="Type a command and press Enter...",
            font=self._entry_font,
        )
        self.command_entry.pack(side="left", fill="x", expand=True, padx=(0, 8))
        self.command_entry.bind("<Return>", self._on_send)
        self.command_entry.bind("<Up>", self._on_history_up)
        self.command_entry.bind("<Down>", self._on_history_down)

        send_button = ctk.CTkButton(
            bottom_bar, text="Send", width=50, command=self._on_send
        )
        send_button.pack(side="right")

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def log_packet(self, text, timestamp=None):
        """
        Append a line to the log, e.g. log_packet("001 GET 005").
        Safe to call from the main/UI thread only -- if you're calling
        this from a background thread, marshal it via window.after(0, ...)
        instead of calling it directly.
        """
        ts = timestamp or datetime.now().strftime("%H:%M:%S.%f")[:-3]
        self._entries.append((ts, text))
        self._append_line(ts, text)

    def clear(self):
        self._entries.clear()
        self.textbox.configure(state="normal")
        self.textbox.delete("1.0", "end")
        self.textbox.configure(state="disabled")

    # ------------------------------------------------------------------
    # Rendering
    # ------------------------------------------------------------------

    def _format_line(self, ts, text):
        if self.show_timestamps:
            return f"[{ts}] {text}\n"
        return f"{text}\n"

    def _append_line(self, ts, text):
        self.textbox.configure(state="normal")
        self.textbox.insert("end", self._format_line(ts, text))
        self.textbox.configure(state="disabled")

        if self.autoscroll:
            self.textbox.see("end")

    def _rerender(self):
        # Rebuild the whole textbox from stored entries. Used when the
        # timestamp display setting changes, so existing lines update too.
        was_at_bottom = self._is_at_bottom()

        self.textbox.configure(state="normal")
        self.textbox.delete("1.0", "end")
        for ts, text in self._entries:
            self.textbox.insert("end", self._format_line(ts, text))
        self.textbox.configure(state="disabled")

        if self.autoscroll or was_at_bottom:
            self.textbox.see("end")

    def _toggle_timestamps(self):
        self.show_timestamps = not self.show_timestamps
        self.timestamps_button.configure(
            text="Timestamps",
            fg_color=("#3B8ED0", "#1F6AA5") if self.show_timestamps else "gray30",
        )
        self._rerender()

    # ------------------------------------------------------------------
    # Autoscroll toggle
    # ------------------------------------------------------------------

    def _toggle_autoscroll(self):
        self.autoscroll = not self.autoscroll
        self._refresh_autoscroll_button()
        if self.autoscroll:
            self.textbox.see("end")

    def _refresh_autoscroll_button(self):
        if self.autoscroll:
            self.autoscroll_button.configure(
                text="Auto-scroll", fg_color=("#3B8ED0", "#1F6AA5")
            )
        else:
            self.autoscroll_button.configure(fg_color="gray30")

    def _on_manual_scroll(self, _event):
        # If the user scrolls manually, respect it: don't fight them by
        # snapping back to the bottom on the next packet unless they're
        # already at the bottom. This flips the toggle to match reality.
        if self.autoscroll and not self._is_at_bottom():
            self.autoscroll = False
            self._refresh_autoscroll_button()

    def _is_at_bottom(self):
        # yview() returns (top_fraction, bottom_fraction) of visible region
        _, bottom = self.textbox.yview()
        return bottom >= 0.999

    # ------------------------------------------------------------------
    # Command entry + history
    # ------------------------------------------------------------------

    def _on_send(self, _event=None):
        text = self.command_entry.get()
        if not text:
            return

        self._command_history.append(text)
        if len(self._command_history) > self.MAX_HISTORY:
            self._command_history.pop(0)
        self._history_index = None

        self.command_entry.delete(0, "end")

        if self.send_callback:
            self.send_callback(text)

    def _on_history_up(self, _event=None):
        if not self._command_history:
            return "break"

        if self._history_index is None:
            self._history_index = len(self._command_history) - 1
        elif self._history_index > 0:
            self._history_index -= 1

        self._set_entry_text(self._command_history[self._history_index])
        return "break"  # prevent cursor from jumping to start of entry

    def _on_history_down(self, _event=None):
        if self._history_index is None:
            return "break"

        if self._history_index < len(self._command_history) - 1:
            self._history_index += 1
            self._set_entry_text(self._command_history[self._history_index])
        else:
            self._history_index = None
            self._set_entry_text("")

        return "break"

    def _set_entry_text(self, text):
        self.command_entry.delete(0, "end")
        self.command_entry.insert(0, text)


if __name__ == "__main__":
    # Quick manual test harness
    root = ctk.CTk()
    root.geometry("500x600")

    def on_send(cmd):
        print(f"Sending: {cmd!r}")
        logs.log_packet(f"echo: {cmd}")

    logs = LogsFrame(root, send_callback=on_send)
    logs.pack(fill="both", expand=True)

    # simulate some incoming packets
    logs.log_packet("001 GET 005")
    logs.log_packet("001 HYG 005 HIGH")
    logs.log_packet("002 SET 010 LOW")

    root.mainloop()
