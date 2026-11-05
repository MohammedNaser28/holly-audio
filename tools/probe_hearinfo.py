#!/usr/bin/env python3
"""Direct probe of the Hollyland 6108 dongle - status poll only (cmd 31, read-only).

Two transports attempted, in fidelity order:
  1. usbfs CONTROL on /dev/bus/usb — byte-identical to what HollyAudio does
     (controlTransfer SET_REPORT/GET_REPORT, docs/protocol.md §1)
  2. hidraw HIDIOCGFEATURE fallback

Nothing here writes settings; GET_HEART_INFO (31) is the app's own status poll.
Run with sudo:  sudo python3 tools/probe_hearinfo.py
"""
import argparse
import ctypes
import errno
import fcntl
import glob
import os
import struct
import sys

# docs/protocol.md §1: wValue = reportType 3 (Feature) << 8 | reportId 5
WVALUE = 0x0305
HID_IFACE = 0  # control interface = IF0 (HID), per lsusb + app findSuitableInterface
CTRL_BUF = 64
# docs/protocol.md §4: GET_HEART_INFO = cmd 31, empty payload => len bytes 00 00, EF at 7
FRAME = bytes([0x05, 0x03, 0xAA, 0xDD, 31, 0x00, 0x00, 0xEF]) + b"\x00" * 56

if sys.maxsize != 0x7FFFFFFFFFFFFFFF or struct.calcsize("P") != 8:  # noqa: F481
    sys.exit("probe assumes x86_64 (u64 data pointers); port before reuse")

LIBC = ctypes.CDLL(None, use_errno=True)  # bare syscall wrappers


def errno_name(e):
    return errno.errorcode.get(e, str(e))


def find_usbfs_node():
    """Locate /dev/bus/usb entry for 3547:0407 by reading the device descriptor."""
    for path in sorted(glob.glob("/dev/bus/usb/*/*")):
        try:
            with open(path, "rb") as fh:
                d = fh.read(18)
            if len(d) >= 14 and struct.unpack_from("<HH", d, 8) == (0x3547, 0x0407):
                return path
        except OSError:
            continue
    return None


