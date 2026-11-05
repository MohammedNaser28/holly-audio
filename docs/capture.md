# Capture workflow — sniffing HollyAudio's traffic to the dongle

Goal: record the exact USB transfers the Android app "HollyAudio" issues for one
action per capture file, so every byte in `docs/protocol.md` traces to a real frame.

Verified device on this laptop (source: `inputs/lsusb-v.txt`):

```
3547:0407  Shenzhen Hollyland Technology Co.,Ltd Wireless Microphone
Bus 006, Device 006   (addresses change on replug — re-check with lsusb)
```

Control-channel candidates (see `docs/device.md`): IF0 HID (EP 0x81 IN / 0x06 OUT int)
and IF3 vendor bulk (EP 0x82 IN / 0x05 OUT bulk). We capture **both** and let the
diffs tell us which one the app actually uses. Do not pre-filter interfaces.

## One-time setup (Arch)

```sh
sudo pacman -S --needed qemu-desktop
```

## Android environment: QEMU + Android-x86 (primary)

Waydroid cannot hand a general USB host device (our dongle) to the container —
the guest has no USB host stack for it. Use a QEMU VM instead: the dongle is
passed through with `-device usb-host,vendorid=0x3547,productid=0x0407`, and
since the guest's USB traffic flows through the **host** kernel (QEMU emits it
via usbfs), host-side `usbmon` still captures everything.

ABI warning: HollyAudio 3.1.0 ships **armeabi-v7a only** — use an image with
libhoudini ARM translation (Android-x86 / Prime OS, not modern Bliss).

Setup (one-time):

```sh
sudo pacman -S --needed qemu-desktop
# download an Android-x86 ISO with houdini, e.g. from android-x86.org/download
tools/qemu-android.sh install -i ~/Downloads/android-x86.iso   # boots the ISO installer
tools/qemu-android.sh run                                      # next boots, from disk
adb connect 127.0.0.1:5555                                     # sideload the APK from host:
adb install /tmp/opencode/xapk/com.hollyland.larkc1.apk         # path created during RE; keep a copy of the xapk in the project dir as input
```

## Per-capture procedure

The bus/device numbers change on replug — always re-check first:

```sh
lsusb | grep -i hollyland      # → Bus 006 Device 006: ID 3547:0407 ...
```

1. Boot the VM (dongle plugged in):

   ```sh
   tools/qemu-android.sh run       # or .../install -i <iso> on first boot
   adb connect 127.0.0.1:5555      # optional; lets you control the app via scrcpy/adb
   ```

2. Start the capture (root needed for usbmon):

   ```sh
   sudo tools/capture.sh -b 6 -d 6 -t 25 -o captures/raw/<action>.pcap
   ```

   Repeat **one action per file**, with ~3 s of idle before/after the action so
   periodic frames can be separated from the action's frames.

3. While it runs, in the **QEMU VM session** drive **exactly one** HollyAudio action.

4. Export raw payload rows and eyeball the first frames:

   ```sh
   python3 tools/pcap_to_frames.py captures/raw/<action>.pcap --bus 6 --device 6 \
       -o captures/<action>.jsonl
   ```

## Action list (one file each)

| name | action | expected side |
|---|---|---|
| `idle` | nothing, 30 s | baseline polling |
| `pair` | pair/begin-pairing flow in app | ~10 s window |
| `unpair` | unpair a mic | |
| `gain-up` | one gain + step on ch1 | |
| `gain-down` | one gain − step, same channel | |
| `nc-on` | enable noise cancellation | |
| `nc-off` | disable noise cancellation | |
| `monitor-on` | enable ear-monitor | |
| `monitor-off` | disable ear-monitor | |
| `mute-mic` | mute a mic in app | |
| `unmute-mic` | unmute mic | |
| `status-open` | open the app's main status screen after fresh replug | initial status dump |

File naming: exactly these names in `captures/` — the JSONL export is what gets
referenced in `docs/protocol.md`, raw `.pcap` stays in `captures/raw/`.

## Notes

- usbmon shows host-side view of both control-channel candidates, so one capture
  per action is enough — we diff interfaces afterwards.
- If a needed app action can't be triggered in QEMU/Android-x86 (houdini
  translation failing on some lib, permission crash), the alternative is a
  real rooted phone + OTG — but then usbmon capture on the host won't see
  anything; you'd capture on the phone instead. Prefer fixing the VM first.
- Do NOT use `usbpcap` on the Windows host for the Android-side traffic — we need
  Linux captures the Python expander can parse.
- Root-only airgap discipline: capture hosts' `usbmon` reads are read-only; the app
  writes frames its own way, we never inject frames.
