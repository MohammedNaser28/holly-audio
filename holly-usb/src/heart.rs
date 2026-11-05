//! Typed GET_HEART_INFO status (§5 of docs/protocol.md).
//!
//! Field-by-field parse mirrors `ProGetHearInfo.receiveData`
//! (`re/apk/sources/com/hollyland/usbmic/protocolV2/ProGetHearInfo.java:50-114`)
//! including its incremental length gates. [wire] confirmation of the
//! single-TX field set (17 bytes at exactly these offsets):
//! `captures/2026-11-05-hearinfo-first-wire.txt`.

use crate::Error;

/// Length gate for the single-TX field set: `signalMode` is payload[16], so
/// the app requires `length >= 17`; both captures answered exactly 17 [wire].
const SINGLE_TX_FIELDS: usize = 17;

/// Status snapshot from `cmd 31 (GET_HEART_INFO)`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize),
    serde(rename_all = "snake_case")
)]
pub struct HeartInfo {
    pub tx1_status: bool,
    pub tx2_status: bool,
    /// Percent, 0..=100 (`USBUtils.toInt(byte)` = `b & 255`, unsigned).
    pub tx1_battery: u8,
    pub tx2_battery: u8,
    pub tx1_mute: bool,
    pub tx2_mute: bool,

    /// Noise-cancel switch (payload offset 6; the SET_NOISE toggle field).
    pub noise_switch: bool,
    /// NC level (payload offset 7; SET_NOISE keeps this unchanged).
    pub noise_level: u8,
    /// Gain.
    pub voice_level: u8,
    pub voice_mode: u8,
    /// Auto-power-off value.
    pub shutdown_time: u8,
    /// True when the light is **on** — the app reads `toInt(data[11]) == 0`
    /// (inverted byte semantics; `ProGetHearInfo.receiveData:75-76`).
    pub light_on: bool,
    pub eq_level: u8,
    pub reverb_switch: bool,
    pub reverb_level: u8,
    /// Identify-beep state.
    pub tx_identify: bool,
    pub signal_mode: u8,

    /// Multi-TX tail, interleaved in the payload (offsets 17-22). Present
    /// only when the reply is long enough — the certified single-TX replies
    /// (17 bytes) leave all of them `None` [wire].
    pub tx3_status: Option<bool>,
    pub tx4_status: Option<bool>,
    pub tx3_battery: Option<u8>,
    pub tx4_battery: Option<u8>,
    pub tx3_mute: Option<bool>,
    pub tx4_mute: Option<bool>,

    /// The full payload, verbatim, for anything this struct does not model.
    pub raw: Vec<u8>,
}

impl HeartInfo {
    /// Parse a `GET_HEART_INFO` reply payload. Gates are the APK's own
    /// (`data.length >= n` before reading `data[n-1]`); a payload below the
    /// certified 17-byte field set is rejected rather than defaulted
    /// (abort-on-malformed rule from the capture tooling).
    pub fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let len = payload.len();
        if len < SINGLE_TX_FIELDS {
            return Err(Error::HeartInfoShort { len });
        }
        // b[k] access is `USBUtils.toInt(data[k]) == <semantics>` [code]:
        //   `toInt` = `b & 255` (USBUtils.java:7-8) — the byte as-is here,
        //   Rust u8 is already unsigned.
        //   `x == 1` bools: ProGetHearInfo.receiveData (e.g. :53, :57).
        //   `light_on` inverts: `toInt(data[11]) == 0` (:75-76).
        let b = |k: usize| payload[k];
        Ok(Self {
            tx1_status: b(0) == 1,
            tx2_status: b(1) == 1,
            tx1_battery: b(2),
            tx2_battery: b(3),
            tx1_mute: b(4) == 1,
            tx2_mute: b(5) == 1,
            noise_switch: b(6) == 1,
            noise_level: b(7),
            voice_level: b(8),
            voice_mode: b(9),
            shutdown_time: b(10),
            light_on: b(11) == 0,
            eq_level: b(12),
            reverb_switch: b(13) == 1,
            reverb_level: b(14),
            tx_identify: b(15) == 1,
            signal_mode: b(16),
            // Interleaved multi-TX gates (:100-123): each field exists only
            // when the payload reaches its own offset.
            tx3_status: (len >= 18).then(|| b(17) == 1),
            tx4_status: (len >= 19).then(|| b(18) == 1),
            tx3_battery: (len >= 20).then(|| b(19)),
            tx4_battery: (len >= 21).then(|| b(20)),
            tx3_mute: (len >= 22).then(|| b(21) == 1),
            tx4_mute: (len >= 23).then(|| b(22) == 1),
            raw: payload.to_vec(),
        })
    }
}
