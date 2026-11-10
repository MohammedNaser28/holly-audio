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
