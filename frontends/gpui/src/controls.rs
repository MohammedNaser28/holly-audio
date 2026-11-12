//! Pure per-control decisions (pure move out of main.rs, commit B): the
//! pick/NOOP/write gates and three-state pick enums for every multi-choice
//! control, the banner palette, the LED decisions and the battery readout
//! digits — all decided ONLY from `holly_app::vm` fields, unit-tested below.
//!
//! The owner rule for these decisions: they are the ONLY place pick
//! semantics live, shared by mouse and keyboard.

use holly_app::vm::{
    Banner, ChannelModeVm, EqVm, GainVm, LightVm, NcVm, ReverbVm, ScheduleVm, TxVm, ViewModel,
    WRITABLE, mic_recognition_allowed,
};

use crate::theme;
use crate::widgets::LedLit;

/// Battery readout digits (reference: `{{m.battery}}` = "100" | "--"): a
/// live transmitter shows its percent; ANYTHING else is "--" — a decoded
/// zero byte on a dead link is device padding, never "0 %".
pub(crate) fn battery_digits(tx: &TxVm) -> String {
    if tx.connected {
        tx.battery.to_string()
    } else {
        "--".to_string()
    }
}

/// What a click or a 1/2/3 press on level L may do right now (the ONLY
/// place pick semantics live, shared by mouse and keyboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LevelPick {
    /// Already the device's level — nothing to write (no duplicate SETs).
    Noop,
    /// A level SET goes out.
    Write,
    /// Blocked: while ANY SET is in flight picks are ignored (pending
    /// latest-wins; no second queue), and a level pick with the switch
    /// verified OFF never writes (level-while-off was never sent/observed
    /// on the wire — the APK always sends `isOpen=true` with its level
    /// presses).
    Blocked,
}

pub(crate) fn level_pick(level: u8, nc: &NcVm) -> LevelPick {
    if nc.pending.is_some() {
        return LevelPick::Blocked;
    }
    if nc.state != Some(true) {
        return LevelPick::Blocked;
    }
    if nc.level == Some(level) {
        return LevelPick::Noop;
    }
    LevelPick::Write
}

/// May THIS NC request go out right now? Guard for the rocker flip and the
/// rocker pads: allowed only with a verified state and NOTHING in flight
/// (any pending SET — switch OR level — blocks picks; while pending the
/// actor is busy and the UI must not queue a second write; the actor's own
/// latest-wins coalescing stays the ONLY queue).
pub(crate) fn nc_write_accepted(nc: &NcVm) -> bool {
    nc.pending.is_none() && nc.state.is_some()
}

/// May THIS EQ request go out right now? Guard for the EQ pads: the family
/// must be writable (`WRITABLE.eq` — true only since the guarded round trip;
/// if a later regression demoted it, the click path goes inert WITHOUT
/// re-touching every view) and nothing may be in flight (ONE SET runs at a
/// time; any pending — NC or EQ — blocks: the actor's latest-wins
/// coalescing stays the ONLY queue, the UI never queues a second write).
pub(crate) fn eq_write_accepted(eq: &EqVm) -> bool {
    WRITABLE.eq && !eq.applying
}

/// Decision for ONE EQ pad pick — parallel to [`LevelPick`]: blocked (the
/// family is read-only or an EQ write is in flight), noop (that pad is
/// already the device's verified selection), else write. NO pending target
/// is ever displayed — the selection moves on the device's verdict only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EqPick {
    Blocked,
    Noop,
    Write,
}

pub(crate) fn eq_pick(index: usize, eq: &EqVm) -> EqPick {
    if !eq_write_accepted(eq) {
        return EqPick::Blocked;
    }
    if eq.selected == Some(index) {
        return EqPick::Noop;
    }
    EqPick::Write
}

/// Write gate for the Volume panel: the certified family AND nothing in
/// flight — the same conjunction as [`eq_write_accepted`] (the
/// `WRITABLE.volume` half is vm.rs's pinned table; the `applying` half
/// comes from the actor-reported `GainVm`).
pub(crate) fn volume_write_accepted(volume: &GainVm) -> bool {
    WRITABLE.volume && !volume.applying
}

/// Decision for ONE Volume-stop pick — parallel to [`EqPick`]: blocked (the
/// family is read-only or a gain write is in flight), noop (that stop is
/// already the device's verified selection), else write. NO pending target
/// is ever displayed — the selection moves on the device's verdict only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumePick {
    Blocked,
    Noop,
    Write,
}

