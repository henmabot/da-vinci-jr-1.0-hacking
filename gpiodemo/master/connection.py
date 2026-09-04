# board.py
#
# Handles the board connection and uart/serial communication

import serial
import serial.tools.list_ports

__all__ = ["connect", "get_packet", "get_ports", "prompt_port"]


def get_ports() -> dict[str, str]:
    ports = serial.tools.list_ports.comports()
    return {port.device: port.description for port in ports}


def connect(port: str, baudrate: int = 115200) -> serial.Serial:
    return serial.Serial(port, baudrate)


def prompt_port() -> serial.Serial:
    ports = get_ports()
    for i, (port, desc) in enumerate(ports.items()):
        print(f"[{i}]: {port}: {desc}")
    print()

    while True:
        port_id = input("Enter port ID: ")
        if not port_id.isdigit():
            print("Invalid port ID")
            continue
        port_id = int(port_id)
        if port_id < 0 or port_id >= len(ports):
            print("port ID out of range")
            continue
        break

    port_name = list(ports.keys())[port_id]
    print(f"\nConnecting to {port_name}...")
    connection = connect(port_name)
    print("Connected successfully!\n")
    return connection


def get_packet(conn: serial.Serial) -> str:
    return conn.readline().decode().strip()


if __name__ == "__main__":
    ser = prompt_port()
    while True:
        print(ser.readline().decode().strip())
