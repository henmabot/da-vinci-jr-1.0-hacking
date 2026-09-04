# parser.py
#
# Parses incoming packets and sends the outgoing packets
#
# The parser is a small state machine sitting between the UI and the
# serial connection. It's driven entirely by `self.queue`, which carries
# two kinds of tasks:
#
# 1. Serial tasks (produced by the serial reader thread)
#    {
#        "serial": True,
#        "packet": "003 HYG 001 HIGH <3",   # raw line read off the wire
#    }
#
#    These are handled by `parse_serial_packet`, which classifies the
#    slave response and, if it corresponds to a packet we sent, invokes
#    the callback that was registered for that packet id.
#
# 2. Window tasks (produced by the UI, e.g. button clicks)
#    {
#        "window": True,
#        "action": "get_value",   # one of `outgoing_packet_types`
#        "pin": 1,                 # pin id (int), omitted for pin-less actions
#        "value": "HIGH",          # only for actions that carry a value
#        "callback": fn,           # optional: fn(result: dict) -> None
#    }
#
#    These are handled by `parse_window_packet`, which allocates a fresh
#    packet id, builds the wire packet per `protocol.py`, remembers what
#    was sent under that id, and writes it to `self.conn`.
#
# 3. Raw tasks (produced by the log console's manual command entry)
#    {
#        "raw": True,
#        "packet": "010 HAI OK?",   # a fully-formed line, sent as-is
#    }
#
#    These bypass packet-id bookkeeping entirely and are just written
#    straight to the wire -- useful for poking the device manually from
#    the log console.
#
# Every line sent or received (window-built, raw, or read from serial)
# is forwarded to the optional `on_log` callback passed to the
# constructor, so a UI log view can mirror the raw wire traffic without
# the parser needing to know anything about the UI itself.
#
#    `action` / `value` combinations:
#      - "handshake"                                  (no pin, no value)
#      - "get_status"                                 (no pin, no value)
#      - "set_direction"   value: "IN" | "OUT"
#      - "get_value"                                   (no value)
#      - "set_value"       value: "HIGH" | "LOW"
#      - "set_pullup"      value: "ON" | "OFF"
#      - "set_listen"      value: "ON" | "OFF"
#      - "get_direction"                               (no value)
#      - "get_listen"                                  (no value)
#      - "get_pullup"                                  (no value)
#      - "goodbye"                                     (no pin, no value)
#
# `callback` results (dispatched via `_dispatch`) are dicts shaped like:
#   {"type": "handshake_ack", "packet_id": int}
#   {"type": "status",        "packet_id": int, "info": list[str]}
#   {"type": "ack",           "packet_id": int}
#   {"type": "error",         "packet_id": int, "info": list[str]}
#   {"type": "unknown_cmd",   "packet_id": int}
#   {"type": "goodbye_ack",   "packet_id": int}
#   {"type": "data",          "packet_id": int, "pin": str, "value": str}
#   {"type": "data",          "packet_id": int, "pin": str, "kind": str, "value": str}
#
# The two "data" shapes come from the fact that a slave `HYG` reply is
# overloaded: it carries a plain `pin value` pair when replying to
# GET/LSN, but a `pin kind value` triple when replying to WYD (kind is
# one of "DIR"/"LSN"/"PLL"). `parse_serial_packet` disambiguates this by
# looking up what was originally sent under that packet id.
#
# Packet ids are ephemeral: once a response is received, the id is
# forgotten -- except for an active "set_listen" ON subscription, whose
# id is kept alive indefinitely since the slave will keep pushing `HYG`
# updates under that same id every time the pin changes, until a
# matching "set_listen" OFF is sent.

from queue import Queue

from serial import Serial

__all__ = ["PacketParser"]

outgoing_packet_types = [
    "handshake",
    "get_status",
    "set_direction",
    "get_value",
    "set_value",
    "set_pullup",
    "set_listen",
    "get_direction",
    "get_listen",
    "get_pullup",
    "goodbye",
]

incoming_packet_types = [
    "handshake_ack",
    "status",
    "ack",
    "data",
    "error",
    "unknown_cmd",
    "goodbye_ack",
]

# action -> (wire command, numeric code, needs "OK?" suffix, arg builder)
#
# The numeric code mirrors the host command list in protocol.py and is
# used purely to disambiguate incoming HYG replies later on.
_ACTION_TABLE = {
    "handshake": ("HAI", 0, False, lambda t: []),
    "get_status": ("HRU", 1, False, lambda t: []),
    "set_direction": ("DIR", 2, True, lambda t: [t["pin"], t["value"]]),
    "get_value": ("GET", 3, True, lambda t: [t["pin"]]),
    "set_value": ("SET", 4, True, lambda t: [t["pin"], t["value"]]),
    "set_pullup": ("PLL", 5, True, lambda t: [t["pin"], t["value"]]),
    "set_listen": ("LSN", 6, True, lambda t: [t["pin"], t["value"]]),
    "get_direction": ("WYD", 7, False, lambda t: [t["pin"], "DIR"]),
    "get_listen": ("WYD", 8, False, lambda t: [t["pin"], "LSN"]),
    "get_pullup": ("WYD", 9, False, lambda t: [t["pin"], "PLL"]),
    "goodbye": ("BYE", 10, False, lambda t: []),
}

