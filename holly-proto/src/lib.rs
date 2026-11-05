//! Hollyland 6108 USB control protocol — pure framing, no I/O.
//!
//! Every constant here must trace back to evidence. The chain is:
//!   inputs/lsusb-v.txt          live `lsusb -v` of the dongle (3547:0407)
//!   inputs/hid-report-descriptor.hex  HID report descriptor (sysfs copy)
//!   docs/protocol.md            full analysis table   (decompiled-app traces)
//!   re/apk/...                  decompiled HollyAudio 3.1.0 (never committed)
//!
//! Confidence flags used in comments:
//!   [code]       traced in decompiled source / descriptor, not yet seen on wire
//!   [wire]       verified against a capture in captures/ (first one:
//!                captures/2026-11-05-hearinfo-first-wire.txt, live dongle)
//! ⚠ Nothing in this file may be invented. If a value has no source, it must not
//! exist here. Where the app has observable quirks (see docs/protocol.md §2),
//! they are reproduced *only* if the device demonstrably requires them.

/// Frame preamble written by the app's sender (`ProtocolV2.getData()`,
/// `application/common/protocol/ProtocolV2.java:76-87`), host→device.
///
/// Bytes (1,2,3) = `03 AA DD`: magic. Byte 0 (`05`) also happens to equal HID
/// feature report 5 (see `REPORT_ID_WVALUE_NORMAL`), but was written into the
/// data buffer by the app regardless — exact semantics are [code], capture
/// pending (docs/protocol.md §2 open question 2).
pub const HEADER: [u8; 4] = [0x05, 0x03, 0xAA, 0xDD];

/// Device→host reply preamble, **measured on the wire**:
/// the dongle answers GET_REPORT (Feature id 5) with `05 03 BB DD` — the same
/// frame as host→device except `BB` replaces the sender's `AA` and no `EF`
/// terminator is emitted.
/// Source: `captures/2026-11-05-hearinfo-first-wire.txt` [wire].
pub const REPLY_HEADER: [u8; 4] = [0x05, 0x03, 0xBB, 0xDD];

/// Frame terminator appended by `ProtocolV2.getData()`
/// (`ProtocolV2.java:99-101`). [code]
pub const TAIL: u8 = 0xEF;

/// wValue for SET_REPORT/GET_REPORT on a normal (non-firmware-OTA) frame.
/// `UsbHandlerManager.REPORT_ID_NORMAL = 773 = 0x0305` =
/// reportType 3 (Feature) << 8 | reportId 5; bRequest values 9 / 1 come from the
/// `UsbCommandInfo` dataclass defaults (`UsbCommandInfo.java:168`). [code]
/// Corroborated by the 63-byte Feature report #5 in
/// `inputs/hid-report-descriptor.hex` (64-byte transfer buffer = 1 report-ID
/// byte + 63 payload bytes).
pub const REPORT_ID_WVALUE_NORMAL: u16 = 0x0305;

/// Firmware-OTA-only wValue (`REPORT_ID_UPGRADE = 774 = 0x0306`; 160-byte
/// buffer). **Out of scope** — this crate intentionally provides no OTA
/// encoding; flashing can brick the device (project rule).
pub const REPORT_ID_WVALUE_UPGRADE: u16 = 0x0306;

/// Normal control-transfer buffer size (`USB_PKT_SIZE_6108_NORMAL = 64`,
/// `UsbHandlerManager.java:12`). [code+descriptor]
pub const CONTROL_BUF_SIZE: usize = 64;

/// Maximum payload length that fits one normal feature report frame:
/// header(4) + cmd(1) + len(2) + payload + tail(1)1 ≤ 64 ⇒ payload ≤ 56 bytes.
/// Derived from `CONTROL_BUF_SIZE`; OTA uses a different buffer and is out of
/// scope. [derived]
pub const MAX_PAYLOAD_LEN: usize = CONTROL_BUF_SIZE - 8;

