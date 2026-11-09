#!/usr/bin/env python3
"""Monitor screendump -> PNG for reading.

Usage: qsnap.py NAME   (writes /tmp/opencode/NAME.png)

HMP screendump emits PPM even with a .png name, so dump NAME.ppm, poll for
the file, then magick-convert to NAME.png.
"""
import os
import socket
import subprocess
import sys
import time

name = sys.argv[1]
ppm = f"/tmp/opencode/{name}.ppm"
png = f"/tmp/opencode/{name}.png"
log = open("/tmp/opencode/_snap.log", "w")

for f in (ppm, png):
    try:
        os.remove(f)
    except OSError:
        pass

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(2.0)
s.connect("/tmp/opencode/qmon.sock")
time.sleep(0.5)
try:
    s.recv(65536)
except socket.timeout:
    pass

s.sendall(f"screendump {ppm}\n".encode())
ok = False
for _ in range(30):
    time.sleep(0.25)
    if os.path.exists(ppm) and os.path.getsize(ppm) > 1000:
        ok = True
        break
log.write(f"screendump ok={ok}\n")
time.sleep(0.5)
r = subprocess.run(["magick", ppm, png], capture_output=True, text=True)
log.write(f"magick rc={r.returncode} err={r.stderr[:300]}\n")
log.write(f"png size={os.path.getsize(png) if os.path.exists(png) else 0}\n")
log.close()