pub(crate) fn volume_pick(index: usize, volume: &GainVm) -> VolumePick {
    if !volume_write_accepted(volume) {
        return VolumePick::Blocked;
    }
    if volume.selected == Some(index) {
        return VolumePick::Noop;
    }
    VolumePick::Write
}

/// Write gate for the Reverb panel: the certified family AND nothing in
/// flight — the same conjunction as [`eq_write_accepted`] (the
/// `WRITABLE.reverb` half is vm.rs's pinned table; the `applying` half
/// comes from the actor-reported `ReverbVm`).
pub(crate) fn reverb_write_accepted(reverb: &ReverbVm) -> bool {
    WRITABLE.reverb && !reverb.applying
}

/// May THIS reverb toggle go out right now? Guard for the switch flip: the
/// family gate plus a VERIFIED switch state — the flip target is
/// `!enabled`, and an unknown state never invents one (same rule as
/// [`nc_write_accepted`]). The device's current gear rides along in the
/// handler.
pub(crate) fn reverb_toggle_accepted(reverb: &ReverbVm) -> bool {
    reverb_write_accepted(reverb) && reverb.enabled.is_some()
}

/// May THIS light toggle go out right now? Guard for the switch flip: the
/// family gate plus a VERIFIED switch state — the flip target is `!on`,
/// and an unknown state never invents one (same rule as
/// [`reverb_toggle_accepted`]). The family gate's `WRITABLE` half is
/// vm.rs's pinned table (certified 2026-11-12), the `applying` half comes
/// from the actor-reported `LightVm`.
pub(crate) fn light_toggle_accepted(light: &LightVm) -> bool {
    WRITABLE.indicator_light && !light.applying && light.on.is_some()
}

/// May THIS mic-recognition toggle go out right now? Guard for the switch
/// flip: the family gate (`WRITABLE.mic_recognition` — true only since the
/// 2026-11-12 round trip), nothing in flight, a VERIFIED switch state, and
/// BOTH transmitters verified connected — the APK's own precondition for
/// this toggle ([`mic_recognition_allowed`], the
/// HLDevice6302Activity.java:805-812 rule the VM carries). The gate reads
/// the whole [`ViewModel`] because that precondition spans the mic cards,
/// not the toggle's own slice.
pub(crate) fn mic_toggle_accepted(vm: &ViewModel) -> bool {
    WRITABLE.mic_recognition
        && !vm.mic_recognition.applying
        && vm.mic_recognition.on.is_some()
        && mic_recognition_allowed(
            vm.tx1.is_some_and(|t| t.connected),
            vm.tx2.is_some_and(|t| t.connected),
        )
}

/// Decision for ONE reverb gear pick — parallel to [`EqPick`]: blocked (the
/// family is read-only, a reverb write is in flight, or the switch state is
/// unverified), noop (that gear is already the device's verified selection
/// while ON), else write. A pick with the switch verified OFF still WRITES:
/// the APK's gear buttons send `isOpen=true` unconditionally
/// (HLDevice6302Activity.java:759/:769/:779 [code]), so the pick turns
/// reverb on with that gear — [wire] the 2026-11-11 round trip's SET legs
/// all carried explicit `[isOpen, gear]` pairs. NO pending target is ever
/// displayed — the selection moves on the device's verdict only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReverbPick {
    Blocked,
    Noop,
    Write,
}

pub(crate) fn reverb_pick(index: usize, reverb: &ReverbVm) -> ReverbPick {
    if !reverb_write_accepted(reverb) || reverb.enabled.is_none() {
        return ReverbPick::Blocked;
    }
    if reverb.enabled == Some(true) && reverb.gear == Some(index as u8) {
        return ReverbPick::Noop;
    }
    ReverbPick::Write
}

/// Write gate for the Channel-mode panel: the certified family AND nothing
/// in flight — the same conjunction as [`eq_write_accepted`] (the
/// `WRITABLE.channel_mode` half is vm.rs's pinned table; the `applying`
/// half comes from the actor-reported `ChannelModeVm`).
pub(crate) fn channel_write_accepted(channel: &ChannelModeVm) -> bool {
    WRITABLE.channel_mode && !channel.applying
}

/// Decision for ONE Channel-mode pick — parallel to [`EqPick`]: blocked
/// (the family is read-only or a channel-mode write is in flight), noop
/// (that mode is already the device's verified selection), else write. NO
/// pending target is ever displayed — the selection moves on the device's
/// verdict only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelPick {
    Blocked,
    Noop,
    Write,
}

