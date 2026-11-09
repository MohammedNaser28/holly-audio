# SET-side gate checklist — why non-NC writes are blocked, per control

Date: 2026-11-06. Status: EVIDENCE GATHERING ONLY.
No write has ever been sent from our side except the two wire-certified NC
operations (SET_NOISE switch + level, `captures/2026-11-05-set-noise.txt`;
guarded level round-trip, `captures/2026-11-05-noise-level-medium.txt`; and
GUI-actor NC touches the same day). This file is the §1 **step-1 cross-check**
for every remaining control: what the APK writes, in what byte space, with
what ACK rule — and what still stands between that and a holly-usb operation.

## 1. Why only NC writes today — three separate layers

1. **Transport is NOT the blocker.** PC-side SETs demonstrably work:
   SET_REPORT (Feature, id 5) accepts a 64-byte frame, the device answers a
   1-byte status ACK, the heart reflects the change, and the GET carve-out
   agrees (`captures/2026-11-05-set-noise.txt` E2/E4 [wire]; GUI-actor
   `Switch(false)`/`Switch(true)`/`Level(1)`/`Level(0)` all `Ok`, 2026-11-06).
2. **The gate IS the blocker — and step 2 is physically unreachable from the
   PC.** The app drives the dongle only while docked in the phone
   (user-verified over sessions 3→4; every phone flip required an unplug
   trip). Phone-app SET frames therefore travel the docked segment, which we
   cannot probe; our GET-only poller sees only the heart delta after replug —
   the VALUE space, never the frame. This is exactly why every read map
   (§5.2) closed while every SET row stayed open.
3. **The crate freeze backs the gate.** holly-usb exposes only
   `get_heart_info` / `set_noise` / `set_noise_level`; any new operation
   waits on an explicit decision (see §6) plus an unfreeze.

## 2. The shared frame template — already [wire]-verified twice

Every Pro* class implements `sendData() = getData()` (e.g.
`ProSetVoiceMode.java:20-22`), i.e. §2's template
`05 03 AA DD | cmd | len_be | payload | EF` padded to 64 bytes, sent as
report 5 via one control transfer. The NC sessions proved the template
byte-exact — including the `[{a},{b}]` two-payload shape — so per-command
risk reduces to the `(cmd byte, payload bytes)` pair alone.

## 3. Per-control cross-check — §1 step 1 CLOSED for every row below

Commands from each class's `getCmd()`; payloads from the
`UsbMessengerV2.saveReserved` argument arrays (`UsbMessengerV2.java:105-232`);
ACK interpretation = 1-byte status, `toInt(data[0]) == 0` → success — uniform
across all 11 `ProSet*` classes (`receiveData` grep matrix, 2026-11-06),
identical to the wire-verified SET_NOISE ack.

Write-value spaces come from the APK's UI callers; read spaces are §5.2's
[code+wire] maps. The two AGREE for every control — no inverted-write
mismatch exists (light and speaker are inverted on BOTH sides).

| control | SET cmd | APK GET | payload (exact order) | write semantics (caller cite) | value space [wire] | gate state |
|---|---|---|---|---|---|---|
| Channel mode | 1 | 2 | `[{value}]` | identity: mono→0, stereo→1 (`HLDevice6302Activity.java:1652/:1662`) | 0,1 [wire] | frame proven family; capture pending |
| Gain | 3 | 4 | `[{value}]` | identity: seekbar select index (`HLDevice6302Activity.java:840-846`) — byte N = stop N+1, matching session-3b heart=3↔"4" | 0..5 [wire] | " |
| Reverb | 5 | 6 | `[{isOpen},{value}]` — **switch byte FIRST** | gear identity GEAR_ONE/TWO/THREE (0/1/2) with `isOpen=true` (`HLDevice6302Activity.java:759/:769/:779/:793`) | gear 0,1,2 [wire all three] | " (arg order resolved, §4) |
| EQ | 7 | 8 | `[{value}]` | identity: `setEq(EQ.OVERCAST/BRIGHT/BALANCE.getType())` = 1/2/3 (`HLDevice6302Activity.java:729/:739/:749`) | 1,2,3 [wire all] | " |
| Shutdown power off | 39 | — | `[{value}]` | identity: quarter("15min")→0, never→1 (`HLDevice6302Activity.java:1668/:1673`) | 0,1 [wire all] | " |
| Indicator light | 33 | — | `[{value}]` | INVERTED both sides: toggle ON→`setLight(0)`, OFF→1 (`HLDevice6302Activity.java:662`); heart read 0=on (`ProGetHearInfo.java:75-76`) | 0=on [wire] | " |
| Identify beep | 35 | — | `[{value}]` | identity: ON→1 (`FAIL_CLOSE=1`), OFF→0 (`SUC_OPEN=0`) (`HLDevice6302Activity.java:819-824`, `DeviceConnectVariable.java:652-654`); heart `== 1` (`ProGetHearInfo.java:88`) | 0 seen [wire]; 1 code-certified | " |
| Speaker (no GUI panel) | 37 | — | `[{value}]` | INVERTED both sides: switch ON→`setSpeaker(0)` (`HLDevice6302Activity.java:652`); heart carries no speaker field | never observed on wire | no panel → display requires a decode first (policy: no invented slot); writes NOT planned |
| TX mute | 22 | — | `[txIndex 0..3, muted]` (`:209`) | — | — | EXTRA-BLOCKED: `deviceType.ordinal()` mapping (protocol.md open question 3) unsettled; muting affects live audio. Excluded until Q3 closes. |

