//! `Snapshot -> ViewModel`: the ONLY device-aware translation a frontend
//! needs. Pure functions; no I/O, not even deps beyond this crate.
//!
//! Frontends render [`ViewModel`] as-is. All knowledge of what a connection
//! state means (banner text incl. what to tell the user about
//! `packaging/70-hollyland.rules` or another process holding the claim)
//! lives HERE, in string form, so UI code never hardcodes device rules.

use serde::Serialize;

use crate::actor::{NoiseWrite, PendingWrite};
use crate::{ConnStatus, Snapshot};

/// Banner the UI should show (or `None` while clean/connected).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Banner {
    NotFound,
    /// Another process holds IF0 (wl: "close holly-cli or other instances").
    Busy,
    /// udev grant missing — UI displays `HINT_PERMISSION` below.
    PermissionDenied,
    /// Protocol error with its message.
    Error,
}

/// Ready-made, tested hint text for [`Banner::PermissionDenied`].
pub const HINT_PERMISSION: &str = "Permission denied — install packaging/70-hollyland.rules \
     (sudo cp packaging/70-hollyland.rules /etc/udev/rules.d/ && \
     sudo udevadm control --reload), then replug the dongle";

/// Titlebar device name — one place, never hardcoded in a view. Source: the
/// project owner confirms the rig is a Hollyland LARK A1 (task brief,
/// 2026-11-06); `docs/device.md` lists the marketing name as an open
/// question and the USB product string says only "Wireless Microphone".
/// Surfacing it here (instead of extending holly-usb, which is frozen) is
/// the decision recorded for T3; revisit if the product string can be
/// plumbed end-to-end.
pub const DEVICE_NAME: &str = "Lark A1";

/// Footer device id [code docs/protocol.md:3 — `PRODUCT_6108 = 1031 =
/// 0x0407 = device PID`, `inputs/lsusb-v.txt`; holly-usb's own NotFound
/// error words carry the same pair]. holly-usb's VENDOR_ID/PRODUCT_ID are
/// not re-exported and holly-usb is frozen, so the composition lives here
/// (the VM stays the only device-adjacent string source views may read);
/// the test pins it so a probe drift can never sneak in silently.
pub const VID_PID: &str = "3547:0407";

/// Which controls have a wire-certified SET path. ONE flag per design
/// control: when a capture lands for a read-only control, flipping its flag
/// (after the §1 gate in docs/gui-roadmap.md) promotes exactly that control —
/// no per-control hunting anywhere.
///
/// Evidence per flag:
/// - `nc_toggle`    — SET_NOISE (cmd 19) [wire
///   captures/2026-11-05-set-noise.txt E2/E4]; `nc_level` — the same
///   frame's second byte, written + verified [wire
///   captures/2026-11-05-noise-level-medium.txt session 3a]
/// - `eq` — SET_EQ (cmd 7) [wire] PC-originated guarded round trip
///   2026-11-09: frame `05 03 AA DD 07 00 01 {02,03} EF` → ack `00` →
///   hearts settled on the written pad
///   (captures/raw/set_eq_roundtrip.pcap + captures/set_eq_roundtrip.jsonl;
///   roadmap §1 route (b), checklist §3 EQ row)
/// - `volume` — SET_VOICE_LEVEL (cmd 3) [wire] PC-originated guarded
///   round trip 2026-11-10: frames `05 03 AA DD 03 00 01 {01,02} EF` →
///   ack `00` → hearts settled on the written stop
///   (captures/raw/set_gain_roundtrip.pcap + captures/
///   set_gain_roundtrip.jsonl; roadmap §1 route (b), checklist §3 Gain row)
/// - `reverb` — SET_REVERB (cmd 5) [wire] PC-originated guarded round trip
///   2026-11-11: frames `05 03 AA DD 05 00 02 {01 00, 01 01, 00 00} EF` →
///   acks `00` → hearts settled on the written pair (§5 offsets 13/14)
///   (captures/raw/set_reverb_roundtrip.pcap + captures/
///   set_reverb_roundtrip.jsonl; roadmap §1 route (b), checklist §3
///   Reverb row)
/// - `channel_mode` — SET_VOICE_MODE (cmd 1) [wire] PC-originated guarded
///   round trip 2026-11-11: frames `05 03 AA DD 01 00 01 {01,00} EF` →
///   acks `00` tail `0x67` → hearts settled on the written mode (§5
///   offset 9; the only heart byte that moved)
///   (captures/raw/set_channel_mode_roundtrip.pcap + captures/
///   set_channel_mode_roundtrip.jsonl; roadmap §1 route (b), checklist §3
///   Channel mode row)
/// - everything else is [code] at best (mic mute cmd 22, light cmd 33,
///   shutdown 39/40 …) and has never been captured, so it stays read-only
///   today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Writable {
    pub nc_toggle: bool,
    pub nc_level: bool,
    pub mic_mute: bool,
    pub volume: bool,
    pub channel_mode: bool,
    pub eq: bool,
    pub reverb: bool,
    pub mic_recognition: bool,
    pub schedule_power_off: bool,
    pub indicator_light: bool,
}

/// The writability table, exactly as the certified chain stands.
pub const WRITABLE: Writable = Writable {
    nc_toggle: true,
    nc_level: true,
    mic_mute: false,
    volume: true,
    channel_mode: true,
    eq: true,
    reverb: true,
    mic_recognition: false,
    schedule_power_off: false,
    indicator_light: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryClass {
    Full,
    High,
    Low,
    Critical,
}

/// Battery color-class thresholds. PRESENTATION-ONLY: neither the APK nor
/// any capture defines device-side battery thresholds; these bands
/// (75/40/15 %) are a display choice, documented here as not protocol data.
pub fn battery_class(pct: u8) -> BatteryClass {
    match pct {
        0..=14 => BatteryClass::Critical,
        15..=39 => BatteryClass::Low,
        40..=74 => BatteryClass::High,
        _ => BatteryClass::Full,
    }
}

/// Battery segments LIT on the 5-segment bar: ~20 % per segment, rounding
/// up so any real charge above zero shows its segment (93 % → 5, 80 % → 4,
/// 20 % → 1, 0 → 0). PRESENTATION-ONLY, same non-protocol rule as
/// [`battery_class`]. DEVIATION from the design mock (which lit all five on
/// any live link, index.html:229): the mock's "5 = alive, not 5 = full" reading
/// hid the real value; `battery_digits` keeps the exact number, this keeps the
/// bar honest with it. Documented in frontends/gpui/README.md.
pub fn battery_segment_count(pct: u8) -> usize {
    pct.div_ceil(20) as usize
}

/// NC level byte -> the phone app's own value name [code
/// HLDevice6302Activity.java:1593/:1610/:1626-1631; §5.1 identity mapping
/// strong = 2, medium = 1, low = 0 — read 0/1 and written 1/2 [wire
/// captures/2026-11-05-noise-level-medium.txt]]. The UI selector order
/// (Weak, Medium, Strong) IS byte order, so this is the only translation
/// the byte needs; anything outside the APK's 0/1/2 set is "Unknown" and
/// renders as no selection.
pub fn nc_level_label(level: u8) -> &'static str {
    match level {
        0 => "Weak",
        1 => "Medium",
        2 => "Strong",
        _ => "Unknown",
    }
}

/// The Volume panel's selected pad. Certified 1-based knob: session 3b read
/// `voiceLevel = 03` [wire] while the user's phone showed slider stop "4"
/// (docs/protocol.md:246-249, `setSelectIndex(voiceLevel)` [:1283]), so
/// byte N = display stop N+1 = the Nth pad of the six-pad row (pads are
/// "1".."6", index 0..5 — reference index.html:260). A byte outside 0..=5
/// has no certified pad and maps to None (no selection, never a guess).
pub fn volume_selection(voice_level: u8) -> Option<usize> {
    (voice_level <= 5).then_some(voice_level as usize)
}

