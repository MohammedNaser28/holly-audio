# GUI roadmap — gpui frontend, post-v1 (analysis v0.1)

Status: Task 8 shipped the read-only surface plus the sole verified write
(SET_NOISE toggle) at commit `57ea7d4`. This file lists what is needed to
turn more of the ViewModel's read-only fields into controls, and the
infrastructure that comes after that. Nothing here may be built without the
evidence named in its own section — the repo rule ("no guessed constants;
every cite") applies to the GUI exactly like every other layer.

## 0a. Frontend baseline update (2026-11-05)

- Signal theme tokens are centralized in `frontends/gpui/src/main.rs` (design
  spec §5: background `#111318` … error `#DF7777`); header gained the HOLLY +
  subtitle + connection badge row, footer gained `state received HH:MM:SS
  (UTC)` (from `Snapshot.received_at`, change-gated so the emit contract stays
  change-only) + crate version. Window 760×620.
- `--demo` mode: `holly_app::demo::start_demo()` runs the real actor over the
  scripted `DemoDevice` (certified session-3 payload); UI marks it via title
  suffix, header chip, and footer note. Build/run/tests: `frontends/gpui/README.md`.
- NC level buttons keep the identity mapping committed today
  (docs/protocol.md §5.1: strong = wire 2, medium = wire 1, low = wire 0;
  the phone's numerals 1..6 concern the voice bar, not NC).

## 0. The v1 baseline (what exists and why it is legal)

- Only two operations are ever sent [wire]:
  - GET_HEART_INFO (cmd 31) — status poll, `captures/2026-11-05-hearinfo-first-wire.txt`
  - SET_NOISE (cmd 19) — `captures/2026-11-05-set-noise.txt` E1–E5
- The actor (`holly-app/src/actor.rs`) owns the single IF0 claim; the gpui
  frontend is a thin renderer of `Snapshot -> ViewModel` (VM in
  `holly-app/src/vm.rs`, all device-rule strings including the udev hint).
- Writability rule: a field becomes a control only when SET + ACK for its
  command are **captured** (`captures/`), cross-checked against the
  decompiled producer (`re/apk/sources/com/hollyland/usbmic/protocolV2/`,
  never committed), and exercised by a guarded hardware test in holly-usb
  (see `set_noise_roundtrip_guarded`, commit `0adcd2e`, for the drop-guard
  pattern that restores the original state even on Ctrl-C).
- The command table in docs/protocol.md §4 is the candidate list — all
  [code] confidence until a capture promotes each one.

## 1. Generic gate: read-only field → control

For every candidate command (command number and payload from §4), in order:

1. **APK cross-check.** Read `ProXxx.java` (needData/receiveData) and the
   matching `UsbMessengerV2` call site (`saveReserved` payload layout,
   `UsbMessengerV2.java:105-261` [code]). Record payload order, byte
   semantics, and the ACK-interpretation (`data[0] == 0` pattern in
   `ProSetVoiceLevel.receiveData` etc.).
2. **Passive capture of the app doing it.** Before writing ANY frame, replay
   the official Android app (or WireShark-session of the desktop app, if we
   find one) doing the same UI action and capture the exact 64-byte buffer.
   Two independent examples minimum — the SET_NOISE session showed drift
   between captures (battery++) — and record the residue byte for each
   (docs/protocol.md open question 2).
3. **holly-proto** gains the frame encoder only if the capture matches the
   APK-computed layout; otherwise the capture wins.
4. **holly-usb** gains one operation with the same shape as `set_noise`:
   fresh GET + SET + ack gating (`BadAck`/`Rejected` on the certified
   lengths only), no retry. Add a mock test with the certified vector
   verbatim (pattern: `holly-usb/tests/mock.rs`) AND a guarded hardware
   test that reads the original value, applies, verifies, restores, logs.
5. **holly-app** gains the actor command (latest-wins coalescing is
   already generic — `Command` gets one more variant), the
   `Snapshot`/ViewModel gets the fields, and the *frontend* gets the
   control including its keyboard binding and its disabled-when-no-heart
   rule. VM strings live in `holly-app/src/vm.rs`; the frontend still learns
   nothing device-specific.

Illegal without the above: inventing a fallback stream, a "write anything"
escape hatch, or estimating value ranges. Ranges are open question 4 in
docs/protocol.md until captured.

## 2. Per-control capture procedures

### 2.1 Gain (voiceLevel) — SET_VOICE_LEVEL cmd 3 / GET cmd 4

- APK: `ProSetVoiceLevel`/`ProGetVoiceLevel` + §4 row (payload
  `[{value}]`).
- Capture recipe (session pattern identical to Task 1): plug dongle
  (fresh bus node recorded), start the capture script used for
  `2026-11-05-set-noise.txt` (tools/capture + usbmon/BPF — see note below),
  in the HollyAudio app: sliders → move gain one step, capture E-frame pair;
  move again → second pair. Record GET_VOICE_LEVEL (cmd 4) hex form as read
  by holly too.
- Watch for: does the app SET a *delta* or an absolute? (§4 says absolute.)
- Then run the generic gate §1. UI: a stepper (not slider) until ranges are
  known — clamped range = invented data.

### 2.2 EQ (eqLevel) — SET_EQ cmd 7 / GET cmd 8

- Same as 2.1, `ProSetEq`; payload `[{value}]` per §4. The read side is
  already observed: `eqLevel` (HeartInfo offset 12) read `3` in both
  certified sessions [wire], so the status poll doubles as the GET capture.
  Only the SET pair is missing.

### 2.3 Reverb — SET_REVERB cmd 5 / GET cmd 6

- Different shape: payload `[{isOpen}, {value}]` (two bytes) — pay attention
  to argument order in `UsbMessengerV2.saveReserved` [code] then confirm in
  the capture. The UI control would be a toggle + numeric; both land in ONE
  SET frame per the APK.

### 2.4 NC level (the second byte of SET_NOISE) — SHIPPED 2026-11-05

- Evidence closed in `captures/2026-11-05-noise-level-medium.txt`:
  nonzero level (1) read [wire] after a phone-side write, then a
  guarded write of level 2 verified by fresh GETs, restore to 1
  confirmed — value set 0/1/2 matches the APK's strong/medium/low
  [code `HLDevice6302Activity.java:1593/:1610/:1626-1631`]; §5 notes
  the byte is a per-value enum, not a free scale.
- Shipped chain: holly-usb `set_noise_level(u8)` (fresh GET + SET
  `{isOpen from fresh GET, level}`, ack gates identical to
  `set_noise`), mock tests with the live level-1 heart + APK frame
  bytes, guarded hardware test `set_noise_level_roundtrip_guarded`;
  holly-app `Command::SetNoiseLevel` + `NoiseWrite {Switch, Level}`
  pending representation; the gpui row shows switch toggle (space) +
  low/medium/strong buttons (keys 1/2/3). Buttons stay idle unless the
  verified switch is ON — a level write with NC off has never been
  sent/observed [wire] (the APK always sends `isOpen=true` in level
  presses), so that combination is deliberately not expressible here.
- NOT shipped: any level outside 0/1/2, or level presets for other
  Hollyland products (their byte meanings were never captured).

### 2.5 TX mute — SET_TX_MUTE cmd 22

- Payload `[{txIndex}, {muted}]`; two unknowns (index encoding starting at
  0 or ordinal; whether TX3/TX4 use the same command) — needs the
  `deviceType` ordinal resolution first (§4 TODO + open question 3).
- Capture recipe: mute TX1 only, then TX2 only — two sessions; the index
  byte difference gives the mapping directly. With one plugged TX, mute it
  in the app and capture.

### 2.6 "Monitor" — NOT a protocol item yet (open question)

- No `ProMonitor*` producer name appears in the table we extracted; "the
  dongle as a UAC mic with a monitor path" most plausibly lives on the
  **IF1/IF2 UAC interfaces or IF3**, which this project NEVER claims
  (docs/device.md "Audio path assumption").
- Required evidence FIRST: grep
  `re/apk/sources/com/hollyland/usbmic/` for `monitor|speaker|au` to rank
  `SET/GET_SPEAKER (37/38)` vs a UAC-side feature — then a capture of the
  physical app with a headphone playing while the monitor knob moves.
- If the evidence ends up being UAC (an audio volume change visible on
  IF1/IF2 or mixers), the honest outcome is a fork: the GUI could use
  standard ALSA/PipeWire UX (outside this protocol) and the IF remains
  untouchable. Write that conclusion into docs/protocol.md rather than a
  fake control.

### 2.7 Other §4 commands deliberately NOT on the GUI roadmap

Neither now nor later without a dedicated new task each: SET_DEVICE_REBOOT
(12), RX/TX_UPGRADE + DFU (26–30), SET_UPGRADE_DEVICE (44), REFRESH_RX
(60), SET_RX_INTO_PAIR (61), SET_TX_INTO_OTA (62). Pairing/re-pair and OTA
are device-takeover operations outside this project's write rules; their
failure modes are beyond rollback.

## 3. EP 0x81 push reader (StatusSource upgrade)

Current truth: `holly-app` polls `get_heart_info()` every 1 s behind the
`StatusSource` trait (`holly-app/src/lib.rs`), which exists precisely so a
push reader can slot in. docs/protocol.md open question 2 notes heart replies
arrive on the GET path (`05 03 BB DD …`); whether the interrupt endpoint
`EP 0x81 IN` (64 B interrupt, `inputs/lsusb-v.txt` EP 1 IN + §IF0 in
docs/device.md) pushes unsolicited copies of those frames is UNDECIDED.

Procedure to decide:

1. Claim IF0 and read the interrupt endpoint `EP 0x81 IN` (64 B interrupt,
   `inputs/lsusb-v.txt` EP 1 IN + docs/device.md §IF0) **without** sending
   anything for 60 s (tool-side, e.g. extend the capture harness; do not let
   hid-generic take the EP for this window). Three possible outcomes:
   - Silence → the firmware only replies to GETs; push reading = poll-free
     nothing, keep polling. Record in docs/protocol.md.
   - Periodic frames → capture + diff against GET replies (BB header?
     cmd 31? residue byte?). If identical shape, a push reader replaces
     the 1 s GET: fewer control transfers, identical parser.
   - State-change frames only (e.g. TX plug/unplug, battery steps) → best
     of both; document the shape, extend §5's incremental-length rule if
     the lengths differ.
2. Implementation shape (after evidence): a
   `holly-usb::PushSource` implementing `StatusSource`, driven by
   `nusb`'s interrupt reads on the same IF0 claim, with the GET poll kept
   as a fallback timer (start-of-session GET + N-second watchdog). The
   actor needs no change — that was the purpose of the trait.
3. UI needs nothing at all.

## 4. Windows / macOS

- Transport: holly-usb already builds on both at source level (`nusb`'s
  core-usb API is cross-platform; the macOS-only `core-foundation` pin
  issue is solved by the vendored patch, see below). Claim semantics
  differ: Windows wants explicit HID report IDs from the interface; macOS
  needs `entitlement`-less HID open (usually fine, but `IOHIDDevice` gets
  involved behind nusb).
- UI: gpui 0.2.2 supports Windows (with `windows-manifest` default feature)
  and macOS. Packaging out of scope here (no installer planned).
- udev does not exist there: Windows requires the WinUSB/HID driver policy
  (usually already open for HID class devices under the same user), macOS
  grants by default. Document, don't guess: test with the Task-1 hardware
  test before the driver code is trusted.
- Patch removal trigger: when EITHER nusb or gpui publishes a release that
  resolves `core-foundation` consistently, our vendored copies
  (`frontends/gpui/patches/nusb`, `patches/xattr`, provenance sha256 in the
  sibling `*.crate-sha256` files) are deleted in the same commit — they are
  a temporary unlock, not a fork.

## 5. Explicitly OUT of scope for v1 (and why)

- Tray icon / status applet — needs the daemon-vs-window ownership question
  solved first (who owns the claim while the window is closed?). Today the
  claim lives with the window's process; a tray would detach it and need a
  policy for "GUI absent, keep polling" — a separate design task.
- Autostart — same actor-ownership question plus user-session integration.
- Multiple windows, theming, i18n — pure UI, no evidence needed, but no
  value yet for a one-panel tool.
- A settings file — nothing to configure that isn't discoverable live.
- Sending ANY command outside the verified set — including so-called
  "sync-all" convenience layers.

## 6. Verification checklist for the GUI itself (hardware, user-run)

```sh
# 0. one-time: udev grant (from Task 6; skip if getfacl already shows the ACL)
sudo rm -f /etc/udev/rules.d/99-hollyland.rules
sudo cp packaging/70-hollyland.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --action=add /sys/bus/usb/devices/6-1   # node may shift after replug
getfacl /dev/bus/usb/006/002      # expect user:<you>:rw- + mask

cd frontends/gpui
cargo run --release
```

In-round: status panel matches `holly status` output line for line (same
HeartInfo source); NC toggle click (and `space`) flips the DEVICE state
with the pending → verified transition visible within ~2 s; banners:
launch the GUI and run `holly status` in a terminal while the GUI holds
IF0 — the CLI must report the claim cleanly (Busy classification) and the
GUI must keep showing the device truth, whatever the arbiter allows;
unplug with the window open → GUI banner NotFound, replug → auto-
reconnect to Connected;
During: OBS/Audacity recording for a few minutes with the GUI open —
watch for audio xruns; the GUI touches only IF0 control transfers and
consumes ~1 frame/s, nothing in the audio path is claimed.
