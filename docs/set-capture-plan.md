# SET capture plan — EQ, gain, light, reverb (roadmap §1 step 2 via the VM rig)

Goal: pass the §1 step-2 gate per control by **passively capturing HollyAudio
sending the SET frames itself** from the QEMU rig — one capture file per
control, forward + restore inside it, ending verified against the heart.
Passive only: we never write frames (capture.md's airgap discipline).

Scope is exactly four controls, in this order:

1. EQ (SET 7 / GET 8)
2. Gain (SET 3 / GET 4)
3. Indicator light (SET 33)
4. Reverb (SET 5 / GET 6)

EXCLUDED (owner's decision + checklist states): shutdown (39) and identify
(35) parked for now; speaker (37) has zero [wire] value-space observations
and no GUI slot; TX mute (22) stays blocked on protocol.md open question 3
plus live-audio impact. Everything outside §4's UI-setting list (reboot,
OTA, pair, upgrade family) was already out of scope by policy.

Per-control frame predictions and write semantics live in
`docs/set-gate-checklist.md` §3; this file only says HOW to observe them.

## 0. Rig recap (docs/capture.md)

- `tools/qemu-android.sh run` — boots the installed `tools/android-x86.qcow2`
  (Android-x86 9.0 + houdini; Houston obsolete warning aside, the only build
  combination proven to run HollyAudio 3.1.0's armeabi-v7a libs), passes the
  dongle through by VID:PID (`-device usb-host,vendorid=0x3547,productid=0x0407`
  — replug-proof), forwards guest adb on `127.0.0.1:5555`, SDL window.
- Host-side usbmon still sees every byte the guest emits (traffic flows
  through the host's usbfs): `sudo tools/capture.sh -b 6 -d <dev> -t 30
  -o captures/raw/<name>.pcap`.
- Export: `python3 tools/pcap_to_frames.py captures/raw/<name>.pcap --bus 6
  --device <dev> -o captures/<name>.jsonl`.

## 1. Pre-flight (once per session)

1. Close every host-side IF0 claimant first (holly-gpui, holly CLI, capture
   pollers) — QEMU grabs the whole device, not just one interface.
2. **Record the starting values host-side, BEFORE the VM boots**
   (`./target/debug/holly status`; pure reads, no writes). Today's expected
   set: eq = Bright (2), gain = 5 ("6"), light = ON (byte 0, inverted),
   reverb = ON gear Large (heart off13=01 off14=02). If anything differs,
   the per-cell rule still holds — forward = any legal OTHER pad, restore =
   the starting pad. VoiceMode/shutdown are not touched but read them too
   for the cross-check.
3. `tools/qemu-android.sh run` (boot ≈ 60–120 s with KVM). When the Android
   home screen is up: `adb connect 127.0.0.1:5555`.
4. Open HollyAudio → the 6108 device page; confirm the receiver shows and
   battery % is updating (the app's own heart poll is our GET/verification
   carrier) and TX2 is connected/unmuted — the APK gates most taps on TX
   status (`HLDevice6302Activity.java:805-811`), a blocked tap wastes the
   window with no frame.

## 2. Universal per-file procedure (each cell)

a. `lsusb | grep -i 3547` → device address `N` (later `--device N`;
   stable all session unless there is a replug — if there is one,
   re-check, per capture.md).
b. Start capture: `sudo tools/capture.sh -b 6 -d N -t 30 -o
   captures/raw/<cell>.pcap`. Do NOT move the dongle during the 30 s.
c. Idle ≥3 s → drive EXACTLY the cell's forward action → idle ≥3 s →
   drive the restore action → idle ≥3 s tail (≥2 heart poll cycles; the app
   polls ~1/s). Driving options: hand taps in the SDL window, or adb
   (`adb shell uiautomator dump` → bounds → `adb shell input tap x y`).
d. Expand: `python3 tools/pcap_to_frames.py captures/raw/<cell>.pcap --bus 6
   --device N -o captures/<cell>.jsonl`.

**Deviation from capture.md's "one action per file"** (deliberate, stated):
one cell = one *logical* reversible action; its file holds forward + restore
separated by ≥3 s idles. The idle rule's purpose (separate periodic frames
from the action's frames) is preserved; but each file now independently
proves shape + ACK + heart effect + restore on its own wire. Two direction
half-files would add nothing the timestamp order doesn't already give.

## 3. The four cells

| # | cell file | forward action (expected frame) | restore action (expected frame) | heart proof (§5) |
|---|---|---|---|---|
| 1 | `eq-flip` | tap EQ pad "Low" → SET_EQ cmd 7 `[01]` (`HLDevice6302Activity.java:729`, OVERCAST=1) | tap pad "Bright" → cmd 7 `[02]` (`:739`) | off 12: 02→01→02 |
| 2 | `gain-flip` | volume one step DOWN stop "6"→"5" → SET_VOICE_LEVEL cmd 3 `[04]` (`:840-846`, setVoice(selectIndex), identity) | back up "5"→"6" → cmd 3 `[05]` | off 8: 05→04→05 |
| 3 | `light-flip` | indicator toggle OFF → SET_LIGHT cmd 33 `[01]` (`:662`, ON→0/OFF→1 inverted) | toggle ON → cmd 33 `[00]` | off 11: 00→01→00 |
| 4 | `reverb-flip` | reverb toggle OFF → SET_REVERB cmd 5 `[00, 02]` — `[{isOpen},{value}]` switch FIRST (`UsbMessengerV2.java:194`; gear rides unchanged, `:793`) | toggle ON → cmd 5 `[01, 02]` | off 13/14: 01 02→00 02→01 02 |

Notes:
- Cell order is mandated (EQ first — owner's instruction). Gain, light,
  reverb follow; reverb last is also practical: it is the only two-byte
  payload, so its frames additionally certify the repeat of the two-byte
  shape that SET_NOISE already proved (switch byte first).
- All starting values above are today's live reads; per-cell rule in §1.2
  overrides silently if they changed.
- The app's settings page does NOT trigger a dedicated GET after SETs for
  these four (no ProGetLight/Speaker/Shutdown/TxIdentify classes exist);
  verification rides the heart poll — which is captureable and already
  fully decoded (§5). The EQ/GET 8 and reverb/GET 6 replies may appear if
  the page re-enters; record them opportunistically.

## 4. Acceptance criteria (per cell, after export)

1. Enumerate host→device control bursts; inside each action's ±1 s window
   exactly ONE new command burst is allowed, and its cmd byte must match the
   cell's expected cmd (7 / 3 / 33 / 5) with the ~§2 frame
   `05 03 AA DD | cmd | len_be | payload | EF`.
2. Payload bytes: byte-exact against the cell table (§3). Any disagreement →
   STOP the plan; file the delta in docs/protocol.md §6 open questions; do
   not proceed to the next cell until explained.
3. Each SET is followed ≤0.5 s later by a device→host control whose single
   payload byte is 0x00 (ProSet*.receiveData convention). Nonzero → the
   device refused: record the byte, do not re-send more than once, cell is
   a fail (not a plan failure).
4. Within ≤2 s a heart (cmd 31) arrives with the expected post-value at the
   expected offset; after the restore the SAME check against the starting
   value. A heart that never returns to the starting value = incident:
   STOP, file, replug before any further action.
5. Record every frame's residue byte (docs/protocol.md open question 2) in
   the cell's notes — collect, don't interpret.
6. If any bursts show on IF3 bulk instead of/next to IF0 control, capture
   both; note which interface carried the SET. (Not expected; 6108's
   control path decompiled to report-5 controlTransfer.)
7. Battery byte drift inside a file is fine (known pattern: session-3b).

## 5. Teardown (after all cells or any stop)

1. Leave the app (Home), shut Android down from the guest UI, then quit QEMU.
2. Wait for the host kernel rebind: `udevadm settle && readlink
   /sys/bus/usb/devices/6-1:1.0/driver` — expect `usbhid`; if `NONE`, one
   replug re-arms it (device.md §claim fix).
3. `./target/debug/holly status` must equal the pre-flight reading (every
   cell self-restored; any drift is an incident to file before anything
   else). No GUI/CLI claim until this matches.

## 6. What a green cell buys

For each passing cell, the checklist row moves from "capture pending" to:
SET frame [wire] (payload + order), ACK [wire], heart effect [wire], restore
[wire] — closing §1 steps 1+2 for that command. Steps 3–4 (holly-proto
encoder, holly-usb op per the `set_noise` pattern, with the guarded
round-trip test) then follow individually. After this pass the remaining
open rows are exactly: 39/35 (excluded here, reinstatable by owner), 37
(decoded value space needed first), 22 (ordinal question), and the
policy-excluded disruptors.
