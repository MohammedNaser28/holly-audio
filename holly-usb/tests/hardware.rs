//! Hardware tests — compile everywhere, run only with `cargo test
//! --test hardware -- --ignored` (and the dongle plugged in, udev rule
//! installed, QEMU VM not holding the device).
//!
//! `open_and_read_heart_info` is read-only (GET_HEART_INFO only). The
//! Task-4b `set_noise_roundtrip_guarded` DOES exercise SET_NOISE on
//! hardware, guarded: it restores the original state on every exit path.
//! Phase 1B adds `set_eq_roundtrip_guarded` (roadmap §1 route (b)): the
//! doc-recomputed SET_EQ frame round-trips on hardware under the same
//! restore-on-every-exit-path guarantee; its rules (ACK-failure probe +
//! single restore, fixed settle poll, neighbouring-value target, owner-
//! gated usbmon capture) are spelled out in docs/gui-roadmap.md §1(b).
//! `set_gain_roundtrip_guarded` (2026-11-10) applies the SAME rules to the
//! gain control — cmd 3 SET_VOICE_LEVEL, "Volume adjustment" — with the
//! target bound to the neighbouring stop BELOW the current one (quieter).
//! `set_reverb_roundtrip_guarded` applies them to reverb (cmd 5
//! SET_REVERB, two-byte payload [isOpen, gear]): when the switch rests
//! OFF it first toggles ON with the starting gear and then moves to the
//! neighbouring gear while on; when it rests ON it only moves the
//! neighbouring gear and NEVER switches off. Reverb is AUDIBLE on the
//! mics — the owner keeps recording/monitoring off for the run.
//! `set_channel_mode_roundtrip_guarded` applies them to channel mode
//! (cmd 1 SET_VOICE_MODE, Mono=0/Stereo=1) — the one control that can
//! re-enumerate the dongle (the mode change re-presents the audio path):
//! every transport failure inside the settle window triggers a reopen by
//! VID:PID, the restore guard reopens too, and an unreturned device
//! aborts with the restore NOT verified. The owner keeps recording,
//! calls, music and monitoring closed for the run.
//! `set_shutdown_time_roundtrip_guarded` applies them to the auto-power-off
//! timer (cmd 39, values ONLY — AGENTS.md lift): the armed phase is held
//! ~2 s and the restore re-arms the countdown (expected, printed).
//! `set_light_roundtrip_guarded` applies them to the indicator light
//! (cmd 33, INVERTED value space 0 = on / 1 = off on both APK sides): the
//! lowest-risk control in the batch — a visible LED only, no audio effect,
//! no re-enumeration — so the owner's only extra duty is confirming
//! VISUALLY that the LED changes during the brief off phase and comes
//! back (a manual observation, reported as such, never evidence).
//! `set_tx_identify_roundtrip_guarded` applies them to mic recognition
//! (cmd 35 SET_TX_IDENTIFY, IDENTITY value space OFF=0/ON=1 — owner lift
//! 2026-11-12, AGENTS.md): the one control in the batch with a hard START
//! gate of its own — the run refuses to begin unless BOTH transmitters
//! report connected (heart offsets 0/1; the APK gates the same toggle on
//! both mics, `HLDevice6302Activity.java:805-812`). The owner's physical
//! confirmation is the transmitter LEDs changing colour during the brief
//! on phase (a manual observation, reported as such, never evidence).

use holly_usb::{HollyControl, NusbTransport};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// The single physical dongle must not be claimed twice at once: the test
/// harness runs tests in parallel threads, and two concurrent IF0 claims
/// collide with EBUSY (observed 2026-11-05: set_noise_roundtrip_guarded
/// failed with `interface is busy (errno 16)` while
/// open_and_read_heart_info held the claim). Poisoning only cascades
/// within one run; every `cargo test` invocation is a fresh process.
fn device_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Drop guard (Task 4b): on any exit path — normal return, failed assert,
/// or panic unwind — poll the device afresh and set NC back to the state it
/// had when the guard was created, *iff* the current state differs. Errors
/// are logged, never raised from `drop`.
struct NoiseRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: bool,
}

impl NoiseRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for NoiseRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                if info.noise_switch == self.original {
                    eprintln!(
                        "guard: NC already at original state ({}), nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: NC at {}, original {} — sending restore SET",
                        info.noise_switch, self.original
                    );
                    if let Err(e) = ctl.set_noise(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left at {} while original was {}",
                            info.noise_switch, self.original
                        );
                    }
                }
                // Verify what the restore actually did — fresh GET again.
                match ctl.get_heart_info() {
                    Ok(post) => eprintln!("guard: post-restore NC switch = {}", post.noise_switch),
                    Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                }
            }
        }
    }
}

/// Task 4b: guarded SET_NOISE round trip on real hardware.
/// SET_NOISE only; stop on any error or unexpected reply (every failure
/// surfaces via `expect`/`assert_eq!` — no retry loops); the guard restores
/// the original state on every exit path including panics.
///
/// Run manually with: cargo test -p holly-usb -- --ignored
#[test]
#[ignore = "mutates device state; run after confirming the dongle is plugged in"]
fn set_noise_roundtrip_guarded() {
    let _dev = device_lock(); // serialize: one claim of the dongle at a time
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    // SAFETY GATE: only proceed on the certified 17-byte payload shape; any
    // other length is treated as malformed here, like everywhere else.
    assert_eq!(
        original.raw.len(),
        17,
        "unexpected heart payload shape — refusing SET run"
    );
    eprintln!(
        "original NC: on={} level={}",
        original.noise_switch, original.noise_level
    );

    let mut guard = NoiseRestoreGuard {
        ctl: Some(ctl),
        original: original.noise_switch,
    };
    let opposite = !original.noise_switch;
    guard
        .ctl()
        .set_noise(opposite)
        .expect("SET to opposite state failed");
    let changed = guard
        .ctl()
        .get_heart_info()
        .expect("GET after first SET failed");
    assert_eq!(
        changed.noise_switch, opposite,
        "SET did not take effect; failing (guard restores original on the way out)"
    );
    eprintln!("toggle confirmed: NC now {opposite}");

    guard
        .ctl()
        .set_noise(original.noise_switch)
        .expect("SET back to original failed");
    let restored = guard.ctl().get_heart_info().expect("final GET failed");
    assert_eq!(
        restored.noise_switch, original.noise_switch,
        "state did not return to the original; guard will try again on exit"
    );
    assert_eq!(
        restored.raw[7], original.raw[7],
        "noise level must be unchanged"
    );
}

#[test]
#[ignore = "needs the 3547:0407 dongle plugged in and udev uaccess"]
fn open_and_read_heart_info() {
    let _dev = device_lock(); // serialize: one claim of the dongle at a time
    let mut c = HollyControl::open().expect("dongle open+claim IF0");
    let info = c.get_heart_info().expect("GET_HEART_INFO on real wire");
    println!("HeartInfo: {info:#?}");
    // Sanity gates that require no assumptions about rig state:
    // battery is a 0..=100 percent byte baked into the field map (u8 by type),
    // and the payload length is the certified single-TX field set.
    assert_eq!(info.raw.len(), 17, "certified 17-byte payload");
}

#[test]
#[ignore = "needs the 3547:0407 dongle plugged in and udev uaccess"]
fn open_fails_cleanly_without_device() {
    let _dev = device_lock(); // this test also claims IF0 when the dongle is attached
    // Nothing to assert on the error kind beyond it being produced: when no
    // dongle is present, this must be Error::NotFound, not a panic.
    match HollyControl::open() {
        Err(e) => assert!(matches!(
            e,
            holly_usb::Error::NotFound | holly_usb::Error::Usb(_) | holly_usb::Error::Decode(_)
        )),
        Ok(mut c) => {
            // With a dongle attached, opening must be fully functional;
            // verify the session is read-only viable then drop (auto-release).
            let _ = c.get_heart_info();
        }
    }
}

/// Drop guard for the LEVEL test: restores BOTH NC bytes on every exit path
/// (diffs come from a fresh GET). Order matters: restore the LEVEL byte
/// first (`set_noise_level` keeps the switch as the fresh GET reports), THEN
/// the switch (`set_noise` keeps whatever level the fresh GET then reports —
/// which is by then the restored original). Errors log, never raise.
struct NcRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    orig_switch: bool,
    orig_level: u8,
}

impl NcRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for NcRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot restore: {e:?}"),
            Ok(info) => {
                if info.noise_level != self.orig_level {
                    eprintln!(
                        "guard: level {} != original {} — restoring level",
                        info.noise_level, self.orig_level
                    );
                    if let Err(e) = ctl.set_noise_level(self.orig_level) {
                        eprintln!(
                            "guard: LEVEL RESTORE FAILED ({e:?}) — device left at level {}",
                            info.noise_level
                        );
                    }
                }
                match ctl.get_heart_info() {
                    Ok(info) => {
                        if info.noise_switch != self.orig_switch {
                            eprintln!(
                                "guard: switch {} != original {} — restoring switch",
                                info.noise_switch, self.orig_switch
                            );
                            if let Err(e) = ctl.set_noise(self.orig_switch) {
                                eprintln!(
                                    "guard: SWITCH RESTORE FAILED ({e:?}) — device left at switch {}",
                                    info.noise_switch
                                );
                            }
                        } else {
                            eprintln!("guard: switch already restored, nothing to do");
                        }
                    }
                    Err(e) => eprintln!("guard: second state poll failed: {e:?}"),
                }
                match ctl.get_heart_info() {
                    Ok(post) => eprintln!(
                        "guard: post-restore NC = (on={} level={})",
                        post.noise_switch, post.noise_level
                    ),
                    Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                }
            }
        }
    }
}