Excluded by policy (device-disrupting or OTA, never UI writes):
SET_DEVICE_REBOOT 12; OTA family 26-30 / 44-45 / 60-62; performance mode
69/70; SN/MAC/version reads. The legacy `Pro_get_speaker` V1 handler
(`UsbDeviceControlViewModel.onResGetSpeaker`) is old-API surface, not this
device's V2 flow.

Footnote — GET companions 34/36/38/40 have **no producer class anywhere in
this APK** (no `return (byte) 34|36|38|40` under `protocolV2/`): the app
reads light/identify/speaker/shutdown through GET_HEART_INFO only. Our
post-SET verification must therefore ride the heart poll (§5 offsets) — the
same thing `set_noise` already does; GET_NOISE cmd 20 is the only real GET
the certified NC path uses.

## 4. The reverb two-byte order question (§6 note) — resolved at [code] level

`setReverb(int value, boolean isOpen)` builds `[isOpen, value]`
(`UsbMessengerV2.java:192-195`) — the switch byte comes FIRST, the identical
builder pattern to `setNoiseLevel(int level, boolean isOpen)` →
`[isOpen, level]` (`:171-175`), whose order is wire-certified (E2/E4).
Nothing left to guess: both two-byte commands lead with the switch.

## 5. Honest residuals, per row

- **cmd numbers** come from decompiled `getCmd()` constants; several were
  constant-folded through unrelated tables (`PostProtocol.Command.GET_MUTE`
  → 39, `Ascii.SYN` → 22, `Utf8.REPLACEMENT_BYTE` → 63,
  `SignedBytes.MAX_POWER_OF_TWO` → 64, `Ascii.DC4` → 20, `Ascii.US` → 31).
  Each resolved to its definition in-source; all agree with §4's table.
- **ACK failure shape** never observed: every NC ack so far was `00` [wire].
  A nonzero status would surface as a pending-set error in the app layer —
  bounded, no restore needed (device either applied it or not).
- **Speaker value space** has zero [wire] observations. It is capture-only.
- **Identify beep value 1** is code-certified (enum), never seen on our wire
  (device was always off). A phone flip would close it.

## 6. What unblocking would look like — DECISION REQUIRED (owner's, not ours)

Owner's correction (2026-11-06): §1 step 2 is satisfiable — the repo carries
the QEMU android rig (`tools/qemu-android.sh`, Android-x86 9.0 ISO,
HollyAudio 3.1.0 APK) for exactly this, so the **primary** path is passive
app-side capture from the guest, and `docs/set-capture-plan.md` §3 defines
the first four reversible captures (EQ 7 first, then gain 3, light 33,
reverb 5).

Current state of that path (verified 2026-11-06, evidence in
docs/capture.md §Rig status): the VM boots with passthrough and adb, but the
installed image lacks the libhoudini ARM translation runtime, so the
armeabi-v7a-only app crashes in `BaseApplication.onCreate`
(`UnsatisfiedLinkError: ... e_machine: 40 EM_ARM`) before any UI, and no
app-side SET capture can happen until a houdini-carrying image is installed
or the blobs are retrofitted. The capture plan §3 stays executable the
moment the guest can drive the app.

If the owner elects to keep the rig blocked and accept substitute evidence:

- **A. Keep the gate as written.** PC stays read-only plus NC forever.
  Zero risk; zero progress.
- **B. Evidence-relaxation for guarded round-trips (fallback only).** §1's
  intent — "never write a frame you haven't seen work" — has effectively
  been met at template level twice ([wire]), the shape loss risk is the
  `(cmd, payload)` pair from ONE builder with one ack convention, and every
  value space is fully [wire]-mapped. A per-command guarded test shaped like
  `holly-usb :: set_noise_level_roundtrip_guarded` (fresh heart → write →
  ack + fresh heart verify → restore → restore verify) bounds any failure to
  one setting value and self-heals. Exclusions per §3 (reboot/mute/OTA).
- **C. Physical sniffer rig** (OTG pass-through capture between phone and
  dongle) — superseded by the VM rig as the step-2 tool (owner's correction).

Any implementation under B waits on: (1) the owner's accept of this file,
(2) an explicit holly-usb unfreeze, (3) a roadmap §1 amendment naming the
guarded round-trip as substitute evidence (documented per command in
`captures/` as runs happen).
