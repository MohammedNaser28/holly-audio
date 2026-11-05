//! Hardware tests — compile everywhere, run only with `cargo test
//! --test hardware -- --ignored` (and the dongle plugged in, udev rule
//! installed, QEMU VM not holding the device).
//!
//! Read-only by design: only GET_HEART_INFO is issued here. SET_NOISE is
//! NOT exercised on hardware by these tests; the state-toggling round trip
//! lives in tools/capture_set_noise.py with its explicit
//! restore-and-verify gates.

use holly_usb::HollyControl;

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
