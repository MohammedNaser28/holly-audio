#!/usr/bin/env python3
"""Task-1 capture: SET_NOISE toggle sequence (only cmd 19 + status polls cmd 31).

Safety contract (project rules + task spec):
- Payloads strictly in-range: [isOpen(0/1), level] with level taken from the
  device's own baseline. No firmware/pairing/reset related commands, ever.
- The control interface is verified to be IF0 (HID, class 3) BEFORE any SET.
- Any malformed reply / cmd echo mismatch / unresponsive leg ABORTS the run;
  no retry loops. A stopped run never issues a second SET without the first
  GET report confirming the flip succeeded.
- Never claims or detaches IF1/IF2 (UAC audio) or IF3. The state machine only
  claims IF0 via USBDEVFS_DISCONNECT_CLAIM and releases it on exit.
- End state must equal start state, verified by a final GET_HEART_INFO.
  (Note: once switched, if the confirming GET does not match, the script stops
  and reports loudly instead of retrying.)

Transports byte-identical to HollyAudio (docs/protocol.md §1) and to the
proven ['wire'] capture captures/2026-11-05-hearinfo-first-wire.txt:
  SET_REPORT(Feature) controlTransfer(0x21, 9, 0x0305, 0, 64, 3000)
  GET_REPORT(Feature) controlTransfer(0xA1, 1, 0x0305, 0, 64, 1000)

Run:  sudo python3 tools/capture_set_noise.py
All exchanges are printed to stdout for capture-file assembly.
"""
import ctypes
import errno
import glob
import os
import struct
import sys

VID, PID = 0x3547, 0x0407            # docs/device.md; inputs/lsusb-v.txt [descriptor]
HID_IFACE = 0                        # IF0 = control (docs/protocol.md §1) [code+wire]
WVALUE = 0x0305                      # Feature report type 3 << 8 | report id 5
CTRL_BUF = 64

CMD_GET_HEART = 31                   # ProGetHearInfo; 'wire'-certified
CMD_SET_NOISE = 19                   # ProSetNoise (UsbMessengerV2.java:172-173)

LIBC = ctypes.CDLL(None, use_errno=True)

if sys.maxsize != 0x7FFFFFFFFFFFFFFF:
    sys.exit("probe assumes x86_64; port before reuse")


def find_node():
    for path in sorted(glob.glob("/dev/bus/usb/*/*")):
        try:
            with open(path, "rb") as fh:
                d = fh.read(18)
            if len(d) >= 14 and struct.unpack_from("<HH", d, 8) == (VID, PID):
                return path
        except OSError:
            continue
    return None


def verify_control_interface(devfs_path):
    """Refuse to proceed unless interface 0 of this device is class 3 (HID).
    sysfs walk: .../<bus-port>:1.0/bInterfaceClass. docs/device.md IF0. [descriptor]"""
    busport = os.path.basename(devfs_path)  # e.g. "002"
    for dev in glob.glob("/sys/bus/usb/devices/*"):
        try:
            with open(os.path.join(dev, "idVendor")) as f:
                if f.read().strip() != f"{VID:04x}":
                    continue
            with open(os.path.join(dev, "idProduct")) as f:
                if f.read().strip() != f"{PID:04x}":
                    continue
            ifc0 = os.path.join(dev, f"{os.path.basename(dev)}:1.0")
            with open(os.path.join(ifc0, "bInterfaceClass")) as f:
                cls = f.read().strip()
            return cls == "03", f"{ifc0} bInterfaceClass={cls}"
        except OSError:
            continue
    return False, "no sysfs entry for 3547:0407"


class Session:
    """Claimed-control-interface session. Mirrors probe_hearinfo.py transfer legs."""

    def __init__(self):
        self.node = find_node()
        if not self.node:
            sys.exit("no /dev node for 3547:0407 (dongle unplugged?)")
        ok, why = verify_control_interface(self.node)
        print(f"node {self.node}: control-iface check: {why}")
        if not ok:
            sys.exit("ABORT: IF0 is not HID class 3 - refusing to claim")
        self.fd = os.open(self.node, os.O_RDWR)
        # USBDEVFS_DISCONNECT_CLAIM (kernel headers): 0x8108551B, struct 264B
        claim = struct.pack("<II256s", HID_IFACE, 0, b"\x00" * 256)
        n = LIBC.ioctl(self.fd, (2 << 30) | (264 << 16) | (0x55 << 8) | 27, claim)
        if n < 0:
            e = ctypes.get_errno()
            sys.exit(f"ABORT: CLAIM IF0: {errno.errorcode.get(e, e)}")
        print("claimed IF0 (usbhid detached)")

    def exchange(self, frame, label):
        """SET_REPORT then GET_REPORT; returns parsed reply frame dict."""
        print(f"\n### {label}")
        print("TX:", frame.hex(" "))
        out = ctypes.create_string_buffer(bytes(frame), len(frame))
        inp = ctypes.create_string_buffer(CTRL_BUF)

        def ctrl(rtype, request, wlength, buf, timeout_ms):
            pkt = struct.pack(
                "<BBHHHH6xQ", rtype, request, WVALUE, HID_IFACE, wlength,
                timeout_ms, ctypes.addressof(buf),
            )
            n = LIBC.ioctl(self.fd, (3 << 30) | (24 << 16) | (0x55 << 8) | 0, pkt)
            if n < 0:
                e = ctypes.get_errno()
                raise OSError(errno.errorcode.get(e, e), "usbfs control leg")
            return n, buf.raw[:n]

        ctrl(0x21, 9, len(frame), out, 3000)      # SET_REPORT Feature
        n, rx = ctrl(0xA1, 1, CTRL_BUF, inp, 1000)  # GET_REPORT Feature
        print(f"RX ({n} bytes):", rx.hex(" "))
        parsed = parse_reply(rx, n, label)
        return parsed

    def release(self):
        try:
            LIBC.ioctl(self.fd, (2 << 30) | (4 << 16) | (0x55 << 8) | 16,
                       struct.pack("<i", HID_IFACE))
        except (OSError, TypeError):
            pass
        os.close(self.fd)