pub(crate) fn channel_pick(index: usize, channel: &ChannelModeVm) -> ChannelPick {
    if !channel_write_accepted(channel) {
        return ChannelPick::Blocked;
    }
    // Stereo needs BOTH mics verified connected (the VM's `stereo_ok` —
    // owner rule 2026-11-12, stricter than the APK's any-mic gate; the
    // full evidence note lives on vm.rs `stereo_allowed`). Mono is never
    // gated by it. The gate fires BEFORE the noop check: with a mic
    // down, a click on the held Stereo pad is blocked, not a noop.
    if index == 1 && !channel.stereo_ok {
        return ChannelPick::Blocked;
    }
    if channel.selected == Some(index) {
        return ChannelPick::Noop;
    }
    ChannelPick::Write
}

/// Write gate for the Schedule panel: the certified family AND nothing in
/// flight — the same conjunction as [`channel_write_accepted`] (the
/// `WRITABLE.schedule_power_off` half is vm.rs's pinned table; the
/// `applying` half comes from the actor-reported `ScheduleVm`).
pub(crate) fn schedule_write_accepted(schedule: &ScheduleVm) -> bool {
    WRITABLE.schedule_power_off && !schedule.applying
}

/// Decision for ONE Schedule pick — parallel to [`ChannelPick`]: blocked
/// (the family is read-only or a shutdown-timer write is in flight), noop
/// (that value is already the device's verified selection), else write.
/// NO pending target is ever displayed — the selection moves on the
/// device's verdict only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulePick {
    Blocked,
    Noop,
    Write,
}

pub(crate) fn schedule_pick(index: usize, schedule: &ScheduleVm) -> SchedulePick {
    if !schedule_write_accepted(schedule) {
        return SchedulePick::Blocked;
    }
    if schedule.selected == Some(index) {
        return SchedulePick::Noop;
    }
    SchedulePick::Write
}

/// Write gate for ONE mic card's mute chip — the three-part conjunction
/// [`reverb_toggle_accepted`] uses, plus the connectedness half the mute
/// family needs: the certified family (`WRITABLE.mic_mute` — flipped true
/// on the 2026-11-11 TX1-slot round trip, captures/
/// set_tx_mute_roundtrip.jsonl; the TX2 slot's write stays gated), the TX
/// CONNECTED (the APK refuses offline mutes with a toast, no write [code
/// `HLDevice6302Activity.java:668/:682`]), and nothing in flight for THAT
/// card (per-slot `applying`; ONE SET runs at a time — any other family's
/// pending blocks too: the actor's latest-wins coalescing stays the ONLY
/// queue, the UI never queues a second write).
pub(crate) fn mic_mute_write_accepted(tx: &TxVm) -> bool {
    WRITABLE.mic_mute && tx.connected && !tx.applying
}

/// Banner decoration per kind, decided ONLY from `Banner` (the design
/// colour rule: amber = needs attention, red = error, grey = everything
/// else; container stays on the designed BANNER_BG/BANNER_LINE). The
/// reference depicts only the busy banner; error/not-found treatments are
/// token compositions of that rule (README documents them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BannerPalette {
    pub(crate) accent: u32,
    pub(crate) title: u32,
    pub(crate) body: u32,
}

pub(crate) fn banner_palette(banner: &Banner) -> BannerPalette {
    match banner {
        Banner::Error => BannerPalette {
            accent: theme::color::ALERT,
            title: theme::color::ALERT,
            body: theme::color::BANNER_TEXT,
        },
        Banner::PermissionDenied | Banner::Busy => BannerPalette {
            accent: theme::color::TUNGSTEN,
            title: theme::color::TEXT,
            body: theme::color::BANNER_TEXT,
        },
        Banner::NotFound => BannerPalette {
            accent: theme::color::TEXT_3,
            title: theme::color::TEXT_2,
            body: theme::color::TEXT_3,
        },
    }
}

/// Footer LED decision (VM-only): red on error; amber (attention) on the
/// busy/denied banners and while connecting; green on a live connection;
/// the grey ring otherwise (not found).
pub(crate) fn vm_led(vm: &ViewModel) -> LedLit {
    match &vm.banner {
        Some(Banner::Error) => LedLit::Alert,
        Some(Banner::PermissionDenied | Banner::Busy) => LedLit::Attention { pulse: false },
        Some(Banner::NotFound) => LedLit::Ring,
        None if vm.banner_text.is_some() => LedLit::Attention { pulse: false },
        None => LedLit::Live,
    }
}

