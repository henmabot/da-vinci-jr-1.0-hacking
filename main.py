import time

from pyfirmata2 import Arduino

board = Arduino("/dev/tty.usbmodem101")
time.sleep(2)
while board.bytes_available():
    board.iterate()

raw_response = {}


def capability_handler(*data):
    raw_response["data"] = data
    print("RAW CAPABILITY RESPONSE:", data)


board.add_cmd_handler(0x6C, capability_handler)  # CAPABILITY_RESPONSE
board.send_sysex(0x6B, [])  # CAPABILITY_QUERY

time.sleep(1)
while board.bytes_available():
    board.iterate()

print("Handler got:", raw_response.get("data"))