/// Guarded SET_NOISE-LEVEL write on real hardware (the second byte of cmd 19):
/// read the original pair, write a different APK-certified level value, verify
/// via a fresh GET, restore the original level. The switch must come through
/// untouched. Failure of any leg aborts loudly — no retries.
///
/// Target choice: only values the APK itself presents for this product
/// (`HLDevice6302Activity.java:1593/:1610/:1626-1631` — strong 2, medium 1,
/// low 0); the test writes whichever of {1, 2} differs from the current level,
/// so the round trip always includes a real state change.
///
/// Run manually with: cargo test -p holly-usb --test hardware -- --ignored --nocapture
#[test]
#[ignore = "mutates device state; run after confirming the dongle is plugged in"]
fn set_noise_level_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original NC: on={} level={} (tx2 battery {})",
        original.noise_switch, original.noise_level, original.tx2_battery
    );

    // Pick an APK-known target level that differs from the observed one.
    let target = if original.noise_level == 1 { 2 } else { 1 };
    eprintln!("writing target level {target}");

    let mut guard = NcRestoreGuard {
        ctl: Some(ctl),
        orig_switch: original.noise_switch,
        orig_level: original.noise_level,
    };

    guard
        .ctl()
        .set_noise_level(target)
        .expect("level SET failed");
    let after = guard.ctl().get_heart_info().expect("post-SET GET failed");
    assert_eq!(
        after.noise_level, target,
        "device must report the written level"
    );
    assert_eq!(
        after.noise_switch, original.noise_switch,
        "level SET must leave the switch untouched"
    );
    assert_eq!(after.raw.len(), 17, "reply shape unchanged");
    eprintln!("level write confirmed: level={}", after.noise_level);

    // Explicit restore leg (the guard double-covers, incl. mid-run panics).
    guard
        .ctl()
        .set_noise_level(original.noise_level)
        .expect("restore level SET failed");
    let restored = guard.ctl().get_heart_info().expect("restore GET failed");
    assert_eq!(
        restored.noise_level, original.noise_level,
        "level did not return to original; guard will retry on exit"
    );
    eprintln!("restore confirmed: level={}", restored.noise_level);
}

// ── Phase 1B route (b): EQ (cmd 7) ------------------------------------------------

/// Bounded read-only settle poll (route (b), owner rule 2026-11-09):
/// a FIXED number of fresh GETs spanning about 2 s run before any heart
/// mismatch is declared — a slow heart is not a dropped write. GET-only;
/// the SET behind this poll is never retried.
/// `Ok(())` = a fresh GET reported `eqLevel == want`;
/// `Err(Some(last))` = the fixed budget ran out (`last` = the final
/// `eqLevel` seen); `Err(None)` = no parseable heart within the budget.
fn settle_eq_level(ctl: &mut HollyControl<NusbTransport>, want: u8) -> Result<(), Option<u8>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.eq_level == want {
                    return Ok(());
                }
                last = Some(info.eq_level);
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the EQ test: on any exit path — normal return, failed
/// assert, or panic unwind — poll the device afresh and, IF the current
/// `eqLevel` differs from the run's starting pad, make ONE verified restore
/// attempt (route (b) rule: the "no retry" clause applies to the SET, and
/// this guard is the restore-of-last-resort, not a second write scheme).
/// Errors are logged, never raised from `drop`.
struct EqRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl EqRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for EqRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                if info.eq_level == self.original {
                    eprintln!(
                        "guard: EQ already at original pad {}, nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: EQ at pad {}, original {} — sending one restore SET",
                        info.eq_level, self.original
                    );
                    if let Err(e) = ctl.set_eq(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left at pad {} while original was {}",
                            info.eq_level, self.original
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => eprintln!("guard: post-restore eqLevel = {}", post.eq_level),
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_EQ round trip on real hardware —
/// substitute evidence for roadmap §1 step 2; on success the protocol.md
/// row tags `[wire] PC-originated round trip`.
///
/// Rules (docs/gui-roadmap.md §1(b), owner rules 2026-11-09):
///   fresh GET → SET → ACK gate (`data[0]==0`) → settle-verified heart
///   delta on `eqLevel` (§5 offset 12, RECEIVER-level) → restore →
///   settled verify. Restore attempted on every exit path.
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the pad moved from the
///   start, ONE verified restore attempt; abort either way.
/// - Any heart mismatch is declared only AFTER the fixed ~2 s settle poll.
/// - Target = the NEIGHBOURING wire-mapped pad (1=Low / 2=Bright /
///   3="Equalization", checklist §3 EQ row), never an extreme.
/// - The SET leg's own fresh-GET return (the pad in force just before the
///   write) is the restore target; drift vs the pre-run GET retargets the
///   guard, and the restore leg's return must report the written pad.
/// - usbmon (owner-executed) captures the run; the .pcap + exported .jsonl
///   land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_eq_roundtrip_guarded -- --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active"]
fn set_eq_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original eqLevel = {} (wire-mapped space 1..=3)",
        original.eq_level
    );

    // Neighbouring valid value — never an extreme (route (b) rule).
    let target = match original.eq_level {
        1 => 2,
        2 => 1,
        3 => 2,
        other => {
            panic!("eqLevel {other} is outside the wire-mapped space 1..=3 — refusing the run")
        }
    };
    eprintln!("writing neighbouring pad {target}");

    let mut guard = EqRestoreGuard {
        ctl: Some(ctl),
        original: original.eq_level,
    };

