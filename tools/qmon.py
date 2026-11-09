#!/usr/bin/env python3
"""QEMU HMP monitor driver over /tmp/opencode/qmon.sock.

Usage:
  qmon.py snap FILE          screendump (PPM; convert with qsnap.py)
  qmon.py send CMD...        one raw monitor command line
  qmon.py key K [K...]       sendkey pulses
  qmon.py str TEXT [DWELL]   type TEXT into the guest console, press Enter

Notes (proven 2026-11-06, docs/capture.md):
- sendkey needs ~0.35 s dwell; extra dwell around '_' (shift-minus).
- Console drops chars unpredictably; snap-verify any commit-grade line.
- ':' = shift-semicolon, '"' = shift-apostrophe.
"""
import socket
import sys
import time

SOCK = "/tmp/opencode/qmon.sock"

MAP = {
    " ": "spc", ".": "dot", "-": "minus", "=": "equal", ";": "semicolon",
    "/": "slash", "|": "shift-backslash", ">": "shift-dot", "<": "shift-comma",
    "+": "shift-equal", "_": "shift-minus", "?": "shift-slash", "#": "shift-3",
    ":": "shift-semicolon", "'": "apostrophe", '"': "shift-apostrophe",
}


def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(2.0)
    s.connect(SOCK)
    return s


def drain(s, wait=0.05):
    time.sleep(wait)
    try:
        return s.recv(65536)
    except socket.timeout:
        return b""


def pulse(s, k, dwell=0.35):
    s.sendall(f"sendkey {k}\n".encode())
    time.sleep(dwell)
    drain(s)


def pulse_raw(s, raw, dwell=0.35):
    s.sendall((raw + "\n").encode())
    time.sleep(dwell)
    drain(s)


def keychar(s, ch, dwell=0.35):
    if ch == "_":
        time.sleep(dwell)
        pulse(s, "shift-minus", dwell=dwell)
        time.sleep(dwell)
        return
    if ch.isupper():
        pulse(s, "shift-" + ch.lower(), dwell=dwell)
        return
    pulse(s, MAP.get(ch, ch), dwell=dwell)


def typestr(s, text, dwell=0.35):
    for ch in text:
        keychar(s, ch, dwell)


def newline(s, dwell=0.9):
    pulse(s, "ret", dwell=dwell)


def snap(s, path):
    # HMP screendump writes PPM regardless of filename; qsnap.py converts.
    pulse_raw(s, f"screendump {path}", dwell=1.5)


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    s = connect()
    drain(s, wait=0.6)
    if cmd == "snap":
        snap(s, sys.argv[2])
        print("saved", sys.argv[2])
    elif cmd == "send":
        pulse_raw(s, " ".join(sys.argv[2:]), dwell=1.2)
        print("sent")
    elif cmd == "key":
        for k in sys.argv[2:]:
            pulse(s, k)
        print("keys done")
    elif cmd == "str":
        dwell = float(sys.argv[3]) if len(sys.argv) > 3 else 0.35
        typestr(s, sys.argv[2], dwell)
        newline(s)
        print("typed:", sys.argv[2][:60])
    else:
        print("usage: qmon.py snap FILE | send CMD... | key KEY... | str TEXT [DWELL]")


if __name__ == "__main__":
    main()