/// The inverse translation a UI click needs: a Volume pad index
/// ([`GainVm::selected`]'s space) -> the WIRE byte `set_gain` writes.
/// Identity on 0..=5: the §3 Gain row's write space IS the §5.2 read
/// space — no stop inversion exists to undo (write space == read space
/// [wire round trip 2026-11-10, checklist §3 Gain row]). Pure function
/// like [`eq_pad_wire`] so frontends never hardcode device values;
/// unmapped (out of 0..=5) returns None.
pub fn volume_pad_wire(index: usize) -> Option<u8> {
    (index <= 5).then_some(index as u8)
}

/// The Channel-mode panel's selected pad. Identity on the wire byte:
/// mono = 0, stereo = 1 [code
/// `HLDevice6302Activity.java:1803-1805` — `monoMode.setSelected(value==0)`
/// / `stereoMode.setSelected(value==1)`; the app drives these pads from
/// `heartInfo.getVoiceMode()` (:1277)]. Anything > 1 has no certified pad.
pub fn channel_mode_selection(voice_mode: u8) -> Option<usize> {
    (voice_mode <= 1).then_some(voice_mode as usize)
}

/// The inverse translation a UI click needs: a Channel-mode pad index
/// ([`ChannelModeVm::selected`]'s space) -> the WIRE byte
/// `set_channel_mode` writes. Identity on 0..=1: the §3 Channel-mode
/// row's write space IS the §5.2 read space — identity write-verified
/// [wire, 2026-11-11 channel-mode run: the 0→1→0 round trip landed on
/// the written byte every leg (§5 offset 9), never out of space].
/// Pure function like [`volume_pad_wire`] so frontends never hardcode
/// device values; unmapped (out of 0..=1) returns None.
pub fn channel_mode_pad_wire(index: usize) -> Option<u8> {
    (index <= 1).then_some(index as u8)
}

/// The Schedule-panel's selected pad: `quarterTime` ("15min") = 0,
/// `neverTime` ("Never") = 1 [code
/// `HLDevice6302Activity.java:1864-1866`, driven by
/// `heartInfo.getShutdownTime()` (:1278)]. Anything > 1 is unmapped.
pub fn shutdown_selection(shutdown_time: u8) -> Option<usize> {
    (shutdown_time <= 1).then_some(shutdown_time as usize)
}

/// The EQ panel's selected pad, in the design's display order
/// ["Equalization", "Low", "Bright"]. The APK's own visual order is
/// `balance_btn`, `overcast_btn`, `bright_btn` [code
/// `hldevice_6108.xml:156-166`] with the SAME strings the design copied
/// ("Equalization"/"Low"/"Bright", `hldevice_6108.xml:1290-1292`), and the
/// bytes are `DeviceConnectVariable.EQ` = BALANCE(3), OVERCAST(1),
/// BRIGHT(2) [code :556-574, selected by `updateEqView` at
/// `HLDevice6302Activity.java:1869-1874`]. So display index 0..2 maps to
/// wire bytes 3/1/2; anything else (including 0 — the byte space is
/// 1-based) has no certified pad.
pub fn eq_selection(eq_level: u8) -> Option<usize> {
    match eq_level {
        3 => Some(0), // Balance  ("Equalization")
        1 => Some(1), // Overcast ("Low")
        2 => Some(2), // Bright   ("Bright")
        _ => None,
    }
}

/// The inverse translation a UI click needs: a DISPLAY-order pad index
/// ([`EqVm::selected`]'s space) -> the WIRE byte `set_eq` writes. Pure
/// function so the mapping is tested once here and frontends never
/// hardcode device values; unmapped (out of 0..2) returns None.
pub fn eq_pad_wire(index: usize) -> Option<u8> {
    match index {
        0 => Some(3), // Balance  ("Equalization")
        1 => Some(1), // Overcast ("Low")
        2 => Some(2), // Bright   ("Bright")
        _ => None,
    }
}

/// The Reverb panel's selected pad ["Small", "Medium", "Large"]:
/// GEAR_ONE = 0, GEAR_TWO = 1, GEAR_THREE = 2 [code
/// `DeviceConnectVariable.Reverb` (:629-632), selected by
/// `updateReverbView` at `HLDevice6302Activity.java:1883-1891`], and the
/// APK's gear strings are exactly the design's words
/// (`hldevice_6108.xml:792-794`: gear_one="Small", gear_two="Medium",
/// gear_three="Large" in visual order :204-214). Anything > 2 is unmapped.
pub fn reverb_selection(reverb_level: u8) -> Option<usize> {
    (reverb_level <= 2).then_some(reverb_level as usize)
}

/// The inverse translation a UI gear pick needs: a DISPLAY-order pad index
/// ([`reverb_selection`]'s space) -> the WIRE byte `set_reverb` writes.
/// Identity on 0..=2: the checklist §3 Reverb row's write space IS the §5.2
/// read space — identity write-verified [wire, 2026-11-11 reverb run: the
/// 0→1 gear flip landed while the switch was on, protocol.md §4 row 5/6].
/// Pure function like [`volume_pad_wire`] so frontends never hardcode
/// device values; unmapped (out of 0..=2) returns None.
pub fn reverb_gear_wire(index: usize) -> Option<u8> {
    (index <= 2).then_some(index as u8)
}

/// The reverb toggle's WIRE pair `(switch, gear)`: the requested switch
/// byte plus the device's CURRENT gear as the value byte — the APK's
/// switch handler sends the remembered `reverbType` on BOTH directions
/// (`setReverb(this.reverbType, z)`, HLDevice6302Activity.java:792-793),
/// and the 2026-11-11 round trip proved the gear byte rides UNTOUCHED
/// through a toggle on the write path (frame 22 `05 03 AA DD 05 00 02
/// 01 00 EF` written to a resting (off, gear 0) device — protocol.md §4
/// row 5/6). An unknown gear (`None` — a byte outside the certified
/// 0..=2 space, or no heart yet) falls back to the APK's own default: the
/// field initializes to GEAR_ONE = 0 [code
/// HLDevice6302Activity.java:341, `DeviceConnectVariable.Reverb`]. Pure
/// function so frontends never hardcode device values.
pub fn reverb_toggle_wire(switch: bool, gear: Option<u8>) -> (bool, u8) {
    (switch, gear.unwrap_or(0))
}

/// The NC panel's status word: one place, driven ONLY by NcVm — a pending
/// (actor-reported, latest-wins) write says "Applying" while the panel keeps
/// showing the previous device state; an unknown state is "Unavailable".
pub fn nc_status(nc: &NcVm) -> &'static str {
    match nc.pending {
        Some(_) => "Applying",
        None => match nc.state {
            Some(true) => "On",
            Some(false) => "Off",
            None => "Unavailable",
        },
    }
}

/// Short connection word for the banner title slot AND the footer LED text:
/// a live connection names itself; anything else borrows the banner's word.
/// Pure (no device I/O), tested.
pub fn conn_status_word(vm: &ViewModel) -> &'static str {
    match vm.banner {
        Some(Banner::Error) => "Device error",
        Some(Banner::Busy) => "Receiver in use",
        Some(Banner::PermissionDenied) => "Access blocked",
        Some(Banner::NotFound) => "No receiver",
        None if vm.banner_text.is_some() => "Searching",
        None => "Receiver connected",
    }
}

