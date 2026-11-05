# Hollyland USB receiver dongle — device notes

Source: `inputs/lsusb-v.txt`, captured live with `lsusb -v -d 3547:0407` from the
user's plugged-in dongle (Bus 006, Device 006). No values anywhere in this file
are invented; numbers come straight from that dump.

- **VID:PID** `0x3547:0x0407` — "Shenzhen Hollyland Technology Co.,Ltd Wireless Microphone"
- **USB 1.10, Full Speed (12 Mbps)** — sets the practical ceiling: max packet sizes
  are ≤ 64 bytes for interrupt/bulk, and the isochronous audio packet is 288 B per 1 ms frame.
- Bus powered, 100 mA. One configuration, 4 interfaces.

## Interface map

### IF0 — HID (control candidate A)
- Class 3 (HID), subclass 0, protocol 0. bcdHID 1.11. Report descriptor length 134,
  but **not yet read** (user-level lsusb cannot open the device for report emulation;
  needs `sudo`). Report IDs unknown until then.
- `EP 0x81 IN` interrupt, 64 B, interval 1 (device → host)
- `EP 0x06 OUT` interrupt, 64 B, interval 1 (host → device)

### IF1 — Audio Control (UAC 1.00)
- 1 input terminal: **microphone, 2 channels (L/R)**, terminal ID 4
- Feature unit ID 5 (from terminal 4): Mute + Volume controls (per-channel bmaControls 0)
- 1 output terminal: USB Streaming, terminal ID 6
- Direction confirmed by wiring: terminal 4 (mic) sources terminal 6 (USB streaming),
  i.e. audio flows mics → dongle → host.

### IF2 — Audio Streaming (UAC 1.00)
- Alt 0: no endpoints (idle)
- Alt 1: `EP 0x87 IN` isochronous, 288 B, PCM 48000 Hz, 2 ch, 24-bit (3-byte subframe)
- Alt 2: `EP 0x87 IN` isochronous, 288 B, PCM 48000 Hz, 2 ch, 16-bit (2-byte subframe)
- Sampling-frequency control supported (bmAttributes 0x01 on EP).

### IF3 — Vendor Specific 0xFF / 0xF0 (control candidate B)
- `EP 0x82 IN` bulk, 64 B (device → host)
- `EP 0x05 OUT` bulk, 64 B (host → device)

## Model identification

- Device PID 0x0407 / 1031 = Hollyland internal model **6108**
  (`re/apk/sources/com/hollyland/application/common/protocol/Product.java:5`).
- The APK's UI packages are `larkc1`-branded, but PID 1031 is driven by the
  "USB mic"/HID path (`com.hollyland.usbmic.*`), NOT the 6303/6201 Lark-generation
  providers — none of them claims 1031
  (`re/apk/.../sound/provider/Protocol{6118,6201,6303}Provider.java`, `getProductId()`).
- User has NOT confirmed the marketing model name; treat "6108" as the working
  internal name.

## Open questions (to resolve in Phase 2, evidence required)

| Question | How it will be answered |
|---|---|
| Is the control channel IF0 (HID) or IF3 (vendor bulk)? | **RESOLVED**: IF0 HID, control transfers SET_REPORT/GET_REPORT — see `docs/protocol.md` (still [code]-confidence; [wire] capture to confirm) |
| Does HollyAudio use HID report IDs / ctrl xfers on EP0? | **RESOLVED**: yes — Feature reports 5 (64 B) and 6 (160 B) via controlTransfer; see `docs/protocol.md` |
| Exact dongle product model (user said not Lark C1; APK bundle is `com.hollyland.larkc1.apk`) | user confirmation; note the mismatch until resolved |

## Audio path assumption (from descriptors, high confidence)

The app must NOT touch IF1/IF2: the dongle exposes itself as a standard UAC
microphone (48 kHz stereo). Linux will bind `snd-usb-audio`/`hid-generic` to these.
Control-plane work should claim IF0 or IF3 only.

## Recapture checklist

`inputs/lsusb-v.txt` was written without root, so the HID **report descriptor**
shows `** UNAVAILABLE **`. Re-run with sudo to fill it:

```sh
sudo lsusb -v -d 3547:0407 > inputs/lsusb-v.txt 2>&1
```
