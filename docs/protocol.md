# Hollyland 6108 dongle — USB control protocol (analysis v0.1)

Device: `3547:0407` = Hollyland internal model **6108** (`PRODUCT_6108 = 1031`,
`re/apk/sources/com/hollyland/application/common/protocol/Product.java:5`;
1031 = 0x0407 = device PID, `inputs/lsusb-v.txt`).

The decompiled app names this the "USB mic"/HID path (`com.hollyland.usbmic.*`).
Three protocol families exist in the APK (6118/6201/6303 providers,
`ProtocolManagerInitializer.java:22-27`); NONE of them claims PID 1031
(6118 → 1041/1042, `Protocol6118Provider.java:110`; 6201 → 1032-1034,1040;
6303 → 1036-1039), so 6108 traffic rides the older
`USBHIDService` + `ProtocolFactoryV2` pipeline exclusively.

Status legend for every entry below until captures verify it:
- confidence **[code]** = traced in decompiled source, plausible, not yet seen on the wire
- confidence **[wire]** = confirmed against a capture in `captures/` (none yet)
- argued-from descriptor = corroborated by `inputs/lsusb-v.txt` / `inputs/hid-report-descriptor.hex`

## 1. Transport (which interface, which transfers)

| Fact | Value | Source |
|---|---|---|
| Control interface | IF0, class 3 (HID) — app picks interface with `getInterfaceClass() == 3` | `HlUsbManagerImpl.findSuitableInterface` (`re/apk/sources/com/hollyland/usb/core/HlUsbManagerImpl.java:802-829`) [code] |
| Command write | `controlTransfer(0x21, 9, wValue, 0, buf, 64, 3000 ms)` = SET_REPORT(Feature) | requestType 33 hardcoded `HlUsbManagerImpl.java:1408`; bRequest/defaults from Kotlin default-args flag in `UsbCommandInfo` ctor: write=9, read=1 (`UsbCommandInfo.java:168`) [code] |
| Response read | `controlTransfer(0xA1, 1, wValue, 0, buf, 64, 1000 ms)` = GET_REPORT(Feature), done when command has `needResponse=true` | `HlUsbManagerImpl.java:1431` (`Opcodes.IF_ICMPLT` is a jadx constant reuse of raw 0xA1) [code]; **verified live: host-initiated GET_REPORT on this leg returned the response frame (`captures/2026-11-05-hearinfo-first-wire.txt`)** [wire] |
| wValue | `reportId` = **773 (0x0305)** normal, **774 (0x0306)** during firmware transfer only | `UsbHandlerManager.java:11-19` (`REPORT_ID_UPGRADE=774`, `REPORT_ID_NORMAL=773`); `updateTransportConfig()` `UsbHandlerManager.java:73-77` [code] |
| Interpretation of wValue | `0x0305` = reportType 3 (Feature) << 8 \| reportId 5 — matches HID descriptor's 134-byte report descriptor: 63-byte Feature report ID 5, 159-byte Feature report ID 6, 16-bit consumer Input report ID 3 | `inputs/hid-report-descriptor.hex` [descriptor] |
| Control buffer size | 64 normal (= 1 report-ID byte + 63 feature bytes), 160 only for firmware transfer | `USB_PKT_SIZE_6108_NORMAL=64`, `USB_PKT_SIZE_6108_UPGRADE=160` (`UsbHandlerManager.java:12-13`) [code+descriptor] |
| Transfer type for 1031 | CONTROL (not bulk) — explicitly special-cased | `USBHIDService.java:404` [code] |
| ASYNC device→host | interrupt reads on EP 0x81 (the HID interface's IN endpoint, claimed by same `findSuitableInterface`) | `USBHIDService` receive loop / `HlUsbManagerImpl.startDataReceiveLoop` [code] |
| EP 0x06 interrupt OUT | captured by app interface selection but 6108 commands do NOT use it (writes go via SET_REPORT); descriptor nonetheless defines 63-byte Input+Output reports ID 8 (vendor page 0xFF53) for the interrupt pipes — unused by the `usbmic` control path in decompiled code | `inputs/hid-report-descriptor.hex` parsed by `tools/parse_hid_report.py` [descriptor] |
| Audio IFs (1/2) | untouched by the control code; dongle is a standard UAC mic (48 kHz, 2ch, 24/16-bit) | `inputs/lsusb-v.txt` [descriptor] |

Set-flag nuance: the raw constructor call at `USBHIDService.java:406` shows
`(reportId, 0, 0, false, null, …)` but the trailing default-marker arg (`488`)
selects the dataclass defaults instead — read 1, write 9, needResponse **true**
(`UsbCommandInfo.java:167-168`)., i.e. every 6108 command expects a GET_REPORT
response (except queue-mega-commands, which set per-command overrides).

Firmware-transfer mode (160-byte buffer, report 6) is OUT OF SCOPE per project rules.

## 2. Frame format (inside a 63-byte feature payload)

Producer: `ProtocolV2.getData()` (`re/apk/sources/com/hollyland/application/common/protocol/ProtocolV2.java:74-103`) [code]

```
offset  0: 0x05   (also equals report id 5; header magic)
offset  1: 0x03
offset  2: 0xAA
offset  3: 0xDD
offset  4: cmd            (u8; command, from the table below)
offset  5..6: payload_len (2 bytes, big-endian via NumberUtil.intToByte2;
                           emitted only when payload non-empty)
offset  7..: payload
last:      0xEF           (frame terminator; position = 7 + payload_len)
…report tail 0x00 to fill 64-byte buffer
```

- **No checksum on the wire** for normal commands — the frame ends in `0xEF`
  (`ProtocolV2.getData()` appends nothing else). There is an XOR checksum helper
  (`ProtocolV2.crcCalculate`, `ProtocolV2.java:179-186`) but it is not applied in
  `getData()`. [code]
- Receiver does **not** validate `03 AA DD` — parsing key is the command byte at
  offset 4 (`USBDataReceiveEvent.updateUSBData`, `usbmic/events/USBDataReceiveEvent.java:49-57`;
  `ProtocolV2.parse` swallows data[4] as cmd) [code].

RX framing (device → host), measured: `captures/2026-11-05-hearinfo-first-wire.txt` [wire]:

| Fact | Value | Source |
|---|---|---|
| Reply preamble | `05 03 BB DD` — **`BB` marks device→host**, `AA` is host→device only | same capture [wire]; app's `getData()` hardcodes AA (`ProtocolV2.java:74-82`) [code] |
| cmd echo | reply repeats the request's cmd byte at offset 4 (`1F` → `1F`) | same capture [wire] |
| Reply length field | 2 bytes BE at 5..6 (`00 11` = 17), payload `data[7..7+len)` | same capture + `ProtocolV2.parse` default branch (`ProtocolV2.java:154-159`) [code+wire] |
| Reply terminator | **no `0xEF` in any reply** — the byte at `7+len` varies (`0x09` first capture; `0x37/0x36/0x37` heart replies and `0x75` ACKs in `captures/2026-11-05-set-noise.txt`). `parse()` never checks magic or terminator, so no contract requires EF in replies | both captures [wire] + `ProtocolV2.parse` has no terminator check [code] |
| SET-command ACK | same `05 03 BB DD` shape, cmd echo, 1-byte payload `00` = success; **status only — the TX payload is not echoed back**. No error-shape reply observed so far (all SETs succeeded); non-zero ACK as error is [code]-only (`ProSetNoise.receiveData` reads `data[0]`) until captured | `captures/2026-11-05-set-noise.txt` E2/E4 [wire] |
| State-change congruence | SET_NOISE (`[0,0]` → off, `[1,0]` → on) flipped `noiseSwitch` (§5 payload offset 6) `01→00→01`; every other payload byte stayed constant; final state == start state | same capture E1/E3/E5 [wire] |

The `hi * 16 + lo` length-decode quirk (`ProtocolV2.parse`, `ProtocolV2.java:157`)
is **observationally dead**: the report-5 payload budget is 63 bytes, so the BE
high byte is always 0 and `0*16 + lo == hi*256 + lo` for every frame that can
physically exist. Same for the 160-byte OTA frames (payload ≤ 152 < 256). app and the device behave identically on all reachable frames;
`holly-proto` encodes/decodes plain BE. [code+wire analysis]

RX-side quirks to verify on the wire (findable bugs in the app, not our choices):
- `ProtocolV2.parse` computes payload length as `data[5]*16 + data[6]` — base-16
  arithmetic decompiled at `ProtocolV2.java:157`; if the device sends true
  big-endian high/low bytes (like `getData()` writes), any frame with
  payload > 15 bytes would be misparsed by this formula. Needs [wire] evidence
  before `holly-proto` copies either behavior.
- OTA commands (26/27/28) read a 1-byte payload at offset 5; DFU data/verify
  (29/30) read at offset 9 (`ProtocolV2.parse` `ProtocolV2.java:136-149`). [code]

## 3. Receive-path length rules

From `USBDataReceiveEvent.java:43-53` [code]:
- PID 1031: frames ≤ 4 bytes rejected (min 5: data[4] must exist)
- other PIDs: ≤ 2 bytes rejected; cmd byte = data[2] for them (not our case)

## 4. Command table (ProtocolFactoryV2)

Producer: `re/apk/sources/com/hollyland/usbmic/protocolV2/ProtocolFactoryV2.java`
dispatch, constants repeated at `application/common/protocol/ProtocolV2.java:18-56`
[all code-confidence until captures]. Request replies reuse the same header and
give `data[0] == 0` for ACK-success on set commands (`ProSetVoiceLevel.receiveData`
pattern etc.).

### Control / config

| cmd | name | request payload | response payload |
|----|------|-----------------|------------------|
| 1 | SET_VOICE_MODE | `[{value}]` | ack (data[0]==0 ⇒ success) |
| 2 | GET_VOICE_MODE | – | `[{value}]` |
| 3 | SET_VOICE_LEVEL | `[{value}]` (gain) | ack |
| 4 | GET_VOICE_LEVEL | – | `[{value}]` |
| 12 | SET_DEVICE_REBOOT | – | ack |
| 18 | GET_DEVICE_SN | `[{deviceType.ordinal()}]` | sn bytes |
| 19 | SET_NOISE | `[{isOpen 0/1}, {level}]` [wire: TX verified, `captures/2026-11-05-set-noise.txt` E2/E4] | 1-byte status ack: `00` = success; device neither echoes `isOpen` nor `level` — status only (`ProSetNoise.receiveData` `data[0]==0` → success) [wire E2/E4 + code] |
| 20 | GET_NOISE | – | `[{isOpen}, {level}]` |
| 22 | SET_TX_MUTE | `[{txIndex 0..3}, {muted 0/1}]` | ack |
| 13 | GET_DEVICE_VERSION | `[{deviceType.ordinal()}]` | version |
| 26 | RX_UPGRADE | n/a (OTA) | ack byte |
| 27 | TX_UPGRADE | n/a (OTA) | ack |
| 28..30 | OTA DFU enter/data/verify | n/a (report ID 6, 160-byte mode) | ack special offsets |
| 31 | GET_HEART_INFO (status poll) | – | see §5 |
| 33 | SET_LIGHT | `[{value}]` | ack |
| 34 | GET_LIGHT | – | `[{value}]` (0 = on) |
| 48 | GET_DEVICE_MAC_RX | – | mac |
| 49 | GET_DEVICE_MAC_TX | `[{deviceType.ordinal() - 1}]` | mac |
| 5/6 | SET/GET_REVERB | `[{isOpen}, {value}]` / – | ack / `[{isOpen}, {level}]` |
| 7/8 | SET/GET_EQ | `[{value}]` | ack / `[{value}]` |
| 35 | SET_TX_IDENTIFY | `[{value}]` | ack |
| 36 | GET_TX_IDENTIFY | – | `[{value}]` |
| 37/38 | SET/GET_SPEAKER | `[{value}]` | ack / `[{value}]` |
| 39/40 | SET/GET_SHUTDOWN_TIME | `[{value}]` / – | ack / `[{value}]` |
| 44/45 | SET_UPGRADE_DEVICE / GET_UPGRADE_RESULT | raw | raw |
| 60 | REFRESH_RX | – | ack |
| 61 | SET_RX_INTO_PAIR | – | ack |
| 62 | SET_TX_INTO_OTA | – | ack |
| 63/64/65 | SIGNAL_MODE set/get/`GET_MODE_FAIL_REASON` | `[{value}]` | ack / `[{value}, …]` |
| 69/70 | SET/GET_PERFORMANCE_MODE | `[{isOpen 0/1}]` | ack / `[{value}]` |

Payload layouts from `UsbMessengerV2` calls to `saveReserved(...)`:
`re/apk/sources/com/hollyland/usbmic/messenger/UsbMessengerV2.java:105-261`.

`deviceType` ordinal semantics (TX1/TX2/RX indexes) still needs either the
`DeviceType` enum or [wire] confirmation — TODO, do not guess.

### Examples (computed from the format, pending [wire] check)

```
SET_VOICE_LEVEL value=5:   05 03 AA DD 03 00 01 05 EF 00… (64-byte buffer, tail at 9)
GET_VOICE_LEVEL:           05 03 AA DD 04 00 00 EF 00…    (len bytes stay 0x00 when
                            payload is empty; getData() writes them only for
                            non-empty payloads, so EF lands at offset 7 either way)
```

## 5. Status poll (cmd 31 GET_HEART_INFO) layout

Producer: `re/apk/sources/com/hollyland/usbmic/protocolV2/ProGetHearInfo.java`
(response offsets, incrementally required — length ≥ n gates each field) [code]

| off | field | meaning |
|-----|-------|---------|
| 0 | tx1Status | 1 = transmitter 1 connected |
| 1 | tx2Status | transmitter 2 connected |
| 2 | tx1Battery | int |
| 3 | tx2Battery | int |
| 4 | tx1MuteStatus | 1 = muted |
| 5 | tx2MuteStatus | 1 = muted |
| 6 | noiseSwitchStatus | 1 = NC on |
| 7 | noiseLevel | NC level |
| 8 | voiceLevel | gain |
| 9 | voiceMode | mode |
| 10 | shutdownTime | auto-power-off value |
| 11 | lightStatus | 0 = light on |
| 12 | eqLevel | EQ level |
| 13 | reverbSwitchStatus | 1 = on |
| 14 | reverbLevel | |
| 15 | txIdentifyStatus | identify beep state |
| 16 | signalMode | |
| 17-22 | tx3/tx4 status, battery, mute | for multi-TX models |

Live capture [wire] (`captures/2026-11-05-hearinfo-first-wire.txt`): daemon
answered with the full 17-byte field set at exactly these offsets
(`len=0x11`): TX1 off, TX2 connected, TX2 battery `0x63` = 99, mutes off,
NC on (`01`), voiceLevel/eqLevel `03`, reverbLevel `02`. Values consistent
with the rig's physical state — [wire] upgrade of the whole single-TX field
map. The 17-byte length also proves the resource layout claim: no length
padding, `off` = `payload[0..16]` contiguous bytes.

## 6. Provenance note

- Sources live in `re/apk/` (decompiled Hollyland 3.1.0, jadx). That directory
  must never be committed — it itself contains Hollyland-owned code we must not
  redistribute (see project hard rules). Only this doc and original
  implementations of observed behavior belong in the repo.
- Next step to upgrade confidence: Phase 1 captures (esp. `status-open`,
  `gain-up`, `nc-on/off`, `monitor`-related actions), diffed against §2 frame
  layout, the length quirk in §2, and exact `03` header byte semantics.

## Open questions

1. ~~`*16` decode vs `intToByte2` encode~~ **RESOLVED 2026-11-05 [wire]**: reply
   payload of 17 bytes observed (`00 11` BE); the two formulas coincide for
   every physically representable frame (payload budget 63 or 152 bytes < 256,
   so BE hi-byte is always 0). Quirk is dead code; `holly-proto` uses plain BE.
2. [wire→ partially resolved] Device-originated frame preamble **is** the same
   `05 03` shape but with `BB`: reply = `05 03 BB DD` (measured
   `captures/2026-11-05-hearinfo-first-wire.txt`, GET_REPORT leg). Whether
   unsolicited interrupt-EP 0x81 pushes (not GET-triggered) use the same `BB` header remains open.
   The post-payload residue byte is observed across captures now: `0x09`
   (heart, session 1), `0x37/0x36/0x37` (heart, session 2 — alternating exactly
   with the flipped `noiseSwitch` byte, and equal to `sum(frame[0..23]) mod 256`
   there), `0x75` (both SET ACKs, constant). The sum formula fails for the ACKs
   (predicts `0xB4`) and session 1 (predicts `0x39`), so no rule is settled —
   collect its values in every capture until it can be named precisely (checksum
   candidate / per-frame counter; no holly-proto change until proven).
3. [code→wire] `deviceType.ordinal()` mapping (TX1 vs TX2 vs RX) for SN/MAC/mute commands.
4. [code→wire] Value ranges for voiceLevel (gain), noiseLevel, eqLevel, speaker, light.
5. [code→wire] Error ACK shape for SET commands: never captured yet (all SETs
   succeeded). `ProSetNoise.receiveData` implies `data[0] != 0` = setStatus
   false, but whether the device ever replies after a rejected SET, with what
   payload/length, is unobserved. Do not synthesize an error frame assumption.