/// The NC panel's LED: tungsten while ON (solid) and while a write is in
/// flight (pulsing); the grey ring otherwise (OFF/unknown — per the
/// colour rule grey means "everything else").
pub(crate) fn nc_led(nc: &NcVm) -> LedLit {
    if nc.pending.is_some() {
        LedLit::Attention { pulse: true }
    } else if nc.state == Some(true) {
        LedLit::Attention { pulse: false }
    } else {
        LedLit::Ring
    }
}
#[cfg(test)]
mod tests {
    use super::{
        EqPick, LedLit, LevelPick, banner_palette, battery_digits, eq_pick, eq_write_accepted,
        level_pick, nc_led, nc_write_accepted, vm_led,
    };
    use holly_app::NoiseWrite;
    use holly_app::vm::{
        Banner, ChannelModeVm, EqVm, GainVm, LightVm, MicRecognitionVm, NcVm, ScheduleVm, TxVm,
        ViewModel, battery_class,
    };

    /// A rocker/VM shape straight from the VM fields.
    fn nc(level: Option<u8>, state: Option<bool>, pending: Option<NoiseWrite>) -> NcVm {
        NcVm {
            state,
            level,
            pending,
            last_error: None,
        }
    }

    /// An EQ panel shape straight from the VM fields.
    fn eq(selected: Option<usize>, applying: bool) -> EqVm {
        EqVm { selected, applying }
    }

    fn tx(connected: bool, battery: u8) -> TxVm {
        TxVm {
            connected,
            battery,
            battery_class: battery_class(battery),
            muted: false,
            applying: false,
        }
    }

    fn blank_vm() -> ViewModel {
        ViewModel {
            banner: None,
            banner_text: None,
            tx1: None,
            tx2: None,
            tx3: None,
            tx4: None,
            nc: nc(None, None, None),
            light: LightVm {
                on: None,
                applying: false,
            },
            volume: GainVm {
                selected: None,
                applying: false,
            },
            eq: eq(None, false),
            mic_recognition: MicRecognitionVm {
                on: None,
                applying: false,
            },
            channel_mode: ChannelModeVm {
                selected: None,
                stereo_ok: false,
                applying: false,
            },
            shutdown: ScheduleVm {
                selected: None,
                applying: false,
            },
            signal_mode: None,
            reverb: None,
            updated: None,
        }
    }

    /// Battery digits: the reference's `{{m.battery}}` = "100" | "--". A
    /// live transmitter shows its percent; ANYTHING on a dead link is "--" —
    /// even a decoded 0 byte, which is device padding, never "0 %".
    #[test]
    fn battery_digits_live_shows_percent_dead_shows_placeholder() {
        assert_eq!(battery_digits(&tx(true, 93)), "93");
        assert_eq!(battery_digits(&tx(true, 0)), "0");
        assert_eq!(battery_digits(&tx(false, 93)), "--");
        assert_eq!(battery_digits(&tx(false, 0)), "--");
    }

    /// The level pick: blocked while ANY SET is in flight, blocked while the
    /// switch is not verified ON (level-while-off was never sent/observed),
    /// noop on the device's own level, write otherwise.
    #[test]
    fn level_pick_matrix() {
        // Switch not verified on: blocked, pending or not.
        for state in [None, Some(false)] {
            assert_eq!(level_pick(2, &nc(None, state, None)), LevelPick::Blocked);
            assert_eq!(
                level_pick(2, &nc(Some(1), state, Some(NoiseWrite::Switch(true)))),
                LevelPick::Blocked
            );
        }
        // Switch on, device at level 1.
        let on = nc(Some(1), Some(true), None);
        assert_eq!(level_pick(1, &on), LevelPick::Noop);
        assert_eq!(level_pick(0, &on), LevelPick::Write);
        assert_eq!(level_pick(2, &on), LevelPick::Write);
        // ANY pending write blocks (switch in flight ALSO blocks levels).
        assert_eq!(
            level_pick(0, &nc(Some(0), Some(true), Some(NoiseWrite::Switch(false)))),
            LevelPick::Blocked
        );
        assert_eq!(
            level_pick(2, &nc(Some(0), Some(true), Some(NoiseWrite::Level(2)))),
            LevelPick::Blocked
        );
    }

