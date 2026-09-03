# protocol.py
#
# Construct the pin control packets to send over the serial port
#
# Host packet structure:
#
# AAA 001 OK?
#
# AAA: command
# 002: pin number (optional for some cmds)
# OK?: end packet
#
# Slave packet structure:
#
# AAA 002 DATA <3
#
# AAA: command
# 002: pin number (optional for some cmds)
# <3: end packet (since OKA and OK was redundant, we use <3)
#
#
# Host commands:
# HAI: basic conn test
# HRU: get device status
# DIR: set pin direction
# GET: read pin value
# SET: write pin value
# PLL: set pin pullup
# LSN: listen for pin value changes
# WYD [DIR/LSN/PLL]: get pin direction, listen, or pullup status
# BYE: reset the device
#
# Slave commands:
# HII: response to HAI
# IAM: send device info/status, respond to HRU
# OKA: response to write/execute commands
# HYG: listener response, sent when a pin value changes
# UMM: error response with error info
# IDK: unknown command
# CYA: sent before resetting the device

host_commands = [
    "HAI",
    "HRU",
    "DIR",
    "GET",
    "SET",
    "PLL",
    "LSN",
    "WYD",
    "BYE",
]

slave_commands = [
    "HII",
    "IAM",
    "OKA",
    "HYG",
    "UMM",
    "IDK",
    "CYA",
]
