#!/usr/bin/env python3
"""tools/residue_check.py — residue-byte hypothesis sweep over captured frames.

The byte behind every device reply's payload (frame byte `7+len`, first
"residue" slot observed at index 24 in the 17-byte heart replies and index 8
in the 1-byte SET ACKs) is tracked as docs/protocol.md open question 2
(`ProtocolV2.parse` never reads it). This tool tests the candidate hypotheses
against EVERY reply captured so far, so each new capture can be added to the
vectors and re-checked in one command:

    python3 tools/residue_check.py

Hypotheses per frame, over three windows (WITHOUT the residue byte itself):
  * sum     — byte sum mod 256 (the candidate the 2026-11-05 session-2
              captures briefly matched, E1/E3/E5, before session 3 killed it)
  * xor     — plain XOR fold; the APK owns one (`ProtocolV2.crcCalculate`,
              ProtocolV2.java:179-186, plain XOR) though `getData()` never
              applies it [code]
  * xor+sal — XOR fold ⊕ a fixed salt (sweep: 0x06, 0x07). Salted variants
              exist because the heart replies fit XOR(payload[0..len-1))
              ⊕ 0x68 (mock.rs replay note — 2026-11-11: len-1, the last
              payload byte/signalMode excluded, NOT the full 17 bytes),
              and XOR of the 7 heart header bytes is a constant 0x6E,
              making that equal XOR(whole 7+len-1 prefix) ⊕ 0x06; the sweep
              found ACK replies fit the full prefix with salt 0x07, so the
              residue VARIES with the command instead of staying 0x75.
  * crc8    — a fixed sweep of common CRC-8 parameter sets
              (poly x init x refin/refout x xorout)
Windows: header+cmd+len+payload (the whole 7+len prefix), the same window
minus the LAST payload byte, cmd+len+payload, payload only, and payload
minus the last byte. Verdicts are counted separately for heart replies and
SET ACKs; rows only print where a hypothesis has at least one hit, to keep
the answer readable. Pure analysis: nothing here feeds the decoder — do NOT
wire any hypothesis into holly-proto/holly-usb without its own evidence gate.

2026-11-11 (reverb round trip, signalMode=1 hearts): the FIRST hearts on
record with payload[16] (signalMode) set to 1 split the two candidate
payload windows — XOR(payload[0..17]) ⊕ 0x68 (the "×10 certified" 17-byte
reading, correct only while signalMode was 0) fails them (0x51 observed vs
0x50 predicted), while XOR(payload[0..16)) ⊕ 0x68 (payload minus its last
byte) fits EVERY heart on record. docs/protocol.md §2 wording follows this
tool's verdict.

Evidence: every vector below is parsed from an evidence file in captures/
(the constant list carries the file + frame identity; frames are the RX legs
exactly as captured, truncated to the 7+len prefix + residue byte). The
payload-only capture `captures/2026-11-06-read-map-verification.txt` logs no
RX frames, so it contributes no residue data point.
"""

from __future__ import annotations

import functools
import sys

# ── vectors ──────────────────────────────────────────────────────────────────
# (label, bytes up to and including the residue, "heart" | "ack")
# Heart replies: 05 03 BB DD 1F 00 11 <17 payload> <residue>; ACKs:
# 05 03 BB DD <cmd> 00 01 <status 00> <residue>. Ranges are protocol.md §2/§5.

def _heart(label: str, payload_hex: str, residue: int) -> tuple[str, bytes, str]:
    body = bytes.fromhex(payload_hex.replace(" ", ""))
    frame = bytes([0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11]) + body + bytes([residue])
    assert len(frame) == 25, label
    return (label, frame, "heart")

def _ack(label: str, cmd: int, residue: int) -> tuple[str, bytes, str]:
    frame = bytes([0x05, 0x03, 0xBB, 0xDD, cmd, 0x00, 0x01, 0x00, residue])
    return (label, frame, "ack")

