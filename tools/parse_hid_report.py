#!/usr/bin/env python3
"""Parse the dongle's HID report descriptor -> all Report IDs with dir+sizes.
Evidence source: inputs/hid-report-descriptor.hex (xxd-style hexdump of sysfs descriptor)."""
import sys, re

txt = open(sys.argv[1]).read()
# xxd format: "offset: 8 x 2-byte groups   ascii" -> hex = first space-separated token
# columns before the double-space ASCII gutter
chunks = ""
for line in txt.splitlines():
    if ":" not in line:
        continue
    body = line.split(":", 1)[1]
    chunks += body.split("  ", 1)[0].replace(" ", "")
data = bytes.fromhex(chunks)
print(f"unhexed {len(data)} descriptor bytes")

reports = {}
gs = {"size": 0, "count": 0, "id": 0}
stack = []
TYPE = {0: "MAIN", 1: "GLOBAL", 2: "LOCAL", 3: "RESV"}
GLOBAL = {0: "USAGE_PAGE", 1: "LOGICAL_MIN", 2: "LOGICAL_MAX", 3: "PHYS_MIN",
          4: "PHYS_MAX", 5: "UNIT_EXP", 6: "UNIT", 7: "REPORT_SIZE",
          8: "REPORT_ID", 9: "REPORT_COUNT", 0xA: "PUSH", 0xB: "POP"}
EMAIN = {8: "INPUT", 9: "OUTPUT", 0xB: "FEATURE", 0xA: "COLLECTION", 0xC: "ENDCOL"}
LLOCAL = {0: "USAGE", 1: "USAGE_MIN", 2: "USAGE_MAX"}

i = 0
while i < len(data):
    b = data[i]
    tag, typ, scode = b >> 4, (b >> 2) & 3, b & 3
    slen = (0, 1, 2, 4)[scode]
    payload = data[i+1:i+1+slen]
    val = int.from_bytes(payload, "little", signed=True) if payload else 0
    if typ == 1:
        name = GLOBAL.get(tag, f"GLOBAL?{tag:x}")
        if name == "REPORT_SIZE": gs["size"] = val
        if name == "REPORT_ID":  gs["id"] = val
        if name == "REPORT_COUNT": gs["count"] = val
    elif typ == 2:
        name = LLOCAL.get(tag, f"LOCAL?{tag:x}")
    elif typ == 0 and tag in EMAIN:
        name = EMAIN[tag]
        if tag == 8:   reports[(gs["id"], "INPUT")]   = gs["size"] * gs["count"]
        elif tag == 9: reports[(gs["id"], "OUTPUT")]  = gs["size"] * gs["count"]
        elif tag == 0xB: reports[(gs["id"], "FEATURE")] = gs["size"] * gs["count"]
        elif tag == 0xA: stack.append(gs.copy())
        elif tag == 0xC: gs = stack.pop()
        name = f"{name}<{gs['id']},{gs['size']}x{gs['count']}={gs['size']*gs['count']}"
    else:
        name = f"TAG{tag:x}/T{typ}"
    print(f"{data[i:i+1+slen].hex():8}{TYPE[typ]:5}{name}")
    i += 1 + slen

print("\n--- device-side report sizes ---")
for (rid, dirn), nb in sorted(reports.items()):
    print(f"report id {rid:3}  {dirn:8}  payload={nb:3} bytes  (+1 id byte = {nb+1})")
