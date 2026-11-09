#!/usr/bin/env python3
"""tools/pcap_to_frames.py — export Hollyland dongle data-payload transfers to JSONL.

Dumps every USB data payload (control-data, interrupt, bulk) for a given bus/device
into captures/*.jsonl, one JSON object per transfer:

    {"ts": 1.234567, "dir": "out", "ep": "0x05", "type": "bulk", "len": 64, "hex": "..."}

Direction is derived from the endpoint address bit 7 (0=OUT host->dev, 1=IN dev->host),
as recorded by usbmon. Requires tshark on PATH.

    python3 tools/pcap_to_frames.py captures/gain-up.pcap \
        --bus 6 --device 6 -o captures/gain-up.jsonl

With --setup, control transfer (EP0) SETUP packets are also emitted, which lets the
analyst see setup.opcode/setup.bRequest for any controlTransfer()-style commands:
add  --setup  and rows carrying type "ctl-setup" will contain the 8-byte setup word.
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

FIELDS = [
    "frame.number", "frame.time_epoch",
    "usb.bus_id", "usb.device_address",
    "usb.endpoint_address", "usb.endpoint_address.direction",
    "usb.transfer_type", "usb.data_len",
    "usb.capdata", "usb.data_fragment", "usbhid.data", "usb.iso.data",
    "usb.setup.bRequest",  # was usb.setup.opcode (removed in tshark 4.x;
    #  checked via `tshark -G fields`, 4.7.4)
]

TRANSFER_TYPES = {0: "iso", 1: "interrupt", 2: "control", 3: "bulk"}


def run_tshark(pcap: Path, extra_filter: str | None, with_setup: bool) -> str:
    fields = list(FIELDS)
    if with_setup:
        fields += ["usb.setup.bRequest", "usb.setup.wValue", "usb.setup.wIndex", "usb.setup.wLength"]
    cmd = ["tshark", "-r", str(pcap), "-Y", "usb.data_len > 0", "-T", "fields"]
    if extra_filter:
        cmd[3] = extra_filter
    cmd += ["-E", "separator=,"] + sum((["-e", f] for f in fields), [])
    if shutil.which("tshark") is None:
        sys.exit("tshark missing: sudo pacman -S wireshark-cli")
    r = subprocess.run(cmd, capture_output=True, text=True, check=True)
    if r.stderr:
        print(r.stderr, file=sys.stderr)
    return r.stdout, fields


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("pcap", type=Path)
    ap.add_argument("-o", "--output", type=Path, default=None,
                    help="output .jsonl (default: alongside the pcap)")
    ap.add_argument("--bus", type=int, help="filter by usb.bus_id in-session")
    ap.add_argument("--device", type=int, help="filter by usb.device_address in-session")
    ap.add_argument("--iso-data", action="store_true",
                    help="include isochronous audio payloads (default: skip, they are not control frames)")
    ap.add_argument("--setup", action="store_true",
                    help="also emit EP0 SETUP packets for control transfers")
    args = ap.parse_args()

    out = args.output or args.pcap.with_suffix(".jsonl")
    text, fields = run_tshark(args.pcap, None, args.setup)
    written = skipped = 0
    with out.open("w") as fh:
        for line in text.splitlines():
            if not line.strip():
                continue
            # maxsplit against the RUNTIME field list: with --setup the
            # setup columns trail FIELDS and must stay separate (pre-existing
            # bug: maxsplit was frozen to len(FIELDS), merging the ctl tail).
            parts = line.split(",", len(fields) - 1)
            if len(parts) < len(fields):
                continue
            row = dict(zip(fields, parts))
            ttype = int(row["usb.transfer_type"], 0) if row["usb.transfer_type"] else None
            if ttype is None:
                continue
            etype = TRANSFER_TYPES.get(ttype, f"t{ttype}")
            if etype == "iso" and not args.iso_data:
                skipped += 1
                continue
            usb_bus = row["usb.bus_id"]
            usb_dev = row["usb.device_address"]
            if args.bus and usb_bus != str(args.bus):
                continue
            if args.device and usb_dev != str(args.device):
                continue
            # Payload source differs per tshark/stream shape (all checked
            # live against captures/raw/set_eq_roundtrip.pcap with tshark
            # 4.7.4): OUT control submits dissect as `usb.data_fragment`,
            # IN control callbacks dissect as `usbhid.data` (old captures'
            # `usb.capdata` stays first for their format).
            hexdata = (
                row.get("usb.capdata")
                or row.get("usb.data_fragment")
                or row.get("usbhid.data")
                or row.get("usb.iso.data")
                or ""
            )
            if not hexdata and row.get("usb.setup.bRequest", None) in (None, ""):
                continue
            epid = int(row["usb.endpoint_address"], 0) if row["usb.endpoint_address"] else 0
            # tshark 4.7 removed `usb.endpoint_direction` (field inventory
            # checked live via `tshark -G fields`); the direction companion
            # of `usb.endpoint_address` is `usb.endpoint_address.direction`,
            # printed as a raw `1`/`0` value.
            rdir = row["usb.endpoint_address.direction"]
            direction = "in" if (rdir in ("in", "1") or (rdir not in (None, "") and epid & 0x80)) else "out"
            rec: dict = {
                "frame": int(row["frame.number"]),
                "ts": float(row["frame.time_epoch"]),
                "dir": direction,
                "ep": f"0x{epid:02x}",
                "type": etype,
                "len": int(row["usb.data_len"] or 0, 0),
                "hex": hexdata.lower().replace(":", ""),
            }
            if args.setup and row.get("usb.setup.bRequest"):
                rec["ctl"] = {
                    "bRequest": int(row["usb.setup.bRequest"], 0),
                    "wValue": int(row["usb.setup.wValue"], 0),
                    "wIndex": int(row["usb.setup.wIndex"], 0),
                    "wLength": int(row["usb.setup.wLength"], 0),
                }
            fh.write(json.dumps(rec, sort_keys=True) + "\n")
            written += 1
    print(f"{out}: {written} rows written, {skipped} iso frames skipped")


if __name__ == "__main__":
    main()