/// Command opcodes, complete map from `ProtocolFactoryV2` dispatch
/// (`usbmic/protocolV2/ProtocolFactoryV2.java`) and the same numbers declared at
/// `application/common/protocol/ProtocolV2.java:18-56`. [code]
pub mod cmd {
    pub const SET_VOICE_MODE: u8 = 1; // ProSetVoiceMode
    pub const GET_VOICE_MODE: u8 = 2; // ProGetVoiceMode
    pub const SET_VOICE_LEVEL: u8 = 3; // ProSetVoiceLevel (gain)
    pub const GET_VOICE_LEVEL: u8 = 4; // ProGetVoiceLevel
    pub const SET_REVERB: u8 = 5; // ProSetReverb
    pub const GET_REVERB: u8 = 6; // ProGetReverb
    pub const SET_EQ: u8 = 7; // ProSetEQ
    pub const GET_EQ: u8 = 8; // ProGetEQ
    pub const SET_DEVICE_REBOOT: u8 = 12; // ProSetDeviceReboot
    pub const SET_TX_MUTE: u8 = 22; // ProSetTxMute ([txIndex, muted])
    pub const GET_DEVICE_VERSION: u8 = 13; // ProGetDeviceVersion
    pub const GET_DEVICE_SN: u8 = 18; // ProGetDeviceSn
    pub const SET_NOISE: u8 = 19; // ProSetNoise ([isOpen, level])
    pub const GET_NOISE: u8 = 20; // ProGetNoise ([isOpen, level])
    pub const OTA_RX_UPGRADE: u8 = 26; // out of scope
    pub const OTA_TX_UPGRADE: u8 = 27; // out of scope
    pub const OTA_DFU_ENTER: u8 = 28; // out of scope
    pub const OTA_DFU_DATA: u8 = 29; // out of scope
    pub const OTA_DFU_VERIFY: u8 = 30; // out of scope
    pub const GET_HEART_INFO: u8 = 31; // ProGetHearInfo — status poll
    pub const SET_LIGHT: u8 = 33; // ProSetLight
    pub const GET_LIGHT: u8 = 34; // not in factory map; constant at ProtocolV2.java:24
    pub const SET_TX_IDENTIFY: u8 = 35; // ProSetTxIdentify
    pub const GET_TX_IDENTIFY: u8 = 36; // constant at ProtocolV2.java:32
    pub const SET_SPEAKER: u8 = 37; // not in factory map; constant at ProtocolV2.java:51
    pub const GET_SPEAKER: u8 = 38; // constant at ProtocolV2.java:31
    pub const SET_SHUTDOWN_TIME: u8 = 39; // ProSetShutdownTime
    pub const GET_SHUTDOWN_TIME: u8 = 40; // constant at ProtocolV2.java:29
    pub const SET_UPGRADE_DEVICE: u8 = 44; // out of scope
    pub const GET_UPGRADE_RESULT: u8 = 45; // out of scope
    pub const GET_DEVICE_MAC_RX: u8 = 48; // ProGetDeviceMacRX
    pub const GET_DEVICE_MAC_TX: u8 = 49; // ProGetDeviceMacTX
    pub const REFRESH_RX: u8 = 60; // ProRefreshRx
    pub const SET_RX_INTO_PAIR: u8 = 61; // ProSetRxIntoPair
    pub const SET_TX_INTO_OTA: u8 = 62; // out of scope
    pub const SET_SIGNAL_MODE: u8 = 63; // ProSetSignalMode
    pub const GET_SIGNAL_MODE: u8 = 64; // ProGetSignalMode
    pub const GET_MODE_FAIL_REASON: u8 = 65; // ProGetModeFailReason
    pub const SET_PERFORMANCE_MODE: u8 = 69; // ProSetPerformanceMode
    pub const GET_PERFORMANCE_MODE: u8 = 70; // ProGetPerformanceMode (cmd 70)