    /// Write gate: allowed only with a verified state and NOTHING in flight.
    #[test]
    fn nc_write_accepted_matrix() {
        // No verified state: never (a request without a known state cannot
        // be deduped against reality).
        assert!(!nc_write_accepted(&nc(None, None, None)));
        // Verified state, nothing in flight: accepted.
        assert!(nc_write_accepted(&nc(Some(0), Some(true), None)));
        assert!(nc_write_accepted(&nc(Some(1), Some(false), None)));
        // ANY pending SET, either byte: rejected.
        assert!(!nc_write_accepted(&nc(
            Some(1),
            Some(true),
            Some(NoiseWrite::Switch(true))
        )));
        assert!(!nc_write_accepted(&nc(
            Some(1),
            Some(true),
            Some(NoiseWrite::Level(1))
        )));
    }

    /// EQ write gate: writable family AND nothing in flight. A demoted
    /// family is unreachable with today's `WRITABLE.eq = true` (the pinned
    /// contract is vm.rs's own table test); the conjunction's `applying`
    /// half is exercised to full coverage here.
    #[test]
    fn eq_write_accepted_matrix() {
        // Verified selection, nothing in flight: accepted.
        assert!(eq_write_accepted(&eq(Some(0), false)));
        // Unknown selection (device at an uncertified byte): still accepted —
        // a click may move ONTO a certified pad.
        assert!(eq_write_accepted(&eq(None, false)));
        // An EQ write in flight: blocked regardless of selection.
        assert!(!eq_write_accepted(&eq(Some(0), true)));
        assert!(!eq_write_accepted(&eq(None, true)));
    }

    /// EQ pick decision: same matric family as `level_pick` — blocked while
    /// applying (or an unwritable family), noop on the device's own pad,
    /// write otherwise. NO pending target is ever displayed.
    #[test]
    fn eq_pick_matrix() {
        let idle = eq(Some(1), false);
        assert_eq!(eq_pick(1, &idle), EqPick::Noop, "the device's own pad");
        assert_eq!(eq_pick(0, &idle), EqPick::Write);
        assert_eq!(eq_pick(2, &idle), EqPick::Write);
        assert_eq!(
            eq_pick(1, &eq(Some(1), true)),
            EqPick::Blocked,
            "an in-flight EQ write blocks every pick"
        );
        assert_eq!(eq_pick(0, &eq(None, false)), EqPick::Write);
    }

    /// Banner palette per kind (colour rule: amber = attention, red = error,
    /// grey = everything else; container colors come from the tokens).
    #[test]
    fn banner_palette_kinds() {
        let err = banner_palette(&Banner::Error);
        assert_eq!(err.accent, crate::theme::color::ALERT);
        assert_eq!(err.title, crate::theme::color::ALERT);

        let attention = banner_palette(&Banner::Busy);
        assert_eq!(attention.accent, crate::theme::color::TUNGSTEN);
        let denied = banner_palette(&Banner::PermissionDenied);
        assert_eq!(denied.accent, crate::theme::color::TUNGSTEN);

        let lost = banner_palette(&Banner::NotFound);
        assert_eq!(lost.accent, crate::theme::color::TEXT_3);
    }

    /// Footer LED decided only from the VM: alert on error, amber on the
    /// attention banners and while connecting, grey ring when not found,
    /// green when live (no banner, no searching text).
    #[test]
    fn vm_led_matrix() {
        let mut vm = blank_vm();
        vm.banner = Some(Banner::Error);
        assert_eq!(vm_led(&vm), LedLit::Alert);

        for banner in [Banner::Busy, Banner::PermissionDenied] {
            vm.banner = Some(banner);
            assert_eq!(vm_led(&vm), LedLit::Attention { pulse: false });
        }

        vm.banner = Some(Banner::NotFound);
        assert_eq!(vm_led(&vm), LedLit::Ring);

        vm.banner = None;
        vm.banner_text = Some("Searching for the dongle…".to_string());
        assert_eq!(vm_led(&vm), LedLit::Attention { pulse: false });

        vm.banner_text = None;
        assert_eq!(vm_led(&vm), LedLit::Live);
    }

    /// NC panel LED decided only from the VM shape: pulsing amber while a
    /// write is in flight (the actor-reported pending), solid amber while ON,
    /// grey ring otherwise.
    #[test]
    fn nc_led_matrix() {
        let pending = nc(None, Some(true), Some(NoiseWrite::Switch(false)));
        assert_eq!(nc_led(&pending), LedLit::Attention { pulse: true });

        let on = nc(Some(1), Some(true), None);
        assert_eq!(nc_led(&on), LedLit::Attention { pulse: false });

        for state in [None, Some(false)] {
            assert_eq!(nc_led(&nc(None, state, None)), LedLit::Ring);
        }
    }
}
