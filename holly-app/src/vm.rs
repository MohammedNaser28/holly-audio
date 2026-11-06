//! `Snapshot -> ViewModel`: the ONLY device-aware translation a frontend
//! needs. Pure functions; no I/O, not even deps beyond this crate.
//!
//! Frontends render [`ViewModel`] as-is. All knowledge of what a connection
//! state means (banner text incl. what to tell the user about
//! `packaging/70-hollyland.rules` or another process holding the claim)
//! lives HERE, in string form, so UI code never hardcodes device rules.

use serde::Serialize;

use crate::actor::NoiseWrite;
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
/// - everything else is [code] at best (mic mute cmd 22, light cmd 33,
///   reverb 5, eq 7, shutdown 39/40 …) and has never been captured, so it
///   stays read-only today.
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
    volume: false,
    channel_mode: false,
    eq: false,
    reverb: false,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TxVm {
    pub connected: bool,
    /// Percent 0..=100 (HeartInfo, §5 of docs/protocol.md).
    pub battery: u8,
    pub battery_class: BatteryClass,
    pub muted: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReverbVm {
    pub enabled: bool,
    pub level: u8,
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
    /// Gain (§5 field `voiceLevel`).
    pub voice_level: Option<u8>,
    /// EQ level (§5 field `eqLevel`).
    pub eq_level: Option<u8>,
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
            voice_level: None,
            eq_level: None,
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
                tx1: Some(tx_vm(heart.tx1_status, heart.tx1_battery, heart.tx1_mute)),
                tx2: Some(tx_vm(heart.tx2_status, heart.tx2_battery, heart.tx2_mute)),
                tx3: tail_tx(heart.tx3_status, heart.tx3_battery, heart.tx3_mute),
                tx4: tail_tx(heart.tx4_status, heart.tx4_battery, heart.tx4_mute),
                nc: NcVm {
                    state: Some(heart.noise_switch),
                    level: Some(heart.noise_level),
                    pending: s.pending_set,
                    last_error: None,
                },
                light: Some(heart.light_on),
                voice_level: Some(heart.voice_level),
                eq_level: Some(heart.eq_level),
                reverb: Some(ReverbVm {
                    enabled: heart.reverb_switch,
                    level: heart.reverb_level,
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
        voice_level: None,
        eq_level: None,
        reverb: None,
        updated: None,
    }
}

fn tx_vm(connected: bool, battery: u8, muted: bool) -> TxVm {
    TxVm {
        connected,
        battery,
        battery_class: battery_class(battery),
        muted,
    }
}

fn tail_tx(status: Option<bool>, battery: Option<u8>, mute: Option<bool>) -> Option<TxVm> {
    // All three fields appear at their own offsets together [code
    // ProGetHearInfo.java:100-123]; if any is None they are None together.
    match (status, battery, mute) {
        (Some(st), Some(bat), Some(m)) => Some(tx_vm(st, bat, m)),
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
        pending: Option<NoiseWrite>,
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
            Some(NoiseWrite::Switch(false)),
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
            Some(NoiseWrite::Switch(true)),
            None,
        ));
        assert_eq!(vm.nc.state, Some(false));
        assert_eq!(vm.nc.pending, Some(NoiseWrite::Switch(true)));

        // Level write pending with switch-on heart: state stays the device's,
        // the pending carries the target byte only.
        let vm = view_model(&snap(
            ConnStatus::Connected,
            Some(e1()),
            Some(NoiseWrite::Level(2)),
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
        assert_eq!(vm.voice_level, Some(3));
        assert_eq!(vm.eq_level, Some(3));
        assert_eq!(
            vm.reverb,
            Some(ReverbVm {
                enabled: false,
                level: 2
            })
        );
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

    /// The writability table is a fixed contract: exactly the two
    /// wire-certified SET paths are true, everything else read-only until
    /// its capture lands (docs/gui-roadmap.md §1 gate).
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
}