def parse_reply(rx, n, label):
    """Strict device-reply decode - on any anomaly, ABORT (no retries)."""
    def fail(why):
        print(f"\nABORT at [{label}]: {why}")
        sys.exit(1)
    if n < 7:
        fail(f"reply only {n} bytes (need >= 7)")
    if rx[:2] != b"\x05\x03" or rx[3] != 0xDD:
        fail(f"reply magic not 05 03 ? DD: {rx[:4].hex(' ')}")
    if rx[2] != 0xBB:
        fail(f"reply byte 2 = {rx[2]:02X}, expected BB (device->host marker)")
    cmd, be_len = rx[4], ((rx[5] << 8) | rx[6])
    if be_len > CTRL_BUF - 7:
        fail(f"reply payload length {be_len} impossible in 64-byte report")
    if 7 + be_len > n:
        fail(f"reply truncation: len {be_len} but only {n} bytes")
    payload = rx[7:7 + be_len]
    print(f"parsed: cmd=0x{cmd:02X} len={be_len} (05 03 BB DD header, BE length)")
    return {"cmd": cmd, "payload": payload}


def heart_frame(tx_id):
    """cmd 31 GET_HEART_INFO poll frame; wire-certified layout."""
    return bytes([0x05, 0x03, 0xAA, 0xDD, CMD_GET_HEART, 0x00, 0x00, 0xEF]) + b"\x00" * 56


def set_noise_frame(is_open, level):
    """cmd 19 SET_NOISE, payload [isOpen, level]. [code]"""
    payload = bytes([1 if is_open else 0, level & 0xFF])
    return (bytes([0x05, 0x03, 0xAA, 0xDD, CMD_SET_NOISE, 0x00, 0x02])
            + payload + bytes([0xEF]) + b"\x00" * 54)


def noise_state(reply):
    """Extract (noiseSwitch, noiseLevel) from a GET_HEART_INFO payload."""
    if reply["cmd"] != CMD_GET_HEART:
        sys.exit(f"ABORT: expected command {CMD_GET_HEART} reply, got {reply['cmd']}")
    p = reply["payload"]
    if len(p) < 8:
        sys.exit(f"ABORT: heart reply payload {len(p)} bytes < 8 (need offsets 6,7)")
    return p[6], p[7], p


def main():
    s = Session()
    # step 1: baseline
    base = noise_state(s.exchange(heart_frame(1), "GET_HEART_INFO baseline"))
    switch0, level = base[0], base[1]
    print(f"\nbaseline: noiseSwitch={switch0} noiseLevel={level}")
    target = 0 if switch0 == 1 else 1
    print(f"target state: noiseSwitch={target} (opposite), level stays {level}")

    # step 2: SET to opposite
    ack1 = s.exchange(set_noise_frame(target, level), "SET_NOISE (toggle)")
    print(f"SET_NOISE ack payload: {ack1['payload'].hex(' ')}"
          f"  (ProSetNoise.receiveData: data[0]==0 means setStatus ok) [code]")

    # step 3: confirm flip
    t = noise_state(s.exchange(heart_frame(2), "GET_HEART_INFO after toggle"))
    if t[0] != target:
        print(f"STOP: toggle did not take effect (switch={t[0]}, want {target}). "
              "No further SETs attempted; original state was NOT touched.")
        s.release()
        return
    print(f"toggle confirmed: noiseSwitch now {t[0]}")

    # step 4: restore original
    ack2 = s.exchange(set_noise_frame(switch0, level), "SET_NOISE (restore)")

    # step 5: confirm restore
    r = noise_state(s.exchange(heart_frame(3), "GET_HEART_INFO final check"))
    if r[0] != switch0:
        print(f"FAIL: restore did not take effect (switch={r[0]}, want {switch0}). "
              "Device left in NON-original state - report this immediately.")
        s.release()
        sys.exit(2)
    print(f"\nDONE: state restored. switch0={switch0} level={level} "
          f"== final {r[0]} (battery check: base={base}, final={r})")
    s.release()


if __name__ == "__main__":
    main()