# numeric codes whose HYG reply carries a "kind" (WYD DIR/LSN/PLL)
_WYD_CODES = (7, 8, 9)

# numeric code for set_listen, whose id can outlive a single reply
_LISTEN_CODE = 6


class PacketParser:
    packet_ids: dict[int, dict]
    conn: Serial
    queue: Queue

    def __init__(self, conn: Serial, queue: Queue, on_log=None):
        self.conn = conn
        self.queue = queue
        self.packet_ids = {}
        self._next_id = 1
        self.on_log = on_log

    def _log(self, line: str):
        if self.on_log:
            self.on_log(line)

    # ------------------------------------------------------------------
    # Packet id / formatting helpers
    # ------------------------------------------------------------------

    def _allocate_id(self) -> int:
        pid = self._next_id
        self._next_id = (self._next_id % 999) + 1
        return pid

    @staticmethod
    def _fmt(value) -> str:
        """Format a packet id or pin as a zero-padded 3-digit decimal."""
        return f"{int(value):03d}"

    # ------------------------------------------------------------------
    # Outgoing (window -> serial)
    # ------------------------------------------------------------------

    def parse_window_packet(self, task: dict):
        action = task["action"]
        command, code, needs_ok, build_args = _ACTION_TABLE[action]

        pid = self._allocate_id()
        raw_args = build_args(task)
        args = [
            self._fmt(arg) if key == "pin" else str(arg)
            for key, arg in zip(("pin", "value"), raw_args)
        ]

        parts = [self._fmt(pid), command, *args]
        if needs_ok:
            parts.append("OK?")
        line = " ".join(parts)

        self.packet_ids[pid] = {
            "command": code,
            "action": action,
            "pin": task.get("pin"),
            "callback": task.get("callback"),
            "persistent": action == "set_listen" and task.get("value") == "ON",
        }

        self._log(line)
        self.conn.write((line + "\n").encode())

    def send_raw(self, line: str):
        """Write a fully-formed line straight to the wire, no bookkeeping."""
        self._log(line)
        self.conn.write((line + "\n").encode())

    # ------------------------------------------------------------------
    # Incoming (serial -> window)
    # ------------------------------------------------------------------

    def parse_serial_packet(self, packet: str):
        self._log(packet)

        pckt = packet.split()
        if len(pckt) < 2:
            return

        packet_id = int(pckt[0])
        command = pckt[1]
        record = self.packet_ids.get(packet_id)

        if command == "HII":
            result = {"type": "handshake_ack", "packet_id": packet_id}
        elif command == "IAM":
            result = {"type": "status", "packet_id": packet_id, "info": pckt[2:-1]}
        elif command == "OKA":
            result = {"type": "ack", "packet_id": packet_id}
        elif command == "UMM":
            result = {"type": "error", "packet_id": packet_id, "info": pckt[2:-1]}
        elif command == "IDK":
            result = {"type": "unknown_cmd", "packet_id": packet_id}
        elif command == "CYA":
            result = {"type": "goodbye_ack", "packet_id": packet_id}
        elif command == "HYG":
            result = self._parse_hyg(packet_id, pckt, record)
        else:
            return  # unrecognized slave command, drop silently

        self._dispatch(result, record)
        if command == "CYA":
            self.packet_ids.clear()
        else:
            self._cleanup(packet_id, record)

    def _parse_hyg(self, packet_id: int, pckt: list[str], record: dict | None) -> dict:
        pin = pckt[2]
        command_code = record["command"] if record else None

        if command_code in _WYD_CODES:
            kind, value = pckt[3], pckt[4]
            return {
                "type": "data",
                "packet_id": packet_id,
                "pin": pin,
                "kind": kind,
                "value": value,
            }

        # plain GET reply, or an unsolicited push from an active listener
        value = pckt[3]
        return {"type": "data", "packet_id": packet_id, "pin": pin, "value": value}

    def _dispatch(self, result: dict, record: dict | None):
        if record and record.get("callback"):
            record["callback"](result)
        # else: nothing registered for this id (e.g. a late/unsolicited
        # packet after cleanup) -- silently dropped

    def _cleanup(self, packet_id: int, record: dict | None):
        if record is None:
            return
        if record["command"] == _LISTEN_CODE and record.get("persistent"):
            return  # keep alive: slave will keep pushing HYG under this id
        self.packet_ids.pop(packet_id, None)

    # ------------------------------------------------------------------
    # Main loop
    # ------------------------------------------------------------------

    def start(self):
        while True:
            task = self.queue.get()
            if task.get("serial"):
                self.parse_serial_packet(task["packet"])
            elif task.get("window"):
                self.parse_window_packet(task)
            elif task.get("raw"):
                self.send_raw(task["packet"])
