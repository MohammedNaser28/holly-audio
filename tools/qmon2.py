#!/usr/bin/env python3
"""Send one monitor command and dump every pending monitor reply text.

Usage: qmon2.py CMD...   (args joined with spaces, raw line, no typing)
"""
import socket
import sys
import time

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(2.0)
s.connect("/tmp/opencode/qmon.sock")
time.sleep(0.6)
try:
    s.recv(65536)
except socket.timeout:
    pass

cmd = " ".join(sys.argv[1:])
s.sendall((cmd + "\n").encode())
buf = b""
for _ in range(4):
    time.sleep(1.0)
    try:
        b = s.recv(65536)
        if b:
            buf += b
    except socket.timeout:
        pass
data = (buf.decode(errors="replace")).replace("[K", "").replace("[D", "")
print("CMD:", cmd)
print("REPLY:", data[-1200:])