/// Mic-card status line: a live link says so explicitly; an unverifiable
/// transmitter never inherits inventing state.
pub fn mic_status(connected: bool) -> &'static str {
    if connected { "Connected" } else { "No data" }
}

/// Per-mic mute button wording. Muted (known and true) reads as state, not
/// as an alarm: no red, ever (design colour rule).
pub fn mute_label(muted: bool) -> &'static str {
    if muted { "Muted" } else { "Mute" }
}

/// The inverse translation a UI click needs: a mic-card index (the
/// design's Mic 1/Mic 2 cards, 0-based) -> the WIRE slot byte
/// `set_tx_mute` writes. Identity on 0..=1: the card order IS the wire's
/// TX1/TX2 order — `txIndex` = `coerceIn(deviceType.ordinal() - 1, 0, 3)`
/// [code `UsbMessengerV2.java:206-210`, callers naming TX1 first at
/// `HLDevice6302Activity.java:677`], so Mic 1 → slot 0, Mic 2 → slot 1.
/// The GUI has exactly two mic cards (design: "Mic 1/Mic 2"), so indices
/// beyond 1 have no card and no write — even though the wire protocol
/// itself addresses four slots. Pure function like [`volume_pad_wire`]
/// so frontends never hardcode device values.
pub fn tx_mute_wire(mic: usize) -> Option<u8> {
    (mic <= 1).then_some(mic as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TxVm {
    pub connected: bool,
    /// Percent 0..=100 (HeartInfo, §5 of docs/protocol.md).
    pub battery: u8,
    pub battery_class: BatteryClass,
    pub muted: bool,
    /// A TX-mute write ADDRESSED TO THIS PANEL (`PendingWrite::TxMute`
    /// carrying this panel's own wire slot) is in flight — the mute chip
    /// dims to the existing in-flight look and ignores clicks until the
    /// device's verdict. Per-slot scoping: a Mic 1 write never dims
    /// Mic 2 (one TX per frame, checklist §3).
    pub applying: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NcVm {
    /// The device's current NC state (the ONLY value a toggle may display
    /// as actual state; changes only via snapshots).
    pub state: Option<bool>,
    /// The device's current level byte (§5 offset 7, wire-observed values
    /// so far: 0 and 1 — captures/2026-11-05-*.txt). Drives which level
    /// button is ACTIVE; read-only.
    pub level: Option<u8>,
    /// The actor-reported in-flight/pending SET, latest-wins. UI shows
    /// "pending" only from THIS field — never optimistically. The enum says
    /// WHICH byte is in flight (switch flip or level write), so each control
    /// greys against its own target (`NoiseWrite::Switch` for the toggle,
    /// `NoiseWrite::Level` for a level button).
    pub pending: Option<NoiseWrite>,
    /// Message from the last failed SET, when one is being shown.
    pub last_error: Option<String>,
}

/// EQ pad state: the DEVICE's current pad in DISPLAY (APK) order, plus
/// whether an EQ write is in flight. `selected` is None for any
/// non-certified byte (the pad space is 1-based; see [`eq_selection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EqVm {
    /// Display-order index of the active pad (0 = "Equalization",
    /// 1 = "Low", 2 = "Bright" [code DeviceConnectVariable.EQ]).
    pub selected: Option<usize>,
    /// An EQ SET (only ever [`PendingWrite::Eq`]) is in flight — the
    /// selector dims and ignores picks until the device's verdict.
    pub applying: bool,
}

/// Gain (Volume adjustment) state: the DEVICE's current stop in pad
/// order, plus whether a gain write is in flight. `selected` is None for
/// any non-certified byte outside 0..=5 (see [`volume_selection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GainVm {
    /// Pad index of the active stop (0..=5 = the phone's "1".."6" —
    /// [wire session 3b, byte N = stop N+1  identity on the wire byte
    /// for every value the round trip exercised, 01/02/03]).
    pub selected: Option<usize>,
    /// A gain SET (only ever [`PendingWrite::Gain`]) is in flight — the
    /// selector dims and ignores picks until the device's verdict.
    pub applying: bool,
}

/// Channel-mode state: the DEVICE's current mode pad, plus whether a
/// channel-mode write is in flight. `selected` is None for any
/// non-certified byte (§5 offset 9; see [`channel_mode_selection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChannelModeVm {
    /// Pad index of the active mode (0 = Mono, 1 = Stereo — [code
    /// `HLDevice6302Activity.java:1803-1805`]).
    pub selected: Option<usize>,
    /// A channel-mode SET (only ever [`PendingWrite::ChannelMode`]) is in
    /// flight — the selector dims and ignores picks until the device's
    /// verdict.
    pub applying: bool,
}

/// Reverb state: the DEVICE's current switch and gear bytes, plus whether
/// a reverb write is in flight. `enabled` is the verified switch byte
/// (§5 offset 13) — None only where no heart exists, so a toggle never
/// invents a flip target; `gear` is Some only for a byte in the certified
/// space 0..=2 (§5 offset 14, see [`reverb_selection`]) — an unknown byte
/// is no selection and never rides a toggle ([`reverb_toggle_wire`]'s
/// GEAR_ONE fallback covers it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReverbVm {
    /// The device's current reverb switch (the ONLY value the toggle may
    /// display as actual state; changes only via snapshots).
    pub enabled: Option<bool>,
    /// The device's current gear byte (§5 offset 14), Some only inside the
    /// certified 0..=2 space; None = no certified selection.
    pub gear: Option<u8>,
    /// A reverb SET (only ever [`PendingWrite::Reverb`]) is in flight —
    /// the panel's controls dim and ignore picks until the device's
    /// verdict.
    pub applying: bool,
}

/// Read-only panels for exactly what HeartInfo verifiably provides
/// (docs/protocol.md §5); nothing else is exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewModel {
    pub banner: Option<Banner>,
    /// Full explanation text for the banner (banner + detail).
    pub banner_text: Option<String>,
    pub tx1: Option<TxVm>,
    pub tx2: Option<TxVm>,
    /// Multi-TX tail — present only in >= 18-byte replies [wire]; the
    /// certified 17-byte shape leaves these None.
    pub tx3: Option<TxVm>,
    pub tx4: Option<TxVm>,
    pub nc: NcVm,
    /// Receiver indicator light (HeartInfo §5 offset 11; the byte is
    /// inverted — 0 means on `[code ProGetHearInfo.java:75-76]`, captured
    /// value [wire]). Read-only until SET_LIGHT (cmd 33) passes the §1 gate.
    pub light: Option<bool>,
    /// Gain (§5 field `voiceLevel`) as the selector's VM state
    /// ([`GainVm`]): display-order selection from the device's byte
    /// (§5 offset 8; write space == read space, 0..=5 — checklist §3
    /// Gain row), plus the in-flight flag for its own panel only.
    pub volume: GainVm,
    /// EQ pads ([`EqVm`]): display-order selection from the device's
    /// `eqLevel` byte (§5 field, offset 12; pad space is 1/2/3 [code
    /// DeviceConnectVariable.EQ]), plus the in-flight flag for its own
    /// panel only.
    pub eq: EqVm,
    /// Identify-beep state (§5 offset 15, `== 1` semantics [code
    /// ProGetHearInfo.java:88]). The Mic-recognition panel's switch reads
    /// THIS — it used to be hardcoded `false` in the view, which could
    /// contradict the device when the phone-side identify was on.
    pub mic_recognition: Option<bool>,
    /// Channel-mode pads ([`ChannelModeVm`]): display-order selection
    /// from the device's `voiceMode` byte (§5 offset 9; 0 = Mono, 1 =
    /// Stereo [code HLDevice6302Activity.java:1803-1805]), plus the
    /// in-flight flag for its own panel only.
    pub channel_mode: ChannelModeVm,
    /// Schedule pad (`shutdownTime`, §5 offset 10) via
    /// [`shutdown_selection`]: 0 = 15min, 1 = Never [code
    /// HLDevice6302Activity.java:1864-1866]. Unmapped for any other byte.
    pub shutdown: Option<usize>,
    /// §5 offset 16 raw (`signalMode`). The APK parses it but renders it
    /// nowhere (no `update*View` consumer in the decompiled activity) and
    /// no design panel exists — carried read-only, surfaced nowhere by
    /// policy: no invented slot.
    pub signal_mode: Option<u8>,
    pub reverb: Option<ReverbVm>,
    /// Echo of [`Snapshot::received_at`] (actor-stamped wall clock) for the
    /// footer's "last reply" display. App metadata, not device data.
    pub updated: Option<u64>,
}

