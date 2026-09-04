# mock_serial.py
#
# A fake serial device for exercising the app without real hardware.
#
# Implements just the subset of pyserial's `Serial` API that the rest of
# this package uses (`write`, `readline`), so it can be dropped in place
# of a real `serial.Serial` connection. Every host packet it receives is
# printed to stdout and answered with a plausible slave packet per
# protocol.py:
#
#   - GET / WYD replies return a random HIGH/LOW (or IN/OUT, ON/OFF for
#     WYD DIR/PLL) -- pin state isn't tracked at all.
#   - DIR / SET / PLL / LSN are always acked immediately.
#   - LSN ... ON starts a background loop that pushes a random HIGH/LOW
#     under the same packet id every 5 seconds until a matching
#     LSN ... OFF is received for that pin.

import random
import threading
import time
from queue import Queue

__all__ = ["MockSerial"]

LISTEN_INTERVAL_S = 5.0


class MockSerial:
    def __init__(self):
        self._inbox: Queue[bytes] = Queue()
        self._listeners: dict[str, bool] = {}
        self._lock = threading.Lock()

    # ------------------------------------------------------------------
    # pyserial.Serial-compatible API
    # ------------------------------------------------------------------

    def write(self, data: bytes) -> int:
        line = data.decode().strip()
        print(f"[mock] <- {line}")
        threading.Thread(target=self._handle, args=(line,), daemon=True).start()
        return len(data)

    def readline(self) -> bytes:
        line = self._inbox.get()
        print(f"[mock] -> {line.decode().strip()}")
        return line

    # ------------------------------------------------------------------
    # Fake device behavior
    # ------------------------------------------------------------------

    def _reply(self, line: str):
        self._inbox.put((line + "\n").encode())

    def _handle(self, line: str):
        parts = line.split()
        if not parts:
            return

        packet_id, command, *rest = parts
        if rest and rest[-1] == "OK?":
            rest = rest[:-1]

        if command == "HAI":
            self._reply(f"{packet_id} HII <3")
        elif command == "HRU":
            self._reply(f"{packet_id} IAM MOCK <3")
        elif command in ("DIR", "SET", "PLL"):
            self._reply(f"{packet_id} OKA <3")
        elif command == "GET":
            pin = rest[0]
            value = random.choice(["HIGH", "LOW"])
            self._reply(f"{packet_id} HYG {pin} {value} <3")
        elif command == "LSN":
            pin, state = rest[0], rest[1]
            self._reply(f"{packet_id} OKA <3")
            if state == "ON":
                self._start_listener(packet_id, pin)
            else:
                self._stop_listener(pin)
        elif command == "WYD":
            pin, kind = rest[0], rest[1]
            if kind == "DIR":
                value = random.choice(["IN", "OUT"])
            elif kind == "PLL":
                value = random.choice(["ON", "OFF"])
            elif kind == "LSN":
                value = "ON" if self._listeners.get(pin) else "OFF"
            else:
                value = "OFF"
            self._reply(f"{packet_id} HYG {pin} {kind} {value} <3")
        elif command == "BYE":
            self._reply(f"{packet_id} CYA <3")
        else:
            self._reply(f"{packet_id} IDK <3")

    def _start_listener(self, packet_id: str, pin: str):
        with self._lock:
            self._listeners[pin] = True

        def loop():
            while True:
                time.sleep(LISTEN_INTERVAL_S)
                with self._lock:
                    if not self._listeners.get(pin):
                        return
                value = random.choice(["HIGH", "LOW"])
                self._reply(f"{packet_id} HYG {pin} {value} <3")

        threading.Thread(target=loop, daemon=True).start()

    def _stop_listener(self, pin: str):
        with self._lock:
            self._listeners[pin] = False