FRAMES: list[tuple[str, bytes, str]] = [
    # session 1 — captures/2026-11-05-hearinfo-first-wire.txt, battery 0x63
    _heart("s1  eq3 lv0 b0x63", "00 01 00 63 00 00 01 00 03 00 00 00 03 00 02 00 00", 0x09),
    # session 2 — captures/2026-11-05-set-noise.txt E1/E3/E5, battery 0x5D
    _heart("s2-E1 eq3 lv0 b0x5D", "00 01 00 5D 00 00 01 00 03 00 00 00 03 00 02 00 00", 0x37),
    _heart("s2-E3 eq3 lv0 b0x5D", "00 01 00 5D 00 00 00 00 03 00 00 00 03 00 02 00 00", 0x36),
    _heart("s2-E5 eq3 lv0 b0x5D", "00 01 00 5D 00 00 01 00 03 00 00 00 03 00 02 00 00", 0x37),
    _ack("s2-E2/E4 ack", 0x13, 0x75),
    # session 3a/3b — captures/2026-11-05-noise-level-medium.txt
    _heart("s3a eq3 lv1 b0x63", "00 01 00 63 00 00 01 01 03 00 00 00 03 00 02 00 00", 0x08),
    _heart("s3b eq3 lv2 b0x5E", "00 01 00 5E 00 00 01 02 03 00 00 00 03 00 02 00 00", 0x36),
    _ack("s3a ack", 0x13, 0x75),
    # EQ round trip — captures/raw/set_eq_roundtrip.pcap exported as
    # captures/set_eq_roundtrip.jsonl frames 17/21 (eqA), 29/33 (eqB),
    # 41/45 (eqC), 25/37 (ack): TX1 connected, battery 0x54, NC on level 0,
    # reverb off (gear 0); eqLevel 3 -> 2 -> 3; SET_EQ cmd 07. Bodies are
    # read VERBATIM from the jsonl `hex` column (frame bytes 7..24+residue).
    _heart("eqA eq3 lv0 b0x54 r17", "01 00 54 00 00 00 01 00 03 00 00 00 03 00 00 00 00", 0x3C),
    _heart("eqB eq2 lv0 b0x54", "01 00 54 00 00 00 01 00 03 00 00 00 02 00 00 00 00", 0x3D),
    _heart("eqC eq3 lv0 b0x54 r41", "01 00 54 00 00 00 01 00 03 00 00 00 03 00 00 00 00", 0x3C),
    _ack("eqW/r ack", 0x07, 0x61),
    # Gain round trip — captures/raw/set_gain_roundtrip.pcap exported as
    # captures/set_gain_roundtrip.jsonl frames 17/21/41/45 (gA, voice 2),
    # 29/33 (gB, voice 1), 25/37 (ack cmd 03): TX1 connected, battery 0x2B,
    # NC on level 2, EQ pad 1, light on, reverb off; voiceLevel 2 -> 1 -> 2.
    # Bodies read VERBATIM from the jsonl `hex` column (frame bytes 7..24+residue).
    _heart("gA lv2 b0x2B r17", "01 00 2B 00 00 00 01 02 02 00 00 00 01 00 00 00 00", 0x42),
    _heart("gB lv1 b0x2B r29", "01 00 2B 00 00 00 01 02 01 00 00 00 01 00 00 00 00", 0x41),
    _ack("gW/r ack", 0x03, 0x65),
    # Reverb round trip — captures/raw/set_reverb_roundtrip.pcap exported as
    # captures/set_reverb_roundtrip.jsonl frames 17/21/53/57 (rA, reverb off
    # gear 0), 29/33 (rB, on gear 0), 41/45 (rC, on gear 1), 25/37/49 (ack
    # cmd 05): TX1 connected, battery 0x3B, NC on level 2, voice 2, EQ pad 2,
    # reverb off gear 0 at start; SET_REVERB legs [01,00] / [01,01] / [00,00].
    # FIRST hearts on record with signalMode (payload[16]) = 1 — they split
    # the 17-byte vs 16-byte checksum windows. Bodies read VERBATIM from the
    # jsonl `hex` column (frame bytes 7..24+residue).
    _heart("rA rev-off g0 b0x3B r17", "01 00 3b 00 00 00 01 02 02 00 00 00 02 00 00 00 01", 0x51),
    _heart("rB rev-on g0 b0x3B r29", "01 00 3b 00 00 00 01 02 02 00 00 00 02 01 00 00 01", 0x50),
    _heart("rC rev-on g1 b0x3B r41", "01 00 3b 00 00 00 01 02 02 00 00 00 02 01 01 00 01", 0x51),
    _ack("rW/r ack", 0x05, 0x63),
]

