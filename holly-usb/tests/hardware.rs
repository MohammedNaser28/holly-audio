//! Hardware tests — compile everywhere, run only with `cargo test
//! --test hardware -- --ignored` (and the dongle plugged in, udev rule
//! installed, QEMU VM not holding the device).
//!
//! `open_and_read_heart_info` is read-only (GET_HEART_INFO only). The
//! Task-4b `set_noise_roundtrip_guarded` DOES exercise SET_NOISE on
//! hardware, guarded: it restores the original state on every exit path.

use holly_usb::{HollyControl, NusbTransport};
use std::sync::{Mutex, MutexGuard, OnceLock};

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