def qemu_holding_dongle():
    """Detect the Android VM - its usb-host passthrough claims the dongle and
    makes any host-side usbfs class request return EBUSY (checkintf)."""
    import subprocess
    r = subprocess.run(
        ["pgrep", "-fa", "qemu-system"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )
    return [l for l in r.stdout.splitlines() if "0x3547" in l]


def usbfs_probe():
    """Control transfers via USBDEVFS_CONTROL, identical to the app's calls."""
    vm = qemu_holding_dongle()
    if vm:
        print("usbfs: BLOCKED - the Android VM currently owns the dongle:")
        for l in vm:
            print("   ", l)
        print("host probe needs the device: shut the VM down and rerun.")
        return
    node = find_usbfs_node()
    if not node:
        print("usbfs: no /dev node for 3547:0407 (dongle unplugged?)")
        return
    print(f"usbfs node: {node}")
    fd = os.open(node, os.O_RDWR)
    try:
        # USBDEVFS_DISCONNECT_CLAIM = _IOR('U', 27, struct usbdevfs_disconnect_claim)
        # (measured with kernel headers: 0x8108551B, sizeof=264:
        #  u32 interface, u32 flags, char driver[256])
        # flags 0 + empty driver name = disconnect whichever kernel driver
        # holds IF0 (usbhid) and claim the interface for this fd in one shot.
        DISCONNECT_CLAIM = (2 << 30) | (264 << 16) | (0x55 << 8) | 27  # 0x8108551B
        claim = struct.pack("<II256s", HID_IFACE, 0, b"\x00" * 256)
        fcntl.ioctl(fd, DISCONNECT_CLAIM, claim, False)
        print("claimed IF0 (kernel usbhid detached) via USBDEVFS_DISCONNECT_CLAIM")

        # USBDEVFS_CONTROL = _IOWR('U', 0, struct usbdevfs_ctrltransfer)
        # (kernel headers: dirn 3, not _IOW — encoding dirn 1 gives plain ENOTTY,
        #  measured 0xC0185500 on this Arch host)
        # x86_64 struct: field bytes 0..10, pad to 16, data* at 16, size 24.
        # Python 3.14 dropped struct 'P'; we pack the pointer as explicit u64 'Q'.
        USBDEVFS_CONTROL = (3 << 30) | (24 << 16) | (0x55 << 8) | 0  # 0xC0185500

        out_buf = ctypes.create_string_buffer(bytes(FRAME), len(FRAME))
        in_buf = ctypes.create_string_buffer(CTRL_BUF)  # writable: kernel fills it

        def ctrl(rtype, request, wlength, buf, timeout_ms):
            pkt = struct.pack(
                "<BBHHHH6xQ",
                rtype, request, WVALUE, HID_IFACE, wlength, timeout_ms,
                ctypes.addressof(buf),
            )
            # Direct libc call: CPython 3.14 fcntl.ioctl returns the buffer copy
            # (bytes) for any buffer arg even with mutate_flag=False, hiding the
            # syscall's integer return - measured empirically (FIONREAD probe).
            # libc.ioctl gives the real retval = bytes transferred.
            n = LIBC.ioctl(fd, USBDEVFS_CONTROL, pkt)
            if n < 0:
                e = ctypes.get_errno()
                raise OSError(e, f"USBDEVFS_CONTROL: {os.strerror(e)}")
            return n, buf.raw[:n]

        # write leg: SET_REPORT(Feature) = bRequest 9, wValue 0x0305 (docs/protocol.md §1)
        try:
            n, _ = ctrl(0x21, 9, len(FRAME), out_buf, 3000)
            print(f"SET_REPORT (Feature, id 5): {n} bytes transferred")
        except OSError as e:
            print(f"SET_REPORT leg: {errno_name(e.errno)} ({e})")
        # read leg: GET_REPORT(Feature) = bRequest 1, wValue 0x0305, 64-byte buffer
        try:
            n, rx = ctrl(0xA1, 1, CTRL_BUF, in_buf, 1000)
            decode_reply("GET_REPORT (Feature, id 5)", rx, n)
        except OSError as e:
            print(f"GET_REPORT leg: {errno_name(e.errno)} ({e})")
    except OSError as e:
        print(f"usbfs claim: {errno_name(e.errno)} ({e})")
    finally:
        try:
            fcntl.ioctl(fd, (2 << 30) | (4 << 16) | (0x55 << 8) | 16,  # RELEASEINTERFACE
                        struct.pack("<i", HID_IFACE), False)
        except OSError:
            pass
        os.close(fd)


def hidraw_probe():
    """Fallback: the same Feature report through hidraw ioctls."""
    nodes = [
        os.path.join("/dev", n)
        for dev in glob.glob("/sys/bus/hid/devices/0003:*3547:0407*")
        for n in os.listdir(os.path.join(dev, "hidraw"))
    ]
    if not nodes:
        print("hidraw: no node for 0003:3547:0407")
    for node in nodes:
        try:
            fd = os.open(node, os.O_RDWR)
        except OSError as e:
            print(f"{node}: open failed {errno_name(e.errno)}")
            continue
        try:
            # HIDIOCGFEATURE(len) = _IOC(READ=2, 'H', 0x07, len)
            # hidraw reads the wanted report id from buffer[0] -> must pre-fill 5,
            # (id 0 does not exist in the descriptor -> EINVAL before any device IO)
            # descriptor: FEATURE report id 5 = 63B payload, 64 with id byte
            # (inputs/hid-report-descriptor.hex, parsed)
            rx = ctypes.create_string_buffer(CTRL_BUF)
            rx[0] = bytes([5])
            req = (2 << 30) | (CTRL_BUF << 16) | (0x48 << 8) | 0x07
            n = fcntl.ioctl(fd, req, rx, True)
            decode_reply(f"{node} HIDIOCGFEATURE", rx.raw[:max(n, 0)], n)
        except OSError as e:
            print(f"{node} GET feature: {errno_name(e.errno)} ({e})")
        finally:
            os.close(fd)


def decode_reply(tag, data, n):
    """Decode against ProtocolV2 framing so discrepancies scream loudly."""
    print(f"\n== {tag}: {n} bytes ==")
    print("raw:", " ".join(f"{x:02X}" for x in data))
    if len(data) >= 5:
        magic = data[:4].hex()
        cmd = data[4]
        be_len = (data[5] << 8) | data[6]
        view = {
            "magic": magic,
            "cmd": f"0x{cmd:02X}",
            "len_be": be_len,
        }
        if 7 + be_len < len(data):
            view["tail@7+len"] = f"0x{data[7 + be_len]:02X}"
        print("decode:", view)
        if magic == "0503aadd":
            print("  [wire] magic 05 03 AA DD confirmed by device reply")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hidraw-only", action="store_true", help="skip usbfs, use hidraw only")
    args = ap.parse_args()
    if args.hidraw_only:
        hidraw_probe()
        return
    usbfs_probe()
    print("\n-- hidraw fallback --")
    hidraw_probe()


if __name__ == "__main__":
    main()