    // SET leg — the op's own fresh GET happens inside set_eq, and the u8
    // it returns is the pad in force JUST before the write: the freshest
    // restore memory the device has. It wins over the pre-run poll; if
    // another client moved EQ between the two GETs, the difference is
    // reported and the exit guard retargets.
    let start = match guard.ctl().set_eq(target) {
        Ok(previous) => {
            if previous != original.eq_level {
                eprintln!(
                    "DRIFT: pre-run GET said eq {}, op's fresh GET reported eq {} — restore retargeted to {}",
                    original.eq_level, previous, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 path: NO SET retry. One fresh GET decides.
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!("eqLevel after reject: {}", now.eq_level);
            if now.eq_level != original.eq_level {
                eprintln!(
                    "pad moved ({} -> {}): ONE verified restore attempt",
                    original.eq_level, now.eq_level
                );
                if let Err(e) = guard.ctl().set_eq(original.eq_level) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_eq_level(guard.ctl(), original.eq_level) {
                    Ok(()) => {
                        eprintln!(
                            "reject-path restore verified: eqLevel={}",
                            original.eq_level
                        )
                    }
                    Err(last) => eprintln!("reject-path restore NOT settled (last seen {last:?})"),
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule).
    match settle_eq_level(guard.ctl(), target) {
        Ok(()) => eprintln!("EQ write confirmed: eqLevel={target}"),
        Err(last) => panic!(
            "heart never reported eqLevel {target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Explicit restore leg (the guard double-covers mid-run panics).
    // The restore TARGET is `start` — set_eq's own returned previous —
    // and the leg's return must report the pad the SET leg wrote, which
    // cross-checks that the fresh GET inside a SET sees that SET's own
    // effect on the device.
    let restore_previous = guard.ctl().set_eq(start).expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the pad the SET leg wrote"
    );
    match settle_eq_level(guard.ctl(), start) {
        Ok(()) => eprintln!("restore confirmed: eqLevel={start}"),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Phase 1B route (b): gain / Volume adjustment (cmd 3) ─────────────────────

/// Bounded read-only settle poll for `voiceLevel` — same shape as
/// [`settle_eq_level`] (route (b) owner rule): a FIXED number of fresh GETs
/// spanning about 2 s run before any heart mismatch is declared — a slow
/// heart is not a dropped write. GET-only; the SET behind this poll is never
/// retried. A poll CONFIRMS only when the voice byte matches AND `voiceMode`
/// is still `mode`: the SET payload is a single byte, so a mode movement
/// would be a protocol violation, reported as a settle failure (§5 offset 9
/// is a separate heart field; we never write it).
/// `Ok(())` = a fresh GET reported `voiceLevel == want` with `voiceMode`
/// unchanged; `Err(Some(last))` = the fixed budget ran out (`last` = the
/// final voice stop seen); `Err(None)` = no parseable heart within the
/// budget.
fn settle_voice_level(
    ctl: &mut HollyControl<NusbTransport>,
    want: u8,
    mode: u8,
) -> Result<(), Option<u8>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.voice_level == want {
                    if info.voice_mode == mode {
                        return Ok(());
                    }
                    eprintln!(
                        "settle poll {poll:>2}/{POLLS}: voice matched {} but voiceMode moved {} -> {} — treating as a mismatch",
                        info.voice_level, mode, info.voice_mode
                    );
                }
                last = Some(info.voice_level);
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the gain test: mirrors [`EqRestoreGuard`] — on any exit
/// path (normal return, failed assert, panic unwind) poll the device afresh
/// and, IF the current `voiceLevel` differs from the run's starting stop,
/// make ONE verified restore attempt (route (b) rule: the "no retry"
/// clause applies to the SET; this guard is the restore-of-last-resort,
/// not a second write scheme). Errors are logged, never raised from `drop`.
struct GainRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl GainRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for GainRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                if info.voice_level == self.original {
                    eprintln!(
                        "guard: gain already at original stop {}, nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: gain at stop {}, original {} — sending one restore SET",
                        info.voice_level, self.original
                    );
                    if let Err(e) = ctl.set_gain(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left at stop {} while original was {}",
                            info.voice_level, self.original
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => {
                            eprintln!("guard: post-restore voiceLevel = {}", post.voice_level)
                        }
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_VOICE_LEVEL (gain / "Volume adjustment")
/// round trip on real hardware — substitute evidence for the §1 step-2
/// gate; on success the checklist §3 Gain row + protocol.md §4 SET_VOICE_LEVEL
/// row tag `[wire] PC-originated round trip`.
///
/// Rules — the set_eq_roundtrip_guarded rule set applied to gain:
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta on `voiceLevel` (§5 offset 8) → restore → settled verify.
///   Restore attempted on every exit path.
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the stop moved from the
///   start, ONE verified restore attempt; abort either way.
/// - Any heart mismatch is declared only AFTER the fixed ~2 s settle poll.
/// - Target = the NEIGHBOURING stop BELOW the current one (quieter —
///   volume goes down, never up, owner rule 2026-11-10); the bottom stop
///   (wire 0 = phone's stop "1") counts as an extreme, so a device resting
///   at wire stop 0 or 1 leaves no valid neighbour-below and the run
///   REFUSES instead of writing up or down an extreme.
/// - `voiceMode` (§5 offset 9) is a separate heart field the single-byte
///   payload cannot carry — never written; the settle poll asserts it.
/// - The SET leg's own fresh-GET return (the stop in force just before the
///   write) is the restore target; drift vs the pre-run GET retargets the
///   guard, and the restore leg's return must report the written stop.
/// - usbmon (owner-executed) captures the run; the .pcap + exported .jsonl
///   land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_gain_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active"]
fn set_gain_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original voiceLevel = {} (wire stop space 0..=5; phone shows stop {}), voiceMode = {}",
        original.voice_level,
        original.voice_level + 1,
        original.voice_mode
    );

    // Neighbouring stop BELOW the current one — quieter, never an extreme,
    // never above the current level (owner rule). Wire 0/1 have no such
    // neighbour (0 IS the bottom stop, 1's only neighbour-below is 0), so
    // those device states refuse the run.
    let target = match original.voice_level {
        2..=5 => original.voice_level - 1,
        0 | 1 => panic!(
            "voiceLevel {} leaves no stop below that is not the extreme (wire 0) — refusing the run; raise the volume via the phone/app first",
            original.voice_level
        ),
        other => panic!(
            "voiceLevel {other} is outside the wire-mapped stop space 0..=5 — refusing the run"
        ),
    };
    eprintln!(
        "writing neighbouring stop {target} (phone stop {})",
        target + 1
    );

    let mut guard = GainRestoreGuard {
        ctl: Some(ctl),
        original: original.voice_level,
    };

    // SET leg — the op's own fresh GET happens inside set_gain, and the u8
    // it returns is the stop in force JUST before the write: the freshest
    // restore memory the device has. It wins over the pre-run poll; if
    // another client moved gain between the two GETs, the difference is
    // reported and the exit guard retargets.
    let start = match guard.ctl().set_gain(target) {
        Ok(previous) => {
            if previous != original.voice_level {
                eprintln!(
                    "DRIFT: pre-run GET said voice {}, op's fresh GET reported voice {} — restore retargeted to {}",
                    original.voice_level, previous, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 path: NO SET retry. One fresh GET decides.
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!("voiceLevel after reject: {}", now.voice_level);
            if now.voice_level != original.voice_level {
                eprintln!(
                    "stop moved ({} -> {}): ONE verified restore attempt",
                    original.voice_level, now.voice_level
                );
                if let Err(e) = guard.ctl().set_gain(original.voice_level) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_voice_level(guard.ctl(), original.voice_level, original.voice_mode) {
                    Ok(()) => eprintln!(
                        "reject-path restore verified: voiceLevel={}",
                        original.voice_level
                    ),
                    Err(last) => eprintln!("reject-path restore NOT settled (last seen {last:?})"),
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule).
    // The poll's match condition includes voiceMode == the pre-run mode
    // (§5 offset 9 is a separate field; see settle_voice_level).
    match settle_voice_level(guard.ctl(), target, original.voice_mode) {
        Ok(()) => eprintln!("gain write confirmed: voiceLevel={target}"),
        Err(last) => panic!(
            "heart never reported voiceLevel {target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Explicit restore leg (the guard double-covers mid-run panics).
    // The restore TARGET is `start` — set_gain's own returned previous —
    // and the leg's return must report the stop the SET leg wrote, which
    // cross-checks that the fresh GET inside a SET sees that SET's own
    // effect on the device.
    let restore_previous = guard.ctl().set_gain(start).expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the stop the SET leg wrote"
    );
    match settle_voice_level(guard.ctl(), start, original.voice_mode) {
        Ok(()) => eprintln!("restore confirmed: voiceLevel={start}"),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Reverb (cmd 5, SET_REVERB) ───────────────────────────────────────────────

/// Bounded read-only settle poll for the reverb PAIR — same shape as
/// [`settle_eq_level`] (route (b) owner rule): a FIXED number of fresh GETs
/// spanning about 2 s run before any heart mismatch is declared — a slow
/// heart is not a dropped write. GET-only; the SET behind this poll is never
/// retried. The poll CONFIRMS only when BOTH bytes match (`reverbSwitch`
/// §5 offset 13 == `want_switch` AND the gear byte §5 offset 14 ==
/// `want_gear`): the docs' certified behaviour is that the heart reports
/// the gear unchanged while the switch is off [wire
/// captures/2026-11-05-hearinfo-first-wire.txt payload[13]=00/payload[14]=02;
/// §5.2 sessions 1-4], so a one-leg-only settle would hide a half write.
/// `Ok(())` = a fresh GET reported the whole pair;
/// `Err(Some(last))` = the fixed budget ran out (`last` = the final pair
/// seen); `Err(None)` = no parseable heart within the budget.
fn settle_reverb(
    ctl: &mut HollyControl<NusbTransport>,
    want_switch: bool,
    want_gear: u8,
) -> Result<(), Option<(bool, u8)>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.reverb_switch == want_switch && info.reverb_level == want_gear {
                    return Ok(());
                }
                last = Some((info.reverb_switch, info.reverb_level));
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the reverb test: mirrors [`GainRestoreGuard`] — on any
/// exit path (normal return, failed assert, panic unwind) poll the device
/// afresh and, IF the current `(reverbSwitch, reverbLevel)` pair differs
/// from the run's starting pair, make ONE verified restore attempt via
/// `set_reverb(original pair)` (route (b) rule: the "no retry" clause
/// applies to the SET; this guard is the restore-of-last-resort, not a
/// second write scheme). Restoring the PAIR — off, the starting gear —
/// mirrors the APK's own toggle-off path, which sends its remembered gear
/// [code HLDevice6302Activity.java:792-793]. Errors are logged, never
/// raised from `drop`.
struct ReverbRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original_switch: bool,
    original_gear: u8,
}

impl ReverbRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for ReverbRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                if info.reverb_switch == self.original_switch
                    && info.reverb_level == self.original_gear
                {
                    eprintln!(
                        "guard: reverb already at original pair ({}, gear {}), nothing to do",
                        self.original_switch, self.original_gear
                    );
                } else {
                    eprintln!(
                        "guard: reverb at ({}, gear {}), original ({}, gear {}) — sending one restore SET",
                        info.reverb_switch,
                        info.reverb_level,
                        self.original_switch,
                        self.original_gear
                    );
                    if let Err(e) = ctl.set_reverb(self.original_switch, self.original_gear) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left at ({}, gear {}) while original was ({}, gear {})",
                            info.reverb_switch,
                            info.reverb_level,
                            self.original_switch,
                            self.original_gear
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => eprintln!(
                            "guard: post-restore reverb = (on={} gear={})",
                            post.reverb_switch, post.reverb_level
                        ),
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_REVERB round trip on real hardware —
/// substitute evidence for the §1 step-2 gate; on success the checklist §3
/// Reverb row + protocol.md §4 SET_REVERB row tag `[wire] PC-originated
/// round trip` (§5.2's gear-keeps-value-while-off note).
///
/// ⚠ AUDIBLE: reverb is applied to the mic signal — keep REC/MONITORING
/// off while this runs.
///
/// Rules — the set_gain_roundtrip_guarded rule set applied to reverb:
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta on the PAIR (`reverbSwitch` §5 offset 13 + gear §5
///   offset 14) → restore → settled verify. Restore attempted on every
///   exit path.
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the pair moved from the
///   start, ONE verified restore attempt; abort either way.
/// - Any heart mismatch is declared only AFTER the fixed ~2 s settle poll.
/// - Gear target = NEIGHBOURING certified gear (space 0/1/2 =
///   GEAR_ONE/TWO/THREE [code HLDevice6302Activity.java:629-632];
///   0→1, 1→2, 2→1), never out of space: anything outside 0..=2 refuses
///   the run (guard convention — the op itself never range-checks).
/// - Start-shape branch (toggle is the cheapest first delta):
///   switch OFF → first leg toggles ON with the STARTING gear (the
///   APK-toggle shape), second leg moves the NEIGHBOURING gear while ON;
///   switch ON → the legs only move gear and the run NEVER writes the
///   switch as off.
/// - The SET leg's own fresh-GET return (the pair in force just before
///   the write) is the restore target; drift vs the pre-run GET retargets
///   the guard, and the restore leg's return must report the pair the SET
///   leg wrote.
/// - usbmon (owner-executed) captures the run; the .pcap + exported
///   .jsonl land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_reverb_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — reverb is AUDIBLE on the mics"]
fn set_reverb_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original reverb: switch={} gear={} — ⚠ reverb is AUDIBLE on the mics, keep REC/MONITORING off",
        original.reverb_switch, original.reverb_level
    );

    // Neighbouring certified gear — never out of space, never an extreme
    // hop (route (b) rule). 0's neighbour is 1; 1 goes up to 2; 2 comes
    // back to 1. Any device resting outside 0..=2 refuses the run.
    let gear_target = match original.reverb_level {
        0 => 1,
        1 => 2,
        2 => 1,
        other => panic!(
            "reverb gear {other} is outside the certified space 0..=2 (GEAR_ONE/TWO/THREE) — refusing the run"
        ),
    };

    let mut guard = ReverbRestoreGuard {
        ctl: Some(ctl),
        original_switch: original.reverb_switch,
        original_gear: original.reverb_level,
    };

    // FIRST leg. Switch OFF → toggle ON with the starting gear (the
    // APK-toggle shape: the switch is the delta, gear passes through).
    // Switch ON → only the neighbouring gear moves (the switch byte stays
    // true; the run never turns reverb off). The op's own fresh GET
    // happens inside set_reverb, and the PAIR it returns is the state in
    // force JUST before the write: the freshest restore memory the device
    // has. It wins over the pre-run poll; if another client moved reverb
    // between the two GETs, the difference is reported and the exit guard
    // retargets.
    let first = if original.reverb_switch {
        guard.ctl().set_reverb(true, gear_target)
    } else {
        guard.ctl().set_reverb(true, original.reverb_level)
    };
    let start = match first {
        Ok(previous) => {
            if previous != (original.reverb_switch, original.reverb_level) {
                eprintln!(
                    "DRIFT: pre-run GET said ({}, gear {}), op's fresh GET reported ({}, gear {}) — restore retargeted",
                    original.reverb_switch, original.reverb_level, previous.0, previous.1
                );
                (guard.original_switch, guard.original_gear) = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 path: NO SET retry. One fresh GET decides.
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!(
                "reverb after reject: (on={} gear={})",
                now.reverb_switch, now.reverb_level
            );
            let now_pair = (now.reverb_switch, now.reverb_level);
            let original_pair = (original.reverb_switch, original.reverb_level);
            if now_pair != original_pair {
                eprintln!(
                    "pair moved ({:?} -> {:?}): ONE verified restore attempt",
                    original_pair, now_pair
                );
                if let Err(e) = guard
                    .ctl()
                    .set_reverb(original.reverb_switch, original.reverb_level)
                {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_reverb(guard.ctl(), original.reverb_switch, original.reverb_level) {
                    Ok(()) => eprintln!(
                        "reject-path restore verified: reverb = ({:?})",
                        original_pair
                    ),
                    Err(last) => eprintln!("reject-path restore NOT settled (last seen {last:?})"),
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule).
    // First leg confirms the switch state written (on) with the gear in
    // force after it: the toggle leg keeps the starting gear, the gear leg
    // lands on the neighbour.
    let first_want_gear = if original.reverb_switch {
        gear_target
    } else {
        original.reverb_level
    };
    match settle_reverb(guard.ctl(), true, first_want_gear) {
        Ok(()) => eprintln!("first leg confirmed: on gear={first_want_gear}"),
        Err(last) => panic!(
            "heart never reported (on, gear {first_want_gear}) within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // SECOND leg (switch was OFF at start only): move the NEIGHBOURING
    // gear while reverb is on — a real gear delta, on the on state this
    // run already created. When the switch started ON there is no second
    // leg: the first leg WAS the neighbour-gear move and the restore is
    // the original pair. Restore NEVER goes through a switch-off hop
    // either way: it is exactly the starting pair (when that is off, the
    // APK-toggle shape writes off WITH the starting gear as its value
    // byte).
    if !original.reverb_switch {
        match guard.ctl().set_reverb(true, gear_target) {
            Ok(previous) => {
                assert_eq!(
                    previous,
                    (true, first_want_gear),
                    "gear leg's fresh GET must report the pair the first leg wrote"
                );
            }
            Err(e) => {
                // No SET retry (route (b)); the exit guard still restores.
                eprintln!(
                    "neighbour-gear SET failed: {e:?} — aborting; guard restores the original on the way out"
                );
                panic!(
                    "neighbour-gear SET failed (ack gate) — aborting with no retry per route (b)"
                );
            }
        }
        match settle_reverb(guard.ctl(), true, gear_target) {
            Ok(()) => eprintln!("gear change confirmed: on gear={gear_target}"),
            Err(last) => panic!(
                "heart never reported (on, gear {gear_target}) within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
            ),
        }
    }

    // Restore leg = exactly the starting pair, via set_reverb so the
    // guard's cross-check style mirrors EQ/gain: the restore's fresh GET
    // must report the last pair this run left on the device. Both branches
    // ended with reverb ON at the neighbouring gear: the S=on branch's
    // only write leg was that move, the S=off branch's second leg was.
    let last_pair = (true, gear_target);
    let restore_previous = guard
        .ctl()
        .set_reverb(start.0, start.1)
        .expect("restore SET failed");
    assert_eq!(
        restore_previous, last_pair,
        "restore leg's fresh GET must report the pair the write leg left"
    );
    match settle_reverb(guard.ctl(), start.0, start.1) {
        Ok(()) => eprintln!("restore confirmed: reverb pair = ({:?})", start),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Channel mode (cmd 1 SET_VOICE_MODE) — the re-enumeration-risk control ───

/// Drop guard for the channel-mode test: mirrors [`ReverbRestoreGuard`],
/// with the feature-prompt's re-enumeration tolerance (§Risks) — the mode
/// change can drop the control handle or change the USB address, so the
/// restore path must REOPEN by VID:PID within a bounded window instead of
/// trusting the handle the run opened. On any exit path (normal return,
/// failed assert, panic unwind): up to 4 reopen rounds (the same ≈2 s
/// budget shape as the settle poll) fetch a fresh GET; the FIRST successful
/// GET decides — if the mode differs from the run's starting mode, ONE
/// verified restore attempt via `set_channel_mode(original)`. Errors are
/// logged, never raised from `drop`; a device that never answers inside
/// the window aborts with the restore NOT verified (report it, no rerun).
struct ChannelModeRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl ChannelModeRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }

    /// Replace the handle after a suspected re-enumeration: drop the dead
    /// one (its Drop reattaches the kernel driver — harmless when the node
    /// is gone), then enumerate afresh and claim IF0. `HollyControl::open`
    /// matches VID:PID only, so a CHANGED USB device address is fine. One
    /// attempt per call; callers bound the retries with the settle budget.
    fn reopen(&mut self) {
        self.ctl = None;
        match HollyControl::open() {
            Ok(c) => {
                eprintln!("reopen: IF0 re-claimed by VID:PID 3547:0407");
                self.ctl = Some(c);
            }
            Err(e) => eprintln!("reopen by VID:PID failed: {e:?}"),
        }
    }
}

impl Drop for ChannelModeRestoreGuard {
    fn drop(&mut self) {
        const ROUNDS: u32 = 4;
        for round in 0..ROUNDS {
            if self.ctl.is_none() {
                std::thread::sleep(Duration::from_millis(500));
                self.reopen();
            }
            let Some(ctl) = self.ctl.as_mut() else {
                continue;
            };
            match ctl.get_heart_info() {
                Ok(info) => {
                    if info.voice_mode == self.original {
                        eprintln!(
                            "guard: channel mode already at original ({}), nothing to do",
                            self.original
                        );
                    } else {
                        eprintln!(
                            "guard: channel mode at {}, original {} — sending one restore SET",
                            info.voice_mode, self.original
                        );
                        if let Err(e) = ctl.set_channel_mode(self.original) {
                            eprintln!(
                                "guard: RESTORE FAILED ({e:?}) — device left at {} while original was {}",
                                info.voice_mode, self.original
                            );
                        }
                        // Verify what the single restore attempt actually did.
                        match ctl.get_heart_info() {
                            Ok(post) => {
                                eprintln!("guard: post-restore voiceMode = {}", post.voice_mode)
                            }
                            Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                        }
                    }
                    return;
                }
                Err(e) => {
                    eprintln!("guard: state poll failed (round {round}/{ROUNDS}): {e:?}");
                    self.ctl = None;
                }
            }
        }
        eprintln!(
            "guard: device never answered within the reopen window — restore NOT verified, REPORT"
        );
    }
}

/// Bounded settle poll for the channel-mode delta: 4 fresh GETs ≈ 2 s,
/// reads only. `Ok(())` = a fresh GET reported the written mode;
/// `Err(Some(last))` = the budget ran out (`last` = the final mode seen);
/// `Err(None)` = no parseable heart within the budget.
///
/// Re-enumeration tolerance (feature prompt §Risks): the write can drop
/// the control handle or change the USB address, so a GET failure inside
/// the budget triggers a reopen by VID:PID and the poll CONTINUES on the
/// fresh handle within the SAME window. If the device does not come back
/// inside the window the budget simply runs out — the caller aborts and
/// the exit guard's own reopen window is the last resort.
fn settle_channel_mode(guard: &mut ChannelModeRestoreGuard, want: u8) -> Result<(), Option<u8>> {
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        let Some(ctl) = guard.ctl.as_mut() else {
            // Handle was lost (or the previous reopen failed): reopen by
            // VID:PID inside the SAME bounded window, keep polling.
            guard.reopen();
            continue;
        };
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.voice_mode == want {
                    return Ok(());
                }
                last = Some(info.voice_mode);
            }
            Err(e) => {
                eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}");
                guard.reopen();
            }
        }
    }
    Err(last)
}

/// Phase 1B route (b): guarded SET_VOICE_MODE round trip on real hardware —
/// substitute evidence for the §1 step-2 gate; on success the checklist §3
/// Channel-mode row + protocol.md §4 SET_VOICE_MODE row tag `[wire]
/// PC-originated round trip` (§5.2's read map gets its write-side
/// confirmation).
///
/// ⚠ THE AUDIO PATH CHANGES: keep recording, calls, music and any
/// monitoring closed while this runs (feature prompt §Risks). The dongle
/// may briefly disappear or change its USB address after the write — the
/// capture script records the whole bus, so the pcap survives an address
/// change; this test tolerates the gap by reopening by VID:PID.
///
/// Rules — the set_reverb_roundtrip_guarded rule set applied to channel
/// mode:
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta (`voiceMode` §5 offset 9) → restore → settled verify.
///   Restore attempted on every exit path (with reopen).
/// - Precondition (feature prompt §Guarded test design): the dongle is
///   readable and the current mode is one of the two documented values;
///   anything else refuses the run. The written value is the OTHER
///   documented value — never outside {0, 1}.
/// - ACK ≠ 0 or SET transport failure: NO SET retry — one fresh GET (the
///   exit guard's reopen window covers a vanished device); if the mode
///   moved from the start, ONE verified restore attempt; abort either way.
/// - Any heart mismatch is declared only AFTER the fixed ~2 s settle poll.
/// - The SET leg's own fresh-GET return (the mode in force just before the
///   write) is the restore target; drift vs the pre-run GET retargets the
///   guard, and the restore leg's return must report the mode the SET leg
///   wrote.
/// - usbmon (owner-executed) captures the run; the .pcap + exported .jsonl
///   land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_channel_mode_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — the audio path re-presents; keep REC/CALLS/MUSIC/MONITORING off"]
fn set_channel_mode_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original channel mode: voiceMode={} (0=Mono, 1=Stereo) — ⚠ keep REC/CALLS/MUSIC/MONITORING off",
        original.voice_mode
    );
    eprintln!(
        "TX states: tx1={} tx2={}",
        original.tx1_status, original.tx2_status
    );

    // Precondition: current mode inside the documented space; write the
    // OTHER documented value. Never out of space (route (b) rule).
    let target = match original.voice_mode {
        0 => 1,
        1 => 0,
        other => panic!(
            "voiceMode {other} is outside the documented space 0/1 (Mono/Stereo) — refusing the run"
        ),
    };

    let mut guard = ChannelModeRestoreGuard {
        ctl: Some(ctl),
        original: original.voice_mode,
    };

    // WRITE leg: the other documented value. The op's own fresh GET happens
    // inside set_channel_mode, and the mode it returns is the state in
    // force JUST before the write: the freshest restore memory the device
    // has. It wins over the pre-run poll; if another client moved the mode
    // between the two GETs, the difference is reported and the exit guard
    // retargets.
    let start = match guard.ctl().set_channel_mode(target) {
        Ok(previous) => {
            if previous != original.voice_mode {
                eprintln!(
                    "DRIFT: pre-run GET said {}, op's fresh GET reported {} — restore retargeted",
                    original.voice_mode, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 / transport-failure path: NO SET retry. One
            // fresh GET decides (if the device vanished mid-SET this GET
            // fails and the panic lands in the exit guard's reopen window).
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!("channel mode after reject: {}", now.voice_mode);
            let wanted = guard.original;
            if now.voice_mode != wanted {
                eprintln!(
                    "mode moved ({} -> {}): ONE verified restore attempt",
                    wanted, now.voice_mode
                );
                if let Err(e) = guard.ctl().set_channel_mode(wanted) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_channel_mode(&mut guard, wanted) {
                    Ok(()) => eprintln!("reject-path restore verified: mode = {}", wanted),
                    Err(last) => {
                        eprintln!("reject-path restore NOT settled (last seen {last:?})")
                    }
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule,
    // reopen-tolerant).
    match settle_channel_mode(&mut guard, target) {
        Ok(()) => eprintln!("write leg confirmed: voiceMode={target}"),
        Err(last) => panic!(
            "heart never reported voiceMode={target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Restore leg = exactly the starting mode, via set_channel_mode so the
    // guard's cross-check style mirrors EQ/gain/reverb: the restore's fresh
    // GET must report the mode the write leg left.
    let restore_previous = guard
        .ctl()
        .set_channel_mode(start)
        .expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the mode the write leg left"
    );
    match settle_channel_mode(&mut guard, start) {
        Ok(()) => eprintln!("restore confirmed: voiceMode={start}"),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── TX mute (cmd 22 SET_TX_MUTE) — the per-mic Mute buttons ─────────────────

/// Bounded read-only settle poll for the mute PAIR — same shape as
/// [`settle_reverb`] (route (b) owner rule): a FIXED number of fresh GETs
/// spanning about 2 s run before any heart mismatch is declared — a slow
/// heart is not a dropped write. GET-only; the SET behind this poll is
/// never retried. The poll CONFIRMS only when BOTH mute bytes match
/// (`tx1_mute` §5 offset 4 == `want_tx1` AND `tx2_mute` §5 offset 5 ==
/// the run's `baseline_tx2`): the write addresses ONE slot per frame
/// ([code] HLDevice6302Activity mute clicks, one ordinal each), so the
/// OTHER slot's byte must stay at its baseline — a one-leg-only settle
/// would hide a cross-slot side effect. `Ok(())` = a fresh GET reported
/// the whole pair; `Err(Some(last))` = the fixed budget ran out (`last`
/// = the final pair seen); `Err(None)` = no parseable heart within the
/// budget.
fn settle_tx_mute(
    ctl: &mut HollyControl<NusbTransport>,
    want_tx1: bool,
    baseline_tx2: bool,
) -> Result<(), Option<(bool, bool)>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.tx1_mute == want_tx1 && info.tx2_mute == baseline_tx2 {
                    return Ok(());
                }
                last = Some((info.tx1_mute, info.tx2_mute));
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the mute test: mirrors [`ReverbRestoreGuard`] — on any
/// exit path (normal return, failed assert, panic unwind) poll the device
/// afresh and, IF `tx1_mute` differs from the run's starting byte, make
/// ONE verified restore attempt via `set_tx_mute(0, original byte)`.
/// TX2 is NEVER written by this run (AGENTS.md: TX2 stays gated until
/// "22 lifted for TX2"): if `tx2_mute` moved from its baseline (it should
/// not — one TX per frame), the guard only REPORTS the move for the owner
/// to fix by hand. Errors are logged, never raised from `drop`.
struct TxMuteRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original_mute: bool,
    baseline_tx2_mute: bool,
}

impl TxMuteRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for TxMuteRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                if info.tx2_mute != self.baseline_tx2_mute {
                    eprintln!(
                        "guard: ⚠ TX2 mute moved ({:?} -> {:?}) during the run — NEVER written by this test; check and fix by hand",
                        self.baseline_tx2_mute, info.tx2_mute
                    );
                }
                if info.tx1_mute == self.original_mute {
                    eprintln!(
                        "guard: TX1 mute already at original ({}), nothing to do",
                        self.original_mute
                    );
                } else {
                    eprintln!(
                        "guard: TX1 mute at {:?}, original {:?} — sending one restore SET",
                        info.tx1_mute, self.original_mute
                    );
                    if let Err(e) = ctl.set_tx_mute(0, self.original_mute) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left with tx1_mute={:?} while original was {:?}",
                            info.tx1_mute, self.original_mute
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => eprintln!(
                            "guard: post-restore mutes = tx1={} tx2={}",
                            post.tx1_mute, post.tx2_mute
                        ),
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_TX_MUTE round trip on real hardware —
/// substitute evidence for the §1 step-2 gate; on success the checklist
/// §3 TX-mute row + protocol.md §4 SET_TX_MUTE row tag `[wire]
/// PC-originated round trip` (§5 offsets 4/5 write==read).
///
/// ⚠ AUDIBLE: muting TX1 cuts its mic signal — keep REC/CALLS closed
/// while this runs (owner rule; TX2 stays OFF and is never powered).
///
/// Rules — the set_reverb_roundtrip_guarded rule set applied to the mute
/// pair, with the per-slot specifics:
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta on the PAIR (`tx1_mute` §5 offset 4 == target AND
///   `tx2_mute` §5 offset 5 == baseline) → restore → settled verify.
///   Restore attempted on every exit path.
/// - Refusal preconditions (never guess the shape): the heart must be the
///   certified 17-byte field set AND TX1 must be CONNECTED — the APK only
///   mutes a live transmitter (offline → toast, no write [code
///   `HLDevice6302Activity.java:668/:682`]).
/// - Direction: the FIRST leg flips the START byte to its opposite
///   (`target = !tx1_mute`) — a device resting muted is first UNMUTED, so
///   the muted phase stays bounded either way; the unmute happens right
///   after the settle poll.
/// - TX2 is NEVER written: `tx2_mute` is only a BASELINE the settle polls
///   verify unchanged, and the exit guard REPORTS any TX2 move instead of
///   writing it.
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the byte moved from the
///   start, ONE verified restore attempt; abort either way.
/// - The SET leg's own fresh-GET return (the addressed slot's pre-write
///   mute) is the restore target; drift vs the pre-run GET retargets the
///   guard, and the restore leg's return must report the byte the SET leg
///   wrote.
/// - usbmon (owner-executed) captures the run; the .pcap + exported
///   .jsonl land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_tx_mute_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — muting TX1 is AUDIBLE (mic signal cut), keep REC/CALLS closed"]
fn set_tx_mute_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    assert!(
        original.tx1_status,
        "TX1 must be CONNECTED for the mute write (the APK refuses offline TXs [code :668/:682]) — connect TX1 and rerun"
    );
    eprintln!(
        "original mutes: tx1={} tx2={} — ⚠ muting is AUDIBLE, keep REC/CALLS closed; TX2 is never powered or written",
        original.tx1_mute, original.tx2_mute
    );
    let baseline_tx2 = original.tx2_mute;
    // Flip the START byte to its opposite: an unmuted device is first
    // muted; a device resting muted is first UNMUTED (bounded muted phase).
    let target = !original.tx1_mute;

    let mut guard = TxMuteRestoreGuard {
        ctl: Some(ctl),
        original_mute: original.tx1_mute,
        baseline_tx2_mute: baseline_tx2,
    };

    // FIRST leg: one addressed write, TX1 slot 0 only. The op's own fresh
    // GET happens inside set_tx_mute, and the Option it returns is the
    // byte in force JUST before the write: the freshest restore memory
    // the device has. It wins over the pre-run poll; if another client
    // moved the mute between the two GETs, the difference is reported and
    // the exit guard retargets.
    match guard.ctl().set_tx_mute(0, target) {
        Ok(previous) => {
            if previous != Some(original.tx1_mute) {
                eprintln!(
                    "DRIFT: pre-run GET said tx1_mute {}, op's fresh GET reported {:?} — restore retargeted",
                    original.tx1_mute, previous
                );
                if let Some(now) = previous {
                    guard.original_mute = now;
                }
            }
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 path: NO SET retry. One fresh GET decides.
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!(
                "mutes after reject: tx1={} tx2={}",
                now.tx1_mute, now.tx2_mute
            );
            if now.tx2_mute != baseline_tx2 {
                eprintln!(
                    "⚠ TX2 mute moved during the reject ({:?} -> {:?}) — reported only, never restored by this run",
                    baseline_tx2, now.tx2_mute
                );
            }
            if now.tx1_mute != original.tx1_mute {
                eprintln!(
                    "tx1_mute moved ({:?} -> {:?}): ONE verified restore attempt",
                    original.tx1_mute, now.tx1_mute
                );
                if let Err(e) = guard.ctl().set_tx_mute(0, original.tx1_mute) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_tx_mute(guard.ctl(), original.tx1_mute, baseline_tx2) {
                    Ok(()) => eprintln!(
                        "reject-path restore verified: tx1_mute = {}",
                        original.tx1_mute
                    ),
                    Err(last) => eprintln!("reject-path restore NOT settled (last seen {last:?})"),
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    }

    // Heart-delta verify through the bounded settle poll (route (b) rule):
    // BOTH bytes — the addressed one at its target AND the untouched
    // TX2 byte at its baseline.
    match settle_tx_mute(guard.ctl(), target, baseline_tx2) {
        Ok(()) => eprintln!("write leg confirmed: tx1_mute={target} (tx2 at baseline)"),
        Err(last) => panic!(
            "heart never reported tx1_mute={target} with tx2 at baseline within the fixed settle budget (last seen: {last:?}) — guard restores the start byte on the way out"
        ),
    }

    // Restore leg = exactly the starting byte, immediately after the
    // settle poll (the muted phase is bounded). The restore's fresh GET
    // must report the byte the write leg left.
    let restore_byte = guard.original_mute;
    let restore_previous = guard
        .ctl()
        .set_tx_mute(0, restore_byte)
        .expect("restore SET failed");
    assert_eq!(
        restore_previous,
        Some(target),
        "restore leg's fresh GET must report the byte the write leg left"
    );
    match settle_tx_mute(guard.ctl(), restore_byte, baseline_tx2) {
        Ok(()) => eprintln!("restore confirmed: tx1_mute={restore_byte}"),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Shutdown timer (cmd 39 SET_SHUTDOWN_TIME) — the power-off schedule ──────

/// Bounded read-only settle poll for the timer value — same shape as
/// [`settle_tx_mute`] (route (b) owner rule): a FIXED number of fresh GETs
/// spanning about 2 s run before any heart mismatch is declared — a slow
/// heart is not a dropped write. GET-only; the SET behind this poll is
/// never retried. ⚠ The poll must NEVER be stretched to "wait the timer
/// out": the settle budget bounds every armed-countdown phase to ~2 s
/// (feature file 02 rule). `Ok(())` = a fresh GET reported the written
/// value; `Err(Some(last))` = the fixed budget ran out (`last` = the final
/// value seen); `Err(None)` = no parseable heart within the budget.
fn settle_shutdown_time(ctl: &mut HollyControl<NusbTransport>, want: u8) -> Result<(), Option<u8>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                if info.shutdown_time == want {
                    return Ok(());
                }
                last = Some(info.shutdown_time);
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the shutdown-timer test: mirrors [`TxMuteRestoreGuard`]
/// (no re-enumeration tolerance — unlike channel mode, a timer write does
/// not re-present the audio path). On any exit path (normal return, failed
/// assert, panic unwind): one fresh GET decides; IF `shutdownTime` differs
/// from the run's starting value, ONE verified restore attempt via
/// `set_shutdown_time(original)` — never a write loop. The restore is THE
/// critical path of this control (a wrongly left "15min" POWERS THE DEVICE
/// OFF after 15 minutes): if the restore cannot be VERIFIED, print a LOUD
/// message telling the owner to set the timer to Never from the phone app
/// immediately (Never is the disarm state — safe regardless of the run's
/// starting value). Errors are logged, never raised from `drop`.
struct ShutdownRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl ShutdownRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for ShutdownRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!(
                "guard: ⚠⚠ state poll failed ({e:?}) — restore CANNOT be verified; original timer was {} (0=15min, 1=Never). IF THE TIMER IS LEFT AT 15MIN, SET IT TO NEVER FROM THE PHONE APP IMMEDIATELY",
                self.original
            ),
            Ok(info) => {
                if info.shutdown_time == self.original {
                    eprintln!(
                        "guard: timer already at original ({}), nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: timer at {}, original {} — sending ONE restore SET",
                        info.shutdown_time, self.original
                    );
                    if let Err(e) = ctl.set_shutdown_time(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left with timer {} while original was {}",
                            info.shutdown_time, self.original
                        );
                    }
                    // Verify what the single restore attempt actually did —
                    // a restore that cannot be verified is the LOUD case.
                    match ctl.get_heart_info() {
                        Ok(post) if post.shutdown_time == self.original => eprintln!(
                            "guard: post-restore timer = {} — restore verified",
                            post.shutdown_time
                        ),
                        Ok(post) => eprintln!(
                            "guard: ⚠⚠ restore NOT verified (timer {} ≠ original {}) — set the timer to Never from the phone app immediately",
                            post.shutdown_time, self.original
                        ),
                        Err(e) => eprintln!(
                            "guard: ⚠⚠ post-restore GET failed ({e:?}) — restore CANNOT be verified; original was {}. IF THE TIMER IS LEFT AT 15MIN, SET IT TO NEVER FROM THE PHONE APP IMMEDIATELY",
                            self.original
                        ),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_SHUTDOWN_TIME round trip on real
/// hardware — substitute evidence for the §1 step-2 gate; on success the
/// checklist §3 Shutdown-power-off row + protocol.md §4 SET_SHUTDOWN_TIME
/// row tag `[wire] PC-originated round trip` (§5 offset 10 write==read).
///
/// ⚠ THE dangerous control: a wrongly left "15min" value POWERS THE DEVICE
/// OFF after 15 minutes. The run therefore never lets an armed countdown
/// outlive the settle poll (~2 s), never waits the timer out, and never
/// loops a write; the restore is attempted on every exit path.
///
/// Rules — the set_channel_mode_roundtrip_guarded rule set applied to the
/// timer (feature file 02, owner note: read the current value FIRST):
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta (`shutdownTime` §5 offset 10) → restore → settled verify.
///   Restore attempted on every exit path (panic and Ctrl-C included).
/// - Direction (both legs stay inside the documented space {0, 1}):
///   start 15min (0) → write Never (1) FIRST (disarms), confirm, restore
///   15min — the restore RE-ARMS the countdown at the original setting,
///   expected and reported; start Never (1) → write 15min (0), confirm,
///   restore Never immediately after the settle poll (the 15min value is
///   held ~2 s only).
/// - Refusal precondition: the fresh heart must be the certified 17-byte
///   field set, and its timer byte must sit inside {0, 1} — anything else
///   refuses the run (never guess a third value).
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the byte moved from the
///   start, ONE verified restore attempt; abort either way.
/// - The SET leg's own fresh-GET return (the pre-write timer value) is the
///   restore target; drift vs the pre-run GET retargets the guard, and the
///   restore leg's return must report the value the write leg left.
/// - usbmon (owner-executed) captures the run; the .pcap + exported .jsonl
///   land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_shutdown_time_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — a wrongly left 15min timer POWERS THE DEVICE OFF; the armed phase is held ~2 s only"]
fn set_shutdown_time_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    eprintln!(
        "original shutdown timer: shutdownTime={} (0=15min, 1=Never)",
        original.shutdown_time
    );
    eprintln!(
        "TX states: tx1={} tx2={}",
        original.tx1_status, original.tx2_status
    );

    // Direction from the fresh heart (feature file 02 + owner note):
    // write the OTHER documented value, restore the start. The 15min→Never
    // start disarms first and re-arms on the restore; the Never→15min
    // start arms for the settle window only.
    let target = match original.shutdown_time {
        0 => {
            eprintln!(
                "start = 15min: writing Never FIRST (disarms), restoring 15min at the end — the restore re-arms the countdown (expected)"
            );
            1
        }
        1 => {
            eprintln!(
                "start = Never: writing 15min (arms the countdown) — restore to Never follows IMMEDIATELY after the settle poll; the device holds 15min ~2 s only"
            );
            0
        }
        other => panic!(
            "shutdownTime {other} is outside the documented space 0/1 (15min/Never) — refusing the run"
        ),
    };

    let mut guard = ShutdownRestoreGuard {
        ctl: Some(ctl),
        original: original.shutdown_time,
    };

    // WRITE leg: the other documented value. The op's own fresh GET happens
    // inside set_shutdown_time, and the value it returns is the state in
    // force JUST before the write: the freshest restore memory the device
    // has. It wins over the pre-run poll; if another client moved the timer
    // between the two GETs, the difference is reported and the exit guard
    // retargets.
    let start = match guard.ctl().set_shutdown_time(target) {
        Ok(previous) => {
            if previous != original.shutdown_time {
                eprintln!(
                    "DRIFT: pre-run GET said {}, op's fresh GET reported {} — restore retargeted",
                    original.shutdown_time, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 / transport-failure path: NO SET retry. One
            // fresh GET decides; if the byte moved from the start, ONE
            // verified restore attempt (the guard would do it too, but the
            // probe result is logged here while the handle is alive).
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            eprintln!("timer after reject: {}", now.shutdown_time);
            let wanted = guard.original;
            if now.shutdown_time != wanted {
                eprintln!(
                    "timer moved ({} -> {}): ONE verified restore attempt",
                    wanted, now.shutdown_time
                );
                if let Err(e) = guard.ctl().set_shutdown_time(wanted) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_shutdown_time(guard.ctl(), wanted) {
                    Ok(()) => eprintln!("reject-path restore verified: timer = {}", wanted),
                    Err(last) => {
                        eprintln!("reject-path restore NOT settled (last seen {last:?})")
                    }
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule,
    // ~2 s — never stretched to wait the timer out).
    match settle_shutdown_time(guard.ctl(), target) {
        Ok(()) => eprintln!("write leg confirmed: shutdownTime={target}"),
        Err(last) => panic!(
            "heart never reported shutdownTime={target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Restore leg = exactly the starting value, immediately after the
    // settle poll (an armed countdown phase must not outlive ~2 s). The
    // restore TARGET is the SET leg's own return (the freshest restore
    // memory — see the write leg above), and the restore's fresh GET must
    // report the value the write leg left.
    let restore_previous = guard
        .ctl()
        .set_shutdown_time(start)
        .expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the value the write leg left"
    );
    match settle_shutdown_time(guard.ctl(), start) {
        Ok(()) => eprintln!("restore confirmed: shutdownTime={start}"),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Indicator light (cmd 33 SET_LIGHT) — the inverted LED toggle ────────────

/// Bounded read-only settle poll for the light byte — same shape as
/// [`settle_shutdown_time`] (route (b) owner rule): a FIXED number of fresh
/// GETs spanning about 2 s run before any heart mismatch is declared — a
/// slow heart is not a dropped write. GET-only; the SET behind this poll is
/// never retried. The comparison is in the INVERTED wire space
/// (`u8::from(!light_on)`: 0 = on / 1 = off — the documented parse
/// `lightStatus == 0` → on [code `ProGetHearInfo.java:75-76`], §5 offset
/// 11), which equals payload[11] verbatim on the documented space {0, 1};
/// abnormal bytes are refused by the run's precondition, never polled.
/// `Ok(())` = a fresh GET reported the written byte; `Err(Some(last))` =
/// the fixed budget ran out (`last` = the final byte seen); `Err(None)` =
/// no parseable heart within the budget.
fn settle_light(ctl: &mut HollyControl<NusbTransport>, want: u8) -> Result<(), Option<u8>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                let wire = u8::from(!info.light_on);
                if wire == want {
                    return Ok(());
                }
                last = Some(wire);
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the indicator-light test: mirrors [`ShutdownRestoreGuard`]
/// (no re-enumeration tolerance — an LED write does not re-present the
/// audio path). On any exit path (normal return, failed assert, panic
/// unwind): one fresh GET decides; IF the light byte differs from the run's
/// starting byte, ONE verified restore attempt via `set_light(original)`.
/// The worst a wrongly left state can do is an LED stuck on or off — the
/// lowest-stakes control in the batch — but the restore is verified the
/// same way as every other control. Errors are logged, never raised from
/// `drop`.
struct LightRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl LightRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for LightRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                let wire = u8::from(!info.light_on);
                if wire == self.original {
                    eprintln!(
                        "guard: light already at original ({}), nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: light at {}, original {} — sending ONE restore SET",
                        wire, self.original
                    );
                    if let Err(e) = ctl.set_light(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left with light byte {} while original was {}",
                            wire, self.original
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => eprintln!(
                            "guard: post-restore light byte = {} ({})",
                            u8::from(!post.light_on),
                            if post.light_on { "on" } else { "off" }
                        ),
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Phase 1B route (b): guarded SET_LIGHT round trip on real hardware —
/// substitute evidence for the §1 step-2 gate; on success the checklist §3
/// Indicator-light row + protocol.md §4 SET_LIGHT row tag `[wire]
/// PC-originated round trip` (§5 offset 11 write==read, INVERTED both
/// sides).
///
/// ℹ the lowest-risk control in the batch: a visible receiver LED only —
/// no audio effect, no re-enumeration, no dangerous phase either way
/// (unlike the shutdown timer, holding "off" is harmless). The owner's
/// extra duty is VISUAL: confirm the LED changes during the brief off
/// phase and comes back — a manual observation, reported as such, never
/// evidence.
///
/// Rules — the set_shutdown_time_roundtrip_guarded rule set applied to the
/// light (feature file 04; cmd 33 is NOT on the AGENTS.md excluded list —
/// no lift needed):
///   fresh GET → SET → ACK gate (`data[0]==0`) → bounded settle-verified
///   heart delta (lightStatus §5 offset 11, wire space 0=on/1=off) →
///   restore → settled verify. Restore attempted on every exit path.
/// - Direction (both legs stay inside the documented space {0, 1}): write
///   the OTHER documented value (toggle), restore the start — the op is
///   IDENTITY on that space (BOTH APK sides carry the same inversion
///   [code `HLDevice6302Activity.java:662`, `ProGetHearInfo.java:75-76`]),
///   so no inverted-write translation happens here.
/// - Refusal precondition: the fresh heart must be the certified 17-byte
///   field set, and its light byte must sit inside {0, 1} — anything else
///   refuses the run (never guess a third value).
/// - ACK ≠ 0: NO SET retry — one fresh GET; if the byte moved from the
///   start, ONE verified restore attempt; abort either way.
/// - The SET leg's own fresh-GET return (the pre-write light byte,
///   payload[11] verbatim) is the restore target; drift vs the pre-run
///   GET retargets the guard, and the restore leg's return must report
///   the byte the write leg left.
/// - usbmon (owner-executed) captures the run; the .pcap + exported .jsonl
///   land in captures/ as the evidence files.
///
/// Run only at the owner's explicit go, usbmon running:
///   cargo test -p holly-usb --test hardware -- set_light_roundtrip_guarded --ignored --nocapture
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — watch the receiver LED during the brief off phase (visual confirmation, reported as such)"]
fn set_light_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");
    let original_byte = original.raw[11];
    eprintln!(
        "original indicator light: byte={} ({}) — watch the receiver LED; the off phase is brief and the restore brings it back",
        original_byte,
        if original.light_on { "on" } else { "off" }
    );

    // Direction from the fresh heart: write the OTHER documented value
    // (toggle), restore the start. Abnormal bytes refuse the run.
    let target = match original_byte {
        0 => {
            eprintln!("start = ON (0): writing OFF (1), restoring ON at the end");
            1
        }
        1 => {
            eprintln!("start = OFF (1): writing ON (0), restoring OFF at the end");
            0
        }
        other => panic!(
            "lightStatus {other} is outside the documented space 0/1 (on/off, INVERTED) — refusing the run"
        ),
    };

    let mut guard = LightRestoreGuard {
        ctl: Some(ctl),
        original: original_byte,
    };

    // WRITE leg: the other documented value. The op's own fresh GET happens
    // inside set_light, and the byte it returns is the state in force JUST
    // before the write (payload[11] verbatim — see set_light's return
    // doc): the freshest restore memory the device has. It wins over the
    // pre-run poll; if another client moved the light between the two
    // GETs, the difference is reported and the exit guard retargets.
    let start = match guard.ctl().set_light(target) {
        Ok(previous) => {
            if previous != original_byte {
                eprintln!(
                    "DRIFT: pre-run GET said byte {}, op's fresh GET reported byte {} — restore retargeted",
                    original_byte, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 / transport-failure path: NO SET retry. One
            // fresh GET decides; if the byte moved from the start, ONE
            // verified restore attempt (the guard would do it too, but the
            // probe result is logged here while the handle is alive).
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            let now_wire = u8::from(!now.light_on);
            eprintln!(
                "light after reject: byte={now_wire} ({})",
                if now.light_on { "on" } else { "off" }
            );
            let wanted = guard.original;
            if now_wire != wanted {
                eprintln!(
                    "light byte moved ({} -> {}): ONE verified restore attempt",
                    wanted, now_wire
                );
                if let Err(e) = guard.ctl().set_light(wanted) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_light(guard.ctl(), wanted) {
                    Ok(()) => eprintln!("reject-path restore verified: byte = {}", wanted),
                    Err(last) => {
                        eprintln!("reject-path restore NOT settled (last seen {last:?})")
                    }
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule,
    // ~2 s — the off phase stays brief either direction).
    match settle_light(guard.ctl(), target) {
        Ok(()) => eprintln!(
            "write leg confirmed: light byte={target} ({})",
            if target == 0 { "on" } else { "off" }
        ),
        Err(last) => panic!(
            "heart never reported light byte={target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Restore leg = exactly the starting byte, immediately after the
    // settle poll. The restore TARGET is the SET leg's own return (the
    // freshest restore memory — see the write leg above), and the
    // restore's fresh GET must report the byte the write leg left.
    let restore_previous = guard.ctl().set_light(start).expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the byte the write leg left"
    );
    match settle_light(guard.ctl(), start) {
        Ok(()) => eprintln!(
            "restore confirmed: light byte={start} ({})",
            if start == 0 { "on" } else { "off" }
        ),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}

// ── Mic recognition (cmd 35 SET_TX_IDENTIFY) — the both-mics-gated toggle ───

/// Bounded read-only settle poll for the identify byte — same shape as
/// [`settle_light`] (route (b) owner rule): a FIXED number of fresh GETs
/// spanning about 2 s run before any heart mismatch is declared — a slow
/// heart is not a dropped write. GET-only; the SET behind this poll is
/// never retried. The comparison is in the IDENTITY wire space
/// (`u8::from(tx_identify)`: 0 = off / 1 = on — the documented parse
/// `txIdentifyStatus == 1` [code `ProGetHearInfo.java:88`], §5 offset 15),
/// which equals payload[15] verbatim on the documented space {0, 1};
/// abnormal bytes are refused by the run's precondition, never polled.
/// `Ok(())` = a fresh GET reported the written byte; `Err(Some(last))` =
/// the fixed budget ran out (`last` = the final byte seen); `Err(None)` =
/// no parseable heart within the budget.
fn settle_tx_identify(ctl: &mut HollyControl<NusbTransport>, want: u8) -> Result<(), Option<u8>> {
    /// Fixed settle budget: 4 fresh GETs ≈ 2 s — deterministic, not
    /// adaptive; nothing beyond this count.
    const POLLS: u32 = 4;
    let mut last = None;
    for poll in 0..POLLS {
        if poll > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        match ctl.get_heart_info() {
            Ok(info) => {
                let wire = u8::from(info.tx_identify);
                if wire == want {
                    return Ok(());
                }
                last = Some(wire);
            }
            Err(e) => eprintln!("settle poll {poll:>2}/{POLLS}: GET failed: {e:?}"),
        }
    }
    Err(last)
}

/// Drop guard for the mic-recognition test: mirrors [`LightRestoreGuard`]
/// (no re-enumeration tolerance — an identify toggle does not re-present
/// the audio path). On any exit path (normal return, failed assert, panic
/// unwind): one fresh GET decides; IF the identify byte differs from the
/// run's starting byte, ONE verified restore attempt via
/// `set_tx_identify(original)`. The feature file's rule is absolute:
/// recognition is never left on after the test — starting off ends off,
/// starting on ends on. Errors are logged, never raised from `drop`.
struct IdentifyRestoreGuard {
    ctl: Option<HollyControl<NusbTransport>>,
    original: u8,
}

impl IdentifyRestoreGuard {
    fn ctl(&mut self) -> &mut HollyControl<NusbTransport> {
        self.ctl
            .as_mut()
            .expect("guard.ctl() may be called only while Some")
    }
}

impl Drop for IdentifyRestoreGuard {
    fn drop(&mut self) {
        let Some(ctl) = self.ctl.as_mut() else {
            return;
        };
        // Restore decisions come only from a fresh GET, never cached state.
        match ctl.get_heart_info() {
            Err(e) => eprintln!("guard: state poll failed, cannot verify restore: {e:?}"),
            Ok(info) => {
                let wire = u8::from(info.tx_identify);
                if wire == self.original {
                    eprintln!(
                        "guard: identify already at original ({}), nothing to do",
                        self.original
                    );
                } else {
                    eprintln!(
                        "guard: identify at {}, original {} — sending ONE restore SET",
                        wire, self.original
                    );
                    if let Err(e) = ctl.set_tx_identify(self.original) {
                        eprintln!(
                            "guard: RESTORE FAILED ({e:?}) — device left with identify byte {} while original was {}",
                            wire, self.original
                        );
                    }
                    // Verify what the single restore attempt actually did.
                    match ctl.get_heart_info() {
                        Ok(post) => eprintln!(
                            "guard: post-restore identify byte = {} ({})",
                            u8::from(post.tx_identify),
                            if post.tx_identify { "on" } else { "off" }
                        ),
                        Err(e) => eprintln!("guard: post-restore GET failed: {e:?}"),
                    }
                }
            }
        }
    }
}

/// Mic recognition round trip, guarded (route (b), owner lift 2026-11-12 —
/// AGENTS.md: identify (35) lifted for mic recognition on/off only).
///
/// HARD START GATE (feature 03 rule): the run refuses to begin unless BOTH
/// transmitters report connected (heart offsets 0/1) — the same
/// precondition the APK applies at UI level
/// (`HLDevice6302Activity.java:805-812`). The device-side behaviour with a
/// mic missing is never probed: no test here deliberately sends the
/// command with one TX down.
///
/// Preconditions: both TX connected, state off/on (either start is fine —
/// the direction used is printed). Write the OTHER documented value
/// (toggle), confirm via the settle poll that §5 offset 15 equals the
/// written byte, restore the start, verify. Never leaves recognition on:
/// starting off writes on (LED colour change = the owner's physical
/// confirmation) and restores off; starting on writes off and restores on.
///
/// ```text
/// cargo test -p holly-usb --test hardware -- set_tx_identify_roundtrip_guarded --ignored --nocapture
/// ```
#[test]
#[ignore = "mutates device state; owner-gated run with usbmon capture active — watch the transmitter LEDs during the brief on phase (colour change = the physical confirmation); REFUSES to run unless both mics are connected"]
fn set_tx_identify_roundtrip_guarded() {
    let _dev = device_lock();
    let mut ctl = HollyControl::open().expect("open + claim IF0");
    let original = ctl.get_heart_info().expect("state poll failed");
    assert_eq!(original.raw.len(), 17, "certified 17-byte payload shape");

    // The hard start gate, BEFORE anything is sent (feature 03 rule —
    // mirrors HLDevice6302Activity.java:805-812 at UI level). Refusal is
    // loud and names the fix.
    assert!(
        original.raw[0] == 1 && original.raw[1] == 1,
        "BOTH transmitters must be connected for the identify run (heart offsets 0/1: tx1={}, tx2={}) — refusing; power both mics, let them connect, and rerun",
        original.raw[0],
        original.raw[1]
    );

    let original_byte = original.raw[15];
    eprintln!(
        "original mic recognition: byte={} ({}) — watch the transmitter LEDs; the colour change during the brief on phase is the physical confirmation",
        original_byte,
        if original.tx_identify { "on" } else { "off" }
    );

    // Direction from the fresh heart: write the OTHER documented value
    // (toggle), restore the start. Recognition is NEVER left on after the
    // test. Abnormal bytes refuse the run.
    let target = match original_byte {
        0 => {
            eprintln!("start = OFF (0): writing ON (1), restoring OFF at the end");
            1
        }
        1 => {
            eprintln!("start = ON (1): writing OFF (0), restoring ON at the end");
            0
        }
        other => panic!(
            "txIdentifyStatus {other} is outside the documented space 0/1 (off/on, identity) — refusing the run"
        ),
    };

    let mut guard = IdentifyRestoreGuard {
        ctl: Some(ctl),
        original: original_byte,
    };

    // WRITE leg: the other documented value. The op's own fresh GET happens
    // inside set_tx_identify, and the byte it returns is the state in force
    // JUST before the write (payload[15] verbatim — see set_tx_identify's
    // return doc): the freshest restore memory the device has. It wins over
    // the pre-run poll; if another client moved the byte between the two
    // GETs, the difference is reported and the exit guard retargets.
    let start = match guard.ctl().set_tx_identify(target) {
        Ok(previous) => {
            if previous != original_byte {
                eprintln!(
                    "DRIFT: pre-run GET said byte {}, op's fresh GET reported byte {} — restore retargeted",
                    original_byte, previous
                );
                guard.original = previous;
            }
            previous
        }
        Err(e) => {
            // Route (b) ACK ≠ 0 / transport-failure path: NO SET retry. One
            // fresh GET decides; if the byte moved from the start, ONE
            // verified restore attempt (the guard would do it too, but the
            // probe result is logged here while the handle is alive).
            eprintln!("SET failed: {e:?} — probing once before the single restore");
            let now = guard
                .ctl()
                .get_heart_info()
                .expect("post-reject GET failed");
            let now_wire = u8::from(now.tx_identify);
            eprintln!(
                "identify after reject: byte={now_wire} ({})",
                if now.tx_identify { "on" } else { "off" }
            );
            let wanted = guard.original;
            if now_wire != wanted {
                eprintln!(
                    "identify byte moved ({} -> {}): ONE verified restore attempt",
                    wanted, now_wire
                );
                if let Err(e) = guard.ctl().set_tx_identify(wanted) {
                    eprintln!("reject-path restore SET failed: {e:?}");
                }
                match settle_tx_identify(guard.ctl(), wanted) {
                    Ok(()) => eprintln!("reject-path restore verified: byte = {}", wanted),
                    Err(last) => {
                        eprintln!("reject-path restore NOT settled (last seen {last:?})")
                    }
                }
            }
            panic!("SET rejected (ack gate) — aborting with no retry per route (b)");
        }
    };

    // Heart-delta verify through the bounded settle poll (route (b) rule,
    // ~2 s — the on phase stays brief either direction).
    match settle_tx_identify(guard.ctl(), target) {
        Ok(()) => eprintln!(
            "write leg confirmed: identify byte={target} ({})",
            if target == 1 { "on" } else { "off" }
        ),
        Err(last) => panic!(
            "heart never reported identify byte={target} within the fixed settle budget (last seen: {last:?}) — guard restores the original on the way out"
        ),
    }

    // Restore leg = exactly the starting byte, immediately after the
    // settle poll. The restore TARGET is the SET leg's own return (the
    // freshest restore memory — see the write leg above), and the
    // restore's fresh GET must report the byte the write leg left.
    let restore_previous = guard
        .ctl()
        .set_tx_identify(start)
        .expect("restore SET failed");
    assert_eq!(
        restore_previous, target,
        "restore leg's fresh GET must report the byte the write leg left"
    );
    match settle_tx_identify(guard.ctl(), start) {
        Ok(()) => eprintln!(
            "restore confirmed: identify byte={start} ({}) — recognition back at the start state, never left on",
            if start == 1 { "on" } else { "off" }
        ),
        Err(last) => panic!(
            "restore not settled within budget (last seen: {last:?}) — guard tries once more on exit"
        ),
    }
}