/// Pure transform: one current Snapshot => the full display model.
pub fn view_model(s: &Snapshot) -> ViewModel {
    match s.status {
        ConnStatus::Connecting => ViewModel {
            banner: None,
            banner_text: Some("Searching for the dongle…".to_string()),
            tx1: None,
            tx2: None,
            tx3: None,
            tx4: None,
            nc: NcVm {
                state: None,
                level: None,
                pending: None,
                last_error: None,
            },
            light: None,
            volume: GainVm {
                selected: None,
                applying: false,
            },
            eq: EqVm {
                selected: None,
                applying: false,
            },
            mic_recognition: None,
            channel_mode: ChannelModeVm {
                selected: None,
                applying: false,
            },
            shutdown: None,
            signal_mode: None,
            reverb: None,
            updated: None,
        },
        ConnStatus::NotFound => ViewModel {
            banner: Some(Banner::NotFound),
            banner_text: Some("No dongle (3547:0407) on the bus".to_string()),
            ..disconnected_core()
        },
        ConnStatus::Busy => ViewModel {
            banner: Some(Banner::Busy),
            banner_text: Some(
                "Another process is using the dongle — close holly-cli or other instances, then try again"
                    .to_string(),
            ),
            ..disconnected_core()
        },
        ConnStatus::PermissionDenied => ViewModel {
            banner: Some(Banner::PermissionDenied),
            banner_text: Some(HINT_PERMISSION.to_string()),
            ..disconnected_core()
        },
        ConnStatus::Error => ViewModel {
            banner: Some(Banner::Error),
            banner_text: Some(
                s.detail
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
            )
            .map(|mut text| {
                if let Some(heart) = &s.heart {
                    // Show the real, verified device state alongside the
                    // failure text. nc state is the only writable field.
                    text.push_str(&format!(
                        " --- device still reports nc={} (level {})",
                        if heart.noise_switch { "on" } else { "off" },
                        heart.noise_level
                    ));
                }
                text
            }),
            ..disconnected_core()
        },
        ConnStatus::Connected => {
            // The real panels. Unreachable-heart is impossible by actor
            // construction (Connected snapshots always carry a heart).
            let Some(heart) = &s.heart else {
                return ViewModel::default();
            };
            ViewModel {
                banner: None,
                banner_text: None,
                tx1: Some(tx_vm(
                    heart.tx1_status,
                    heart.tx1_battery,
                    heart.tx1_mute,
                    tx_applying(s, 0),
                )),
                tx2: Some(tx_vm(
                    heart.tx2_status,
                    heart.tx2_battery,
                    heart.tx2_mute,
                    tx_applying(s, 1),
                )),
                tx3: tail_tx(
                    heart.tx3_status,
                    heart.tx3_battery,
                    heart.tx3_mute,
                    tx_applying(s, 2),
                ),
                tx4: tail_tx(
                    heart.tx4_status,
                    heart.tx4_battery,
                    heart.tx4_mute,
                    tx_applying(s, 3),
                ),
                nc: NcVm {
                    state: Some(heart.noise_switch),
                    level: Some(heart.noise_level),
                    // Only a NC write pends THIS panel: the one-bucket
                    // pending is filtered down per family so an EQ write
                    // never greys the NC controls (and vice versa).
                    pending: match s.pending_set {
                        Some(PendingWrite::Noise(w)) => Some(w),
                        Some(PendingWrite::Eq(_))
                        | Some(PendingWrite::Gain(_))
                        | Some(PendingWrite::Reverb(_, _))
                        | Some(PendingWrite::ChannelMode(_))
                        | Some(PendingWrite::TxMute { .. })
                        | None => None,
                    },
                    last_error: None,
                },
                light: Some(heart.light_on),
                volume: GainVm {
                    selected: volume_selection(heart.voice_level),
                    applying: matches!(s.pending_set, Some(PendingWrite::Gain(_))),
                },
                eq: EqVm {
                    selected: eq_selection(heart.eq_level),
                    applying: matches!(s.pending_set, Some(PendingWrite::Eq(_))),
                },
                mic_recognition: Some(heart.tx_identify),
                channel_mode: ChannelModeVm {
                    selected: channel_mode_selection(heart.voice_mode),
                    applying: matches!(s.pending_set, Some(PendingWrite::ChannelMode(_))),
                },
                shutdown: shutdown_selection(heart.shutdown_time),
                signal_mode: Some(heart.signal_mode),
                reverb: Some(ReverbVm {
                    enabled: Some(heart.reverb_switch),
                    // The certified gear byte only: `reverb_selection`'s
                    // 0..=2 space, mapped back to the wire byte. Anything
                    // else (a byte outside the APK's GEAR_ONE..THREE) is
                    // no selection and never rides a toggle.
                    gear: reverb_selection(heart.reverb_level).map(|pad| pad as u8),
                    applying: matches!(s.pending_set, Some(PendingWrite::Reverb(_, _))),
                }),
                updated: s.received_at,
            }
        }
    }
}

fn disconnected_core() -> ViewModel {
    ViewModel {
        banner: None, // set by each arm
        banner_text: None,
        tx1: None,
        tx2: None,
        tx3: None,
        tx4: None,
        nc: NcVm {
            state: None,
            level: None,
            pending: None,
            last_error: None,
        },
        light: None,
        volume: GainVm {
            selected: None,
            applying: false,
        },
        eq: EqVm {
            selected: None,
            applying: false,
        },
        mic_recognition: None,
        channel_mode: ChannelModeVm {
            selected: None,
            applying: false,
        },
        shutdown: None,
        signal_mode: None,
        reverb: None,
        updated: None,
    }
}

/// Per-slot pending scoping for the mic cards: a TX-mute write dims ONLY
/// the panel whose wire slot it addresses (tx 0..=3 = tx1..tx4) — every
/// other family's pending never touches a mic card. Mirrors the per-family
/// filters the NC/EQ/gain/reverb/channel panels apply to the same
/// one-bucket pending.
fn tx_applying(s: &Snapshot, slot: u8) -> bool {
    matches!(
        s.pending_set,
        Some(PendingWrite::TxMute { tx, .. }) if tx == slot
    )
}

fn tx_vm(connected: bool, battery: u8, muted: bool, applying: bool) -> TxVm {
    TxVm {
        connected,
        battery,
        battery_class: battery_class(battery),
        muted,
        applying,
    }
}

fn tail_tx(
    status: Option<bool>,
    battery: Option<u8>,
    mute: Option<bool>,
    applying: bool,
) -> Option<TxVm> {
    // All three fields appear at their own offsets together [code
    // ProGetHearInfo.java:100-123]; if any is None they are None together.
    match (status, battery, mute) {
        (Some(st), Some(bat), Some(m)) => Some(tx_vm(st, bat, m, applying)),
        _ => None,
    }
}