# ── hypotheses ──────────────────────────────────────────────────────────────

WINDOWS = {
    "head7+len": (0, 0),       # frame[0 .. 7+len)
    "head7+len-1": (0, -1),    # frame[0 .. 7+len-1) — prefix minus the LAST payload byte
    "cmd..payload": (4, 0),
    "payload": (7, 0),         # payload[0 .. len)
    "payload-1": (7, -1),      # payload[0 .. len-1) — minus the last payload byte
}

def xor_fold(data: bytes) -> int:
    return functools.reduce(int.__xor__, data, 0)

# Salted XOR: checksum candidates with a folded-in constant. XOR of the 7
# heart header bytes (05 03 BB DD 1F 00 11) is the constant 0x6E, so the
# payload-minus-last-byte checksum XOR(payload[0..len-1)) ⊕ 0x68 lands here
# as salt 0x06 over the reduced prefix window; the ACK-family salt in the
# full prefix window is 0x07.
XOR_SALTS = (0x06, 0x07)

# CRC-8 sweep: (poly, init, refine_input_reflect_output, xorout). The
# reflected variants cover the Maxim/09/Dallas family; the unreflected the
# ATM/SMBus/octal families. One loop, no hand-rolled exotic names: this is a
# search, not a spec.
CRC8_PARAMSETS = [
    (poly, init, refin, xorout)
    for poly in (0x07, 0x1D, 0x31, 0x9B, 0xEA, 0x12, 0xC6, 0xD5)
    for init in (0x00, 0xFF)
    for refin in (False, True)
    for xorout in (0x00, 0xFF)
]

def crc8(data: bytes, poly: int, init: int, refin: bool, xorout: int) -> int:
    crc = init
    for b in data:
        if refin:
            b = int(f"{b:08b}"[::-1], 2)
        crc ^= b
        for _ in range(8):
            crc = ((crc << 1) ^ poly) & 0xFF if crc & 0x80 else (crc << 1) & 0xFF
    if refin:
        crc = int(f"{crc:08b}"[::-1], 2)
    return crc ^ xorout

# ── evaluation ──────────────────────────────────────────────────────────────

def windows_of(frame: bytes) -> dict[str, bytes]:
    # payload layout for both shapes here: len field at 5..6 (BE, low byte at 6).
    # `hi` offsets the exclusive end by 0 (full window) or -1 (drop the last
    # payload byte — the signalMode/status slot the 2026-11-11 split exposed).
    end = 7 + frame[6]
    return {
        name: frame[lo:end + hi]
        for name, (lo, hi) in WINDOWS.items()
    }