    /// Human-readable names for CLI/logs. Only names that appear in the APK
    /// (`Pro*` class names) — never invented ones. [code]
    pub const fn name(command: u8) -> Option<&'static str> {
        Some(match command {
            SET_VOICE_MODE => "SET_VOICE_MODE",
            GET_VOICE_MODE => "GET_VOICE_MODE",
            SET_VOICE_LEVEL => "SET_VOICE_LEVEL",
            GET_VOICE_LEVEL => "GET_VOICE_LEVEL",
            SET_REVERB => "SET_REVERB",
            GET_REVERB => "GET_REVERB",
            SET_EQ => "SET_EQ",
            GET_EQ => "GET_EQ",
            SET_DEVICE_REBOOT => "SET_DEVICE_REBOOT",
            GET_DEVICE_VERSION => "GET_DEVICE_VERSION",
            SET_TX_MUTE => "SET_TX_MUTE",
            GET_DEVICE_SN => "GET_DEVICE_SN",
            SET_NOISE => "SET_NOISE",
            GET_NOISE => "GET_NOISE",
            OTA_RX_UPGRADE => "OTA_RX_UPGRADE",
            OTA_TX_UPGRADE => "OTA_TX_UPGRADE",
            OTA_DFU_ENTER => "OTA_DFU_ENTER",
            OTA_DFU_DATA => "OTA_DFU_DATA",
            OTA_DFU_VERIFY => "OTA_DFU_VERIFY",
            GET_HEART_INFO => "GET_HEART_INFO",
            SET_LIGHT => "SET_LIGHT",
            GET_LIGHT => "GET_LIGHT",
            SET_TX_IDENTIFY => "SET_TX_IDENTIFY",
            GET_TX_IDENTIFY => "GET_TX_IDENTIFY",
            SET_SPEAKER => "SET_SPEAKER",
            GET_SPEAKER => "GET_SPEAKER",
            SET_SHUTDOWN_TIME => "SET_SHUTDOWN_TIME",
            GET_SHUTDOWN_TIME => "GET_SHUTDOWN_TIME",
            SET_UPGRADE_DEVICE => "SET_UPGRADE_DEVICE",
            GET_UPGRADE_RESULT => "GET_UPGRADE_RESULT",
            GET_DEVICE_MAC_RX => "GET_DEVICE_MAC_RX",
            GET_DEVICE_MAC_TX => "GET_DEVICE_MAC_TX",
            REFRESH_RX => "REFRESH_RX",
            SET_RX_INTO_PAIR => "SET_RX_INTO_PAIR",
            SET_TX_INTO_OTA => "SET_TX_INTO_OTA",
            SET_SIGNAL_MODE => "SET_SIGNAL_MODE",
            GET_SIGNAL_MODE => "GET_SIGNAL_MODE",
            GET_MODE_FAIL_REASON => "GET_MODE_FAIL_REASON",
            SET_PERFORMANCE_MODE => "SET_PERFORMANCE_MODE",
            GET_PERFORMANCE_MODE => "GET_PERFORMANCE_MODE",
            _ => return None,
        })
    }
}