impl Default for ViewModel {
    /// All-empty, banner-less — the "not yet seen anything" shape used only
    /// in impossible states (actor never emits Connected without a heart).
    fn default() -> Self {
        disconnected_core()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnStatus, Snapshot};
    use holly_usb::HeartInfo;

    /// E1 payload (17 bytes), capture `2026-11-05-set-noise.txt` — same
    /// vector the actor tests parse through the transport.
    const E1_PAYLOAD: [u8; 17] = [0, 1, 0, 93, 0, 0, 1, 0, 3, 0, 0, 0, 3, 0, 2, 0, 0];

    fn e1() -> HeartInfo {
        HeartInfo::from_payload(&E1_PAYLOAD).expect("E1 parses")
    }

    fn snap(
        status: ConnStatus,
        heart: Option<HeartInfo>,
        pending: Option<PendingWrite>,
        detail: Option<&str>,
    ) -> Snapshot {
        Snapshot {
            status,
            detail: detail.map(str::to_string),
            heart,
            pending_set: pending,
            received_at: None,
        }
    }

    fn banner_of(status: ConnStatus) -> String {
        view_model(&snap(status, None, None, None))
            .banner_text
            .expect("banner states carry text")
    }

    #[test]
    fn connecting_is_not_a_banner() {
        let vm = view_model(&snap(ConnStatus::Connecting, None, None, None));
        assert_eq!(vm.banner, None);
        let text = vm.banner_text.expect("but it does say something");
        assert!(text.contains("Searching"), "{text}");
    }

    #[test]
    fn not_found_busy_denied_banners() {
        assert_eq!(
            view_model(&snap(ConnStatus::NotFound, None, None, None)).banner,
            Some(Banner::NotFound)
        );
        assert!(banner_of(ConnStatus::NotFound).contains("No dongle"));

        assert_eq!(
            view_model(&snap(ConnStatus::Busy, None, None, None)).banner,
            Some(Banner::Busy)
        );
        let busy = banner_of(ConnStatus::Busy);
        assert!(busy.contains("Another process"), "{busy}");
        assert!(busy.contains("holly-cli"), "{busy}");

        assert_eq!(
            view_model(&snap(ConnStatus::PermissionDenied, None, None, None)).banner,
            Some(Banner::PermissionDenied)
        );
        assert!(banner_of(ConnStatus::PermissionDenied).contains("70-hollyland.rules"));
    }

    #[test]
    fn error_without_heart_shows_detail_only() {
        let vm = view_model(&snap(
            ConnStatus::Error,
            None,
            None,
            Some("malformed device reply: …"),
        ));
        assert_eq!(vm.banner, Some(Banner::Error));
        assert_eq!(vm.banner_text.unwrap(), "malformed device reply: …");
        // No panels in an Error snapshot without a heart.
        assert!(vm.tx1.is_none() && vm.tx2.is_none() && vm.nc.state.is_none());
    }

    #[test]
    fn error_with_heart_shows_the_real_device_state() {
        let vm = view_model(&snap(
            ConnStatus::Error,
            Some(e1()),
            None,
            Some("device rejected the SET (status 0x02)"),
        ));
        let text = vm.banner_text.expect("banner text");
        assert_eq!(vm.banner, Some(Banner::Error));
        assert!(
            text.contains("device rejected the SET (status 0x02)"),
            "{text}"
        );
        assert!(text.contains("nc=on"), "real NC state embedded: {text}");
        assert!(text.contains("level 0"), "real NC level embedded: {text}");
    }

    #[test]
    fn error_without_detail_falls_back() {
        let vm = view_model(&snap(ConnStatus::Error, None, None, None));
        assert_eq!(vm.banner_text.as_deref(), Some("Unknown error"));
    }

    /// Presentation-only thresholds (never protocol data) — boundary sweep.
    #[test]
    fn battery_class_boundaries() {
        use BatteryClass::*;
        for (pct, want) in [
            (0, Critical),
            (14, Critical),
            (15, Low),
            (39, Low),
            (40, High),
            (74, High),
            (75, Full),
            (100, Full),
        ] {
            assert_eq!(battery_class(pct), want, "battery {pct}%");
        }
    }