def main() -> None:
    counters: dict[tuple[str, str, str], list[bool]] = {}
    for label, frame, kind in FRAMES:
        residue = frame[7 + frame[6]]
        assert residue == frame[-1], label
        wins = windows_of(frame)
        for wname, data in wins.items():
            counters.setdefault(("sum", wname, kind), []).append(sum(data) % 256 == residue)
            counters.setdefault(("xor", wname, kind), []).append(xor_fold(data) == residue)
            for salt in XOR_SALTS:
                counters.setdefault((f"xor+{salt:02x}", wname, kind), []).append(
                    xor_fold(data) ^ salt == residue
                )
        for paramset in CRC8_PARAMSETS:
            poly, init, refin, xorout = paramset
            for wname, data in wins.items():
                counters.setdefault((f"crc8({poly:02x},{init:02x},r{refin!r},{xorout:02x})", wname, kind), []).append(
                    crc8(data, poly, init, refin, xorout) == residue
                )

    hearts_n = sum(1 for _, _, k in FRAMES if k == "heart")
    acks_n = sum(1 for _, _, k in FRAMES if k == "ack")
    print(f"vectors: {hearts_n} heart replies, {acks_n} SET ACKs "
          "(captures/2026-11-05-*.txt + captures/set_eq_roundtrip.jsonl +\n"
          "captures/set_gain_roundtrip.jsonl + captures/set_reverb_roundtrip.jsonl)\n")

    print(f"{'hypothesis':42} {'hearts':>9} {'acks':>7} {'total':>7}")
    # aggregate per (hypothesis, window) across both kinds; only hypotheses
    # with at least one hit print — a 0-hit row tells nothing new.
    per: dict[tuple[str, str], dict[str, list[int]]] = {}
    for (name, wname, kind), flags in counters.items():
        agg = per.setdefault((name, wname), {"heart": [0, 0], "ack": [0, 0]})
        agg[kind][0] += sum(flags)
        agg[kind][1] += len(flags)
    hits = 0
    for (name, wname), agg in sorted(per.items()):
        h_ok, h_n = agg["heart"]
        a_ok, a_n = agg["ack"]
        total, all_n = h_ok + a_ok, h_n + a_n
        hits += total
        if total:
            print(f"{name:32} {wname:9} {h_ok:>4}/{h_n:<4} {a_ok:>4}/{a_n:<3} {total:>4}/{all_n}")

    # Rule check on the embedded set: heart replies take the 7+len-minus-1
    # prefix window with salt 0x06 (≡ XOR(payload[0..len-1)) ⊕ 0x68 — for
    # the 17-byte hearts: XOR(payload[0..16)) ⊕ 0x68, the signalMode byte
    # EXCLUDED), SET ACKs take the full 7+len prefix with salt 0x07
    # (≡ cmd ⊕ 0x66 for the 1-byte-status shape; the status byte is always
    # 00 so far, so the ACK windows coincide and the distinction is open).
    # The full-payload heart reading (XOR(payload[0..17]) ⊕ 0x68, mock.rs's
    # old wording) is checked separately: it can only agree while
    # signalMode = 0.
    h16 = counters[("xor+06", "head7+len-1", "heart")]
    h17 = counters[("xor+06", "head7+len", "heart")]
    a7 = counters[("xor+07", "head7+len", "ack")]
    h16ok, h16n = sum(h16), len(h16)
    h17ok, h17n = sum(h17), len(h17)
    a7ok, a7n = sum(a7), len(a7)
    if h16ok == h16n and a7ok == a7n and h17ok < h17n:
        print(
            f"\nverdict: the residue byte IS a reproducible XOR-8 checksum,\n"
            f"with the last payload byte EXCLUDED from the heart window:\n"
            f"hearts fit XOR(payload[0..len-1)) ⊕ 0x68 — for the 17-byte\n"
            f"payload, XOR(payload[0..16)) ⊕ 0x68, signalMode excluded\n"
            f"({h16ok}/{h16n} vectors) — while SET ACKs fit the full 7+len\n"
            f"prefix ⊕ 0x07 ({a7ok}/{a7n}; ≡ cmd ⊕ 0x66, n=4 command shapes).\n"
            f"The old 17-byte heart reading XOR(payload[0..17]) ⊕ 0x68 fits\n"
            f"only {h17ok}/{h17n}: it coincided with the 16-byte rule while\n"
            f"signalMode was 0 and is REFUTED by the reverb run's\n"
            f"signalMode=1 hearts (0x51 observed vs 0x50 predicted). The\n"
            f"mock-replay note's 'certified ×10' wording was the 16-byte\n"
            f"rule in disguise. Per-family salt derivation (0x06 vs 0x07)\n"
            f"stays open (protocol.md open question 2); decode behaviour\n"
            f"untouched — no holly-proto change."
        )
    elif h17ok == h17n and a7ok == a7n:
        print(
            f"\nverdict: full-payload 17-byte heart reading holds ({h17ok}/{h17n})\n"
            f"— the signalMode split did not reproduce; re-check the reverb\n"
            f"vectors. ACKs {a7ok}/{a7n} at full prefix ⊕ 0x07."
        )
    else:
        print(
            "\nverdict: no tested hypothesis predicts the residue byte on every\n"
            f"captured heart reply + ACK (best partial hits listed above; {hits} "
            "hit(s) in the\nwhole sweep). The 2026-11-05 sum/session-2 agreement is "
            "reproduced (battery 0x5D\nframes only) and broken by every later "
            "session; the ACK residue itself varies\nwith the command (NC 13 -> "
            "0x75, EQ 07 -> 0x61) at identical payload shapes.\nOpen question 2 "
            "stays open; decode behaviour untouched."
        )

if __name__ == "__main__":
    sys.exit(main())