/// Frame preamble, host→device: `05 03 AA DD <cmd> <len_hi> <len_lo> <payload..> EF`
/// Device replies use `REPLY_HEADER` (`05 03 BB DD`) and omit `EF` — see decode().
/// Layout per `ProtocolV2.getData()` (ProtocolV2.java:74-103) → docs/protocol.md §2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub command: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(command: u8, payload: Vec<u8>) -> Self {
        Frame { command, payload }
    }

    /// Encoded bytes exactly as `ProtocolV2.getData()` writes them, WITHOUT the
    /// fixed-64 zero padding. Payload length must fit one feature report.
    /// Length is big-endian (`NumberUtil.intToByte2` → `[i>>8, i&255]`,
    /// `application/common/util/NumberUtil.java`), and is ONLY written when the
    /// payload is non-empty; with an empty payload offsets 5-6 stay 0x00 and EF
    /// lands at offset 7. [code]
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        if self.payload.len() > MAX_PAYLOAD_LEN {
            return Err(EncodeError::PayloadTooLong {
                len: self.payload.len(),
                max: MAX_PAYLOAD_LEN,
            });
        }
        let mut out = Vec::with_capacity(8 + self.payload.len());
        out.extend_from_slice(&HEADER); // 05 03 AA DD
        out.push(self.command); // offset 4
        if !self.payload.is_empty() {
            // big-endian, matching NumberUtil.intToByte2 [code]
            out.push((self.payload.len() >> 8) as u8);
            out.push((self.payload.len() & 0xFF) as u8);
            out.extend_from_slice(&self.payload);
        } else {
            out.extend_from_slice(&[0x00, 0x00]); // offsets 5-6 as the app leaves them
        }
        out.push(TAIL);
        Ok(out)
    }

    /// 64-byte transmission buffer: encode() + zero padding to `CONTROL_BUF_SIZE`,
    /// exactly the shape handed to `controlTransfer(…, 64, …)`. [code]
    pub fn encode_buf(&self) -> Result<[u8; CONTROL_BUF_SIZE], EncodeError> {
        let mut buf = [0u8; CONTROL_BUF_SIZE];
        let enc = self.encode()?;
        buf[..enc.len()].copy_from_slice(&enc);
        Ok(buf)
    }

    /// Decodes one frame from a capture/transfer buffer. Direction-aware per
    /// measured wire behavior:
    ///   - host→device frames (`05 03 AA DD`) must end in the `0xEF` tail —
    ///     exactly what `ProtocolV2.getData()` emits.
    ///   - device→host frames (`05 03 BB DD`) have **no terminator**: the app's
    ///     `parse()` only reads `data[4..7+len)` and never checks magic or tail,
    ///     and the live reply ended in `0x09` at the would-be tail slot
    ///     (`captures/2026-11-05-hearinfo-first-wire.txt`). Trailing bytes
    ///     beyond `7+len` are ignored — no invented semantics.
    ///   - trailing zero padding is tolerated (both the exact encoded bytes and
    ///     the padded 64-byte transfer shape decode).
    pub fn decode(buf: &[u8]) -> Result<Frame, DecodeError> {
        if buf.len() < 8 {
            // GET_REPORT replies ≥ 5 bytes are accepted by the app logic
            // (USBDataReceiveEvent: ≥5 for PID 1031), but a *well-formed*
            // ProtocolV2 frame needs ≥ 8 (header+cmd+len+tail). 5-byte partial
            // frames must never reach here silently.
            return Err(DecodeError::TooShort { len: buf.len() });
        }
        if buf[0] != HEADER[0] || buf[1] != HEADER[1] || buf[3] != HEADER[3] {
            // bytes 0,1 (`05 03`) and 3 (`DD`) are shared by both directions;
            // byte 2 (`AA`/`BB`) selects the direction, checked below.
            return Err(DecodeError::BadMagic {
                found: [buf[0], buf[1], buf[2], buf[3]],
            });
        }
        let from_device = buf[2] == REPLY_HEADER[2];
        if !from_device && buf[2] != HEADER[2] {
            // byte 2 is neither AA nor BB — magic unknown.
            return Err(DecodeError::BadMagic {
                found: [buf[0], buf[1], buf[2], buf[3]],
            });
        }
        let command = buf[4];
        // Payload length decode is APK-exact, NOT true big-endian:
        //   ProtocolV2.java:157: `int i = (USBUtils.toInt(bArr[0]) * 16)
        //   + USBUtils.toInt(bArr[1]);` with USBUtils.toInt(byte) = `b & 255`
        //   (USBUtils.java:7-8). So i = hi*16 + lo.
        // Every observed frame carries hi = 0 (payload ≤ 63 < 256 budget),
        // where hi*16 + lo == hi*256 + lo — coincidence, not equivalence.
        // Per project rule this formula is preserved verbatim until a capture
        // with payload > 255 bytes proves which formula the device speaks.
        let len = (buf[5] as usize) * 16 + (buf[6] as usize);
        if !from_device {
            // host→device frames carry the getData() terminator: tail byte at
            // 7+len must exist, so demand 7+len+1 bytes total.
            if buf.len() < 7 + len + 1 {
                return Err(DecodeError::Truncated {
                    need: 7 + len + 1,
                    len: buf.len(),
                });
            }
            if buf[7 + len] != TAIL {
                return Err(DecodeError::BadTail {
                    pos: 7 + len,
                    found: buf[7 + len],
                });
            }
        } else if buf.len() < 7 + len {
            // device replies have no terminator; only the payload must exist.
            return Err(DecodeError::Truncated {
                need: 7 + len,
                len: buf.len(),
            });
        }
        let payload = buf[7..7 + len].to_vec();
        Ok(Frame { command, payload })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// Payload does not fit one 63-byte feature report.
    PayloadTooLong { len: usize, max: usize },
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::PayloadTooLong { len, max } => {
                write!(f, "payload {len} bytes exceeds single-frame max {max}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    TooShort { len: usize },
    BadMagic { found: [u8; 4] },
    Truncated { need: usize, len: usize },
    BadTail { pos: usize, found: u8 },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::TooShort { len } => write!(f, "frame too short ({len} bytes < 8)"),
            DecodeError::BadMagic { found } => write!(f, "bad magic {found:02X?}"),
            DecodeError::Truncated { need, len } => {
                write!(f, "truncated frame: need {need} bytes, got {len}")
            }
            DecodeError::BadTail { pos, found } => {
                write!(
                    f,
                    "expected 0xEF terminator at offset {pos}, found {found:#04x}"
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {}
impl std::error::Error for DecodeError {}

/// The app's `ProtocolV2.crcCalculate` (`ProtocolV2.java:179-186`): plain XOR of
/// all bytes. NOT part of normal frames on the wire — kept for future
/// comparison work (e.g. OTA data blocks use it), and for "did we match the
/// app byte-for-byte" tooling. [code]
pub fn xor_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, &b| acc ^ b)
}