    /// Connected: NC state comes from the heart, pending passes through from
    /// the actor; the toggle may only ever be driven by these two fields.
    #[test]
    fn nc_passthrough() {
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Noise(NoiseWrite::Switch(false))),
            None,
        ));
        assert_eq!(
            vm.nc,
            NcVm {
                state: Some(true),
                level: Some(0),
                pending: Some(NoiseWrite::Switch(false)),
                last_error: None
            }
        );

        // NC off heart + pending switch flip → both visible simultaneously.
        let mut off = e1();
        off.noise_switch = false;
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(off),
            Some(PendingWrite::Noise(NoiseWrite::Switch(true))),
            None,
        ));
        assert_eq!(vm.nc.state, Some(false));
        assert_eq!(vm.nc.pending, Some(NoiseWrite::Switch(true)));

        // Level write pending with switch-on heart: state stays the device's,
        // the pending carries the target byte only.
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Noise(NoiseWrite::Level(2))),
            None,
        ));
        assert_eq!(vm.nc.state, Some(true));
        assert_eq!(vm.nc.pending, Some(NoiseWrite::Level(2)));
    }

    #[test]
    fn connected_panels_match_the_certified_heart() {
        let vm = view_model(&snap(ConnStatus::Connected, Some(e1()), None, None));
        assert_eq!(vm.banner, None);
        // tx1/tx2 are always-present panels when a heart exists; E1 says tx1
        // disconnected, tx2 connected.
        let tx1 = vm.tx1.expect("panel even for a disconnected TX");
        assert!(!tx1.connected);
        assert_eq!(tx1.battery, 0);
        let tx2 = vm.tx2.expect("tx2 connected in E1");
        assert!(tx2.connected);
        assert_eq!(tx2.battery, 93);
        assert_eq!(tx2.battery_class, BatteryClass::Full);
        assert!(!tx2.muted);
        assert_eq!(vm.tx3, None, "tail TXs absent in the 17-byte shape");
        assert_eq!(vm.tx4, None);
        assert_eq!(
            vm.volume,
            GainVm {
                selected: Some(3), // wire byte 3 = the phone's stop "4"
                applying: false
            }
        );
        assert_eq!(
            vm.eq,
            EqVm {
                selected: Some(0), // wire pad 3 = "Equalization" (balance)
                applying: false
            }
        );
        assert_eq!(
            vm.reverb,
            Some(ReverbVm {
                enabled: Some(false),
                gear: Some(2),
                applying: false
            })
        );
        // The formerly-dropped fields: E1's own bytes (cite §5 offsets).
        assert_eq!(vm.mic_recognition, Some(false), "txIdentify off [15]");
        assert_eq!(
            vm.channel_mode,
            ChannelModeVm {
                selected: Some(0), // voiceMode 0 = Mono [9]
                applying: false
            }
        );
        assert_eq!(vm.shutdown, Some(0), "shutdownTime 0 = 15min [10]");
        assert_eq!(vm.signal_mode, Some(0), "carried raw [16]");
    }

    /// Tail TXs appear only when the reply is long enough — and each field
    /// is gated by its own offset (ProGetHearInfo.java:100-123 [code]), so a
    /// partially-populated triple collapses to `None` in the VM.
    #[test]
    fn tail_tx_grouping() {
        // Interior tail layout [code]: payload[17]=tx3_status, [18]=tx4_status,
        // [19]=tx3_battery, [20]=tx4_battery, [21]=tx3_mute, [22]=tx4_mute.
        // Each new length byte enables exactly one more field.
        let new_field: [u8; 6] = [1, 1, 80, 70, 1, 0];
        let mut payload = E1_PAYLOAD.to_vec();
        for value in new_field {
            payload.push(value);
            let len = payload.len();
            let heart = HeartInfo::from_payload(&payload).expect("payload parses");
            let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
            match len {
                18..=20 => {
                    // tx3 lacks battery/mute, tx4 lacks everything after
                    // status — both collapse to None.
                    assert_eq!(vm.tx3, None, "len {len}: partial triple collapses");
                    assert_eq!(vm.tx4, None, "len {len}: partial triple collapses");
                }
                21 => {
                    // tx3: status+battery Some, mute None → collapse;
                    // tx4: status+battery Some, mute None → collapse.
                    assert_eq!(vm.tx3, None, "len 21");
                    assert_eq!(vm.tx4, None, "len 21");
                }
                22 => {
                    // tx3 complete: connected, 80 % = Full, muted.
                    let tx3 = vm.tx3.expect("tx3 complete at len 22");
                    assert!(tx3.connected);
                    assert_eq!(tx3.battery, 80);
                    assert_eq!(tx3.battery_class, BatteryClass::Full);
                    assert!(tx3.muted);
                    // tx4: mute still absent → collapse.
                    assert_eq!(vm.tx4, None, "len 22: tx4 mute gated at 23");
                }
                23 => {
                    let tx3 = vm.tx3.expect("tx3 complete");
                    assert!(tx3.connected);
                    assert_eq!(tx3.battery, 80);
                    let tx4 = vm.tx4.expect("tx4 complete at len 23");
                    assert!(tx4.connected);
                    assert_eq!(tx4.battery, 70);
                    assert_eq!(tx4.battery_class, BatteryClass::High);
                    assert!(!tx4.muted);
                }
                _ => unreachable!("only lens 18..=23 are scripted"),
            }
        }
    }

    /// NC level names: the §5.1 identity mapping, owned here so no view
    /// reimplements the byte→name translation; out-of-range bytes are
    /// "Unknown" (rendered as no selection), never a guessed name.
    #[test]
    fn nc_level_labels_are_the_apk_identity_map() {
        assert_eq!(nc_level_label(0), "Weak");
        assert_eq!(nc_level_label(1), "Medium");
        assert_eq!(nc_level_label(2), "Strong");
        assert_eq!(nc_level_label(3), "Unknown");
        assert_eq!(nc_level_label(255), "Unknown");
    }

    /// Volume pads: the certified 1-based knob (session 3b: byte 03 = the
    /// phone's stop "4", protocol.md:246-249) — pad index == byte for
    /// 0..=5, anything wider has no pad and draws no selection.
    #[test]
    fn volume_selection_is_the_certified_identity() {
        assert_eq!(volume_selection(0), Some(0), "pad 1");
        assert_eq!(volume_selection(3), Some(3), "pad 4 — session 3b");
        assert_eq!(volume_selection(5), Some(5), "pad 6");
        assert_eq!(volume_selection(6), None, "no certified pad");
        assert_eq!(volume_selection(255), None, "no certified pad");
    }

    /// Channel mode: mono = 0, stereo = 1 (identity) [code
    /// HLDevice6302Activity.java:1803-1805]; anything else unselected.
    #[test]
    fn channel_mode_selection_is_the_apk_identity() {
        assert_eq!(channel_mode_selection(0), Some(0), "Mono");
        assert_eq!(channel_mode_selection(1), Some(1), "Stereo");
        assert_eq!(channel_mode_selection(2), None, "no certified pad");
        assert_eq!(channel_mode_selection(255), None, "no certified pad");
    }

    /// Schedule pads: 0 = quarter ("15min"), 1 = never ("Never") [code
    /// HLDevice6302Activity.java:1864-1866]; anything else unselected.
    #[test]
    fn shutdown_selection_is_the_apk_identity() {
        assert_eq!(shutdown_selection(0), Some(0), "15min");
        assert_eq!(shutdown_selection(1), Some(1), "Never");
        assert_eq!(shutdown_selection(2), None, "no certified pad");
        assert_eq!(shutdown_selection(255), None, "no certified pad");
    }

    /// EQ pads: display order ["Equalization","Low","Bright"] = wire bytes
    /// [3,1,2] [code DeviceConnectVariable.EQ + hldevice_6108.xml row
    /// order]; the byte space is 1-based (0 = no certified pad), anything
    /// outside 1..=3 unselected.
    #[test]
    fn eq_selection_is_the_apk_pad_order() {
        assert_eq!(eq_selection(3), Some(0), "Equalization (balance)");
        assert_eq!(eq_selection(1), Some(1), "Low (overcast)");
        assert_eq!(eq_selection(2), Some(2), "Bright");
        assert_eq!(eq_selection(0), None, "no certified pad — 1-based space");
        assert_eq!(eq_selection(4), None, "no certified pad");
        assert_eq!(eq_selection(255), None, "no certified pad");
    }

    /// The click path's inverse: every display pad round-trips through the
    /// wire bytes (selection -> wire -> selection is identity), and
    /// unmapped indices stay unmapped.
    #[test]
    fn eq_pad_wire_roundtrips_eq_selection() {
        for (index, wire) in [(0usize, 3u8), (1, 1), (2, 2)] {
            assert_eq!(eq_pad_wire(index), Some(wire), "pad {index}");
            assert_eq!(eq_selection(wire), Some(index), "wire {wire}");
            let mut heart = e1();
            heart.eq_level = wire;
            // The VM's connected shape carries the pad as `selected`; a UI
            // sending `eq_pad_wire(selected)` lands on the same byte.
            let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
            assert_eq!(vm.eq.selected, Some(index), "heart wire {wire}");
            assert_eq!(
                eq_pad_wire(vm.eq.selected.expect("mapped above")),
                Some(wire)
            );
        }
        assert_eq!(eq_pad_wire(3), None, "out of the three-pad range");
        assert_eq!(eq_pad_wire(usize::MAX), None);
    }

    /// The click path's inverse for Volume: display pads 0..=5 map to the
    /// wire bytes 0..=5 — IDENTITY, because the §3 Gain row's write space
    /// is the §5.2 read space (no stop inversion; [wire round trip
    /// 2026-11-10, stop 1/2 on the wire byte-exact]). Unmapped indices
    /// stay unmapped, and a gain write in flight dims ONLY the volume
    /// panel (`applying` per family, selection still the device's).
    #[test]
    fn volume_pad_wire_roundtrips_volume_selection() {
        for wire in 0..=5u8 {
            assert_eq!(
                volume_pad_wire(wire as usize),
                Some(wire),
                "pad {wire} is its own wire byte"
            );
            assert_eq!(volume_selection(wire), Some(wire as usize), "wire {wire}");
            // The VM's connected shape carries the stop as `selected`; a UI
            // sending `volume_pad_wire(selected)` lands on the same byte.
            let mut heart = e1();
            heart.voice_level = wire;
            let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
            assert_eq!(vm.volume.selected, Some(wire as usize), "heart wire {wire}");
            assert_eq!(
                volume_pad_wire(vm.volume.selected.expect("mapped above")),
                Some(wire)
            );
        }
        assert_eq!(volume_pad_wire(6), None, "out of the six-pad space");
        assert_eq!(volume_pad_wire(usize::MAX), None);

        let gain = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Gain(2)),
            None,
        ));
        assert!(gain.volume.applying, "a gain write dims the volume panel");
        assert_eq!(
            gain.volume.selected,
            Some(3),
            "selection stays the device's"
        );
        assert_eq!(gain.nc.pending, None, "NC panel never sees a gain write");
        assert!(!gain.eq.applying, "EQ panel never sees a gain write");
    }

    /// The click path's inverse for Channel mode: display pads 0..=1 map
    /// to the wire bytes 0..=1 — IDENTITY, because the §3 Channel-mode
    /// row's write space is the §5.2 read space (identity write-verified
    /// [wire, 2026-11-11 channel-mode run: the 0→1→0 round trip landed on
    /// the written byte every leg]). Unmapped indices stay unmapped, and
    /// a channel-mode write in flight dims ONLY the channel panel.
    #[test]
    fn channel_mode_pad_wire_roundtrips_channel_selection() {
        for wire in 0..=1u8 {
            assert_eq!(
                channel_mode_pad_wire(wire as usize),
                Some(wire),
                "pad {wire} is its own wire byte"
            );
            assert_eq!(
                channel_mode_selection(wire),
                Some(wire as usize),
                "wire {wire}"
            );
            // The VM's connected shape carries the mode as `selected`; a
            // UI sending `channel_mode_pad_wire(selected)` lands on the
            // same byte.
            let mut heart = e1();
            heart.voice_mode = wire;
            let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
            assert_eq!(
                vm.channel_mode.selected,
                Some(wire as usize),
                "heart wire {wire}"
            );
            assert_eq!(
                channel_mode_pad_wire(vm.channel_mode.selected.expect("mapped above")),
                Some(wire)
            );
        }
        assert_eq!(channel_mode_pad_wire(2), None, "out of the two-pad space");
        assert_eq!(channel_mode_pad_wire(usize::MAX), None);
        // An UNMAPPED heart byte is no selection at all (never a guess):
        let mut heart = e1();
        heart.voice_mode = 2;
        let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
        assert_eq!(
            vm.channel_mode.selected, None,
            "byte 2 has no certified mode"
        );

        let channel = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::ChannelMode(1)),
            None,
        ));
        assert!(
            channel.channel_mode.applying,
            "a channel-mode write dims the channel panel"
        );
        assert_eq!(
            channel.channel_mode.selected,
            Some(0),
            "selection stays the device's (E1 rests at Mono 0)"
        );
        assert_eq!(
            channel.nc.pending, None,
            "NC panel never sees a channel-mode write"
        );
        assert!(!channel.eq.applying, "EQ panel never sees a channel write");
        assert!(
            !channel.volume.applying,
            "Volume panel never sees a channel write"
        );
        assert!(
            !channel.reverb.as_ref().expect("connected").applying,
            "Reverb panel never sees a channel write"
        );
    }

    /// The click path's inverse for Reverb gears: display pads 0..=2 map to
    /// the wire bytes 0..=2 — IDENTITY, because the checklist §3 Reverb
    /// row's write space is the §5.2 read space (identity write-verified
    /// [wire, 2026-11-11 reverb run: the 0→1 gear flip landed while the
    /// switch was on]). Unmapped indices stay unmapped, and the VM's
    /// connected shape carries the CERTIFIED byte only.
    #[test]
    fn reverb_gear_wire_roundtrips_reverb_selection() {
        for wire in 0..=2u8 {
            assert_eq!(
                reverb_gear_wire(wire as usize),
                Some(wire),
                "pad {wire} is its own wire byte"
            );
            assert_eq!(reverb_selection(wire), Some(wire as usize), "wire {wire}");
            let mut heart = e1();
            heart.reverb_level = wire;
            let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
            assert_eq!(vm.reverb.as_ref().expect("connected").gear, Some(wire));
            assert_eq!(
                reverb_gear_wire(
                    vm.reverb
                        .as_ref()
                        .and_then(|r| r.gear)
                        .and_then(reverb_selection)
                        .expect("certified byte maps")
                ),
                Some(wire)
            );
        }
        assert_eq!(reverb_gear_wire(3), None, "out of the three-pad space");
        assert_eq!(reverb_gear_wire(usize::MAX), None);
        // An UNMAPPED heart byte is no selection at all (never a guess):
        let mut heart = e1();
        heart.reverb_level = 3;
        let vm = view_model(&snap(ConnStatus::Connected, Some(heart), None, None));
        assert_eq!(
            vm.reverb.as_ref().expect("connected").gear,
            None,
            "byte 3 has no certified gear"
        );
    }

    /// The toggle's wire pair: the APK's switch handler sends the
    /// remembered `reverbType` on BOTH directions
    /// (HLDevice6302Activity.java:792-793 [code]) — and the 2026-11-11 run
    /// proved the gear byte rides untouched through a toggle [wire]. An
    /// unknown gear falls back to the APK's own default GEAR_ONE = 0
    /// (`:341` [code]).
    #[test]
    fn reverb_toggle_wire_sends_the_remembered_gear() {
        assert_eq!(
            reverb_toggle_wire(true, Some(0)),
            (true, 0),
            "the run's toggle leg: switch on, starting gear rides [wire]"
        );
        assert_eq!(
            reverb_toggle_wire(false, Some(1)),
            (false, 1),
            "restore = the full pair, gear remembered [wire frame 46]"
        );
        assert_eq!(
            reverb_toggle_wire(true, Some(2)),
            (true, 2),
            "a certified gear always passes through"
        );
        assert_eq!(
            reverb_toggle_wire(true, None),
            (true, 0),
            "unknown gear falls back to GEAR_ONE(0) [code :341]"
        );
        assert_eq!(reverb_toggle_wire(false, None), (false, 0));
    }

    /// Panels gate on their OWN pending only: an in-flight reverb write
    /// (`PendingWrite::Reverb`) shows `applying` on the reverb panel while
    /// every other panel stays untouched. (`WRITABLE.reverb` is pinned
    /// separately by `writable_table_matches_the_certified_chain`.)
    #[test]
    fn reverb_applying_is_family_scoped_and_writable() {
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Reverb(true, 2)),
            None,
        ));
        let reverb = vm.reverb.expect("connected");
        assert!(reverb.applying, "a reverb write dims the reverb panel");
        assert_eq!(
            (reverb.enabled, reverb.gear),
            (Some(false), Some(2)),
            "selection stays the device's (no pending target shown)"
        );
        assert_eq!(vm.nc.pending, None, "NC panel never sees a reverb write");
        assert!(!vm.eq.applying, "EQ panel never sees a reverb write");
        assert!(
            !vm.volume.applying,
            "Volume panel never sees a reverb write"
        );
    }

    /// Panels gate on their OWN pending only: an in-flight EQ write
    /// (`PendingWrite::Eq`) shows `applying` on the EQ panel while the NC
    /// panel stays untouched (pending None), and the reverse.
    #[test]
    fn pending_is_filtered_by_control_family() {
        let eq_vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Eq(2)),
            None,
        ));
        assert!(eq_vm.eq.applying, "EQ write dims the EQ panel");
        assert_eq!(eq_vm.eq.selected, Some(0), "selection stays the device's");
        assert_eq!(eq_vm.nc.pending, None, "NC panel never sees an EQ write");

        let nc_vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Noise(NoiseWrite::Level(1))),
            None,
        ));
        assert_eq!(nc_vm.nc.pending, Some(NoiseWrite::Level(1)));
        assert_eq!(
            nc_vm.eq,
            EqVm {
                selected: Some(0),
                applying: false
            },
            "NC write never dims the EQ panel"
        );
    }

    /// Reverb pads: Small/Medium/Large = wire bytes 0/1/2 (identity) [code
    /// DeviceConnectVariable.Reverb + gear strings]; anything else unselected.
    #[test]
    fn reverb_selection_is_the_apk_identity() {
        assert_eq!(reverb_selection(0), Some(0), "Small");
        assert_eq!(reverb_selection(1), Some(1), "Medium");
        assert_eq!(reverb_selection(2), Some(2), "Large");
        assert_eq!(reverb_selection(3), None, "no certified pad");
        assert_eq!(reverb_selection(255), None, "no certified pad");
    }

    /// Mic-card → wire-slot translation: the design's Mic 1/Mic 2 card
    /// indices map identity onto the wire's TX1/TX2 slots [code the APK's
    /// `ordinal() - 1` clamp, TX1 first]; there are exactly two mic cards,
    /// so anything beyond has no card and no write.
    #[test]
    fn tx_mute_wire_is_the_two_card_identity() {
        assert_eq!(tx_mute_wire(0), Some(0), "Mic 1 = wire slot 0");
        assert_eq!(tx_mute_wire(1), Some(1), "Mic 2 = wire slot 1");
        assert_eq!(tx_mute_wire(2), None, "no mic card beyond the two panels");
        assert_eq!(tx_mute_wire(255), None);
    }

    /// A TX-mute write pends ONLY the panel whose slot it addresses (per-
    /// slot scoping, one TX per frame): a Mic 1 write dims tx1 while tx2
    /// stays untouched — and no other family's write dims any mic card.
    #[test]
    fn tx_mute_applying_is_slot_scoped() {
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::TxMute { tx: 0, muted: true }),
            None,
        ));
        let tx1 = vm.tx1.expect("tx1 panel exists");
        assert!(tx1.applying, "a Mic 1 write dims the Mic 1 panel");
        assert!(!tx1.muted, "state stays the device's");
        assert!(
            !vm.tx2.expect("tx2 panel exists").applying,
            "Mic 2 never sees a Mic 1 write"
        );

        // A slot-1 write scopes the other way.
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::TxMute { tx: 1, muted: true }),
            None,
        ));
        assert!(!vm.tx1.expect("tx1").applying, "Mic 1 stays live");
        assert!(vm.tx2.expect("tx2").applying, "Mic 2 dims");

        // Another family's write: no mic card dims, NC never sees it.
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(PendingWrite::Eq(2)),
            None,
        ));
        assert!(!vm.tx1.expect("tx1").applying);
        assert!(!vm.tx2.expect("tx2").applying);
        assert_eq!(vm.nc.pending, None, "EQ write never pends the NC panel");
    }

    /// Battery segments: ~20 % per segment, ceil, 0 unlit (presentation-
    /// only thresholds — boundary sweep).
    #[test]
    fn battery_segment_count_boundaries() {
        for (pct, want) in [
            (0u8, 0),
            (1, 1),
            (19, 1),
            (20, 1),
            (21, 2),
            (40, 2),
            (60, 3),
            (80, 4),
            (93, 5),
            (98, 5),
            (100, 5),
        ] {
            assert_eq!(battery_segment_count(pct), want, "battery {pct}%");
        }
    }

    /// The writability table is a fixed contract: exactly the
    /// wire-certified SET paths are true, everything else read-only until
    /// its capture lands (docs/gui-roadmap.md §1 gate). EQ joined via its
    /// PC-originated guarded round trip ([wire] 2026-11-09;
    /// captures/set_eq_roundtrip.jsonl); Volume joined the same route
    /// (b) on 2026-11-10 (captures/set_gain_roundtrip.jsonl); Reverb on
    /// 2026-11-11 (captures/set_reverb_roundtrip.jsonl); Channel mode on
    /// 2026-11-11 (captures/set_channel_mode_roundtrip.jsonl).
    #[test]
    fn writable_table_matches_the_certified_chain() {
        let all_read_only = Writable {
            nc_toggle: false,
            nc_level: false,
            mic_mute: false,
            volume: false,
            channel_mode: false,
            eq: false,
            reverb: false,
            mic_recognition: false,
            schedule_power_off: false,
            indicator_light: false,
        };
        assert_eq!(
            WRITABLE,
            Writable {
                nc_toggle: true,
                nc_level: true,
                eq: true,
                volume: true,
                reverb: true,
                channel_mode: true,
                ..all_read_only
            }
        );
    }

    /// NC status word: pending beats the state (the panel keeps showing the
    /// previous device state while ANY SET is in flight); no verified state
    /// is "Unavailable" (the busy-banner case renders that way too).
    #[test]
    fn nc_status_word_matrix() {
        let nc = |state: Option<bool>, pending: Option<NoiseWrite>| NcVm {
            state,
            level: Some(0),
            pending,
            last_error: None,
        };
        assert_eq!(nc_status(&nc(None, None)), "Unavailable");
        assert_eq!(nc_status(&nc(Some(false), None)), "Off");
        assert_eq!(nc_status(&nc(Some(true), None)), "On");
        // A SWITCH in flight says Applying…
        assert_eq!(
            nc_status(&nc(Some(true), Some(NoiseWrite::Switch(false)))),
            "Applying"
        );
        // …and so does a LEVEL write (one status word for the panel).
        assert_eq!(
            nc_status(&nc(Some(true), Some(NoiseWrite::Level(2)))),
            "Applying"
        );
    }

    /// Short connection words: banner-backed states borrow the banner word,
    /// connecting says Searching, and a live heart names itself — the same
    /// fn feeds the banner title slot and the footer LED text.
    #[test]
    fn conn_status_word_matrix() {
        let mut vmblank = view_model(&snap(ConnStatus::Connecting, None, None, None));
        assert_eq!(conn_status_word(&vmblank), "Searching");

        vmblank = view_model(&snap(ConnStatus::Busy, None, None, None));
        assert_eq!(conn_status_word(&vmblank), "Receiver in use");
        vmblank = view_model(&snap(ConnStatus::PermissionDenied, None, None, None));
        assert_eq!(conn_status_word(&vmblank), "Access blocked");
        vmblank = view_model(&snap(ConnStatus::NotFound, None, None, None));
        assert_eq!(conn_status_word(&vmblank), "No receiver");
        vmblank = view_model(&snap(
            ConnStatus::Error,
            None,
            None,
            Some("malformed device reply: …"),
        ));
        assert_eq!(conn_status_word(&vmblank), "Device error");

        let connected = view_model(&snap(ConnStatus::Connected, Some(e1()), None, None));
        assert_eq!(conn_status_word(&connected), "Receiver connected");
    }

    /// Indicator light passthrough: header field semantics (offset 11,
    /// inverted byte) surface straight to the view; unknown without a heart.
    #[test]
    fn light_passthrough() {
        let vm = view_model(&snap(ConnStatus::Connecting, None, None, None));
        assert_eq!(vm.light, None);

        // E1: light byte 0 -> light_on (inverted) -> Some(true). The demo
        // payloads and both live captures carry the same semantics.
        let vm = view_model(&snap(ConnStatus::Connected, Some(e1()), None, None));
        assert_eq!(vm.light, Some(true));
    }

    /// Device name: the const is the single source (views must not carry a
    /// second copy); the test pins the wording so casing drift never ships.
    #[test]
    fn device_name_is_pinned() {
        assert_eq!(DEVICE_NAME, "Lark A1");
    }

    /// Footer device id pinned the same way as the device name (one source
    /// in the VM, no view-side literal).
    #[test]
    fn vid_pid_is_the_certified_pair() {
        assert_eq!(VID_PID, "3547:0407");
    }
}
