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

## Crash behavior (observed 2026-11-05, `tools/crash_test.sh` stdout)

Question: if a process holding the IF0 claim dies with SIGKILL (no Drop
handler, no usbfs RELEASE ioctl), does the kernel restore the interface so
the next process can claim it, and does the audio stack survive?

Observed, quoted from the script run (probe =
`holly-usb/examples/claim_and_sleep`, one GET then sleep:

```text
--- DURING claim (should show IF0 WITHOUT usbhid) ---
6-1:1.0                driver=usbfs        # kernel shows the claim
6-1:1.1                driver=snd-usb-audio
6-1:1.2                driver=snd-usb-audio
6-1:1.3                driver=NONE

killed probe with SIGKILL (no Drop handler ran)

--- AFTER SIGKILL (task: does usbhid come back to IF0? audio still bound?) ---
6-1:1.0                driver=NONE         # usbfs gone, nothing rebound
6-1:1.1                driver=snd-usb-audio
6-1:1.2                driver=snd-usb-audio
6-1:1.3                driver=NONE         # unchanged
```

- **IF0 DURING the claim: `driver=usbfs`** — the kernel marks a claimed
  interface as usbfs; this is the normal, expected state for HID-class
  interfaces driven by userspace (there is no kernel HID driver on IF0 in
  the idle state either, see the BEFORE note below).
- **IF0 AFTER the SIGKILL: `driver=NONE`** — the dead process's claim was
  cleaned up by the kernel and *no* driver rebound to the interface.
  Consequences for us: a crashed process leaves a clean (unclaimed)
  interface, so the next holly-usb session can claim it — but nothing
  re-arms usbhid for the device, matching the idle baseline where IF0 and
  IF3 both had no kernel driver (`6-1:1.0 driver=NONE`,
  `6-1:1.3 driver=NONE` in this same run's BEFORE section; IF0 had NONE
  also in the very first (partial, sudo-cut) run the same day).
- **The audio stack was untouched by the crash, in all three phases**:
  `snd-usb-audio` stayed bound to IF1/IF2 BEFORE, DURING, and AFTER; ALSA
  still listed `card 2: Microphone [Wireless Microphone] (USB Audio)`
  with its capture subdevice; PipeWire kept
  `alsa_input.usb-Shenzhen_Hollyland_Technology_Co._Ltd_Wireless_Microphone_
  Wireless_Microphone-01.analog-stereo` alive before and after the kill.
- The audio stack was also untouched by *claiming* IF0: DURING the
  controlled claim, accept for IF0's usbfs binding, all audio bindings
  and devices stayed as in BEFORE. Claiming IF0 does not disturb IF1/IF2.

Not asserted (needs evidence): whether a device-node fd left node
permissions (0666 rule) after the kill; none of the checks compare that
state. Also not re-verified by this script: an actual *re-claim* by a
new process after the SIGKILL — the guarded hardware test runs before
the script in its chain and claims cleanly, but an explicit
claim-after-kill step is not in the script.

## Hardware session notes (2026-11-05)

First live GET/SET round-trips through holly-usb/nusb (the
`holly-usb --ignored` tests, all 3 green):

- `open_and_read_heart_info`: live GET through nusb; reply parsed into
  the certified §5 shape, 17 bytes, multi-TX fields all `None`:
  `tx1 off, tx2 on, tx2 battery 76 → 75` (drift between runs, matches
  `docs/protocol.md` §5), `nc on level 0`, `voice 3/0`,
  `shutdown 0`, `light on, eq 3`, `reverb off level 2`, `signal 0`.
- `set_noise_roundtrip_guarded`: GET → NC `on=true level=0`;
  SET off; fresh GET shows `nc off`; SET on; fresh GET returns
  `nc on level 0` with the level byte unchanged; the drop guard's own
  fresh GET logged "NC already at original state (true), nothing to
  do", post-restore switch `true`. **The Task-1 SET_NOISE [wire] vector
  is now exercised by the crate itself on the real wire.**

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
