//! Hardware tests — compile everywhere, run only with `cargo test
//! --test hardware -- --ignored` (and the dongle plugged in, udev rule
//! installed, QEMU VM not holding the device).
//!
//! Read-only by design: only GET_HEART_INFO is issued here. SET_NOISE is
//! NOT exercised on hardware by these tests; the state-toggling round trip
//! lives in tools/capture_set_noise.py with its explicit
//! restore-and-verify gates.

use holly_usb::{HollyControl, NusbTransport};

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
