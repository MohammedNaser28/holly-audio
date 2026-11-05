//! CI tests: certified wire vectors from captures/, replayed through
//! [`MockTransport`]. No hardware is touched. Byte arrays below are the exact
//! 64-byte transfer buffers from:
//!   captures/2026-11-05-set-noise.txt           (E1-E5, session 2)
//!   captures/2026-11-05-hearinfo-first-wire.txt (session 1)

use holly_usb::{Error, HollyControl, MockTransport, cmd};

const HEART_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x1F, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// E1 RX: baseline heart reply (NC on, TX2 connected, battery 0x5D = 93).
const HEART_RX_E1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// E3 RX: identical payload except offset 6 — noiseSwitch 01 -> 00.
const HEART_RX_E3: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x00, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// E2 TX: SET_NOISE { isOpen: 0, level: 0 }, byte-exact (Task 1 capture).
const SET_OFF_TX_E2: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x13, 0x00, 0x02, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// E2/E4 RX: SET_NOISE ack — one status byte 0x00, residue 0x75 behind it.
const SET_ACK_E2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x13, 0x00, 0x01, 0x00, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn control(mock: MockTransport) -> HollyControl<MockTransport> {
    HollyControl::new(mock)
}

/// GET path: TX frame byte-exact to E1's TX, reply parse matches the certified
/// field semantics (tx1 off, tx2 on, battery 93, NC on, level 0).
#[test]
fn get_heart_info_roundtrip_wire() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut c = control(mock);

    let info = c.get_heart_info().expect("E1 reply must parse");
    assert!(info.noise_switch);
    assert_eq!(info.noise_level, 0);
    assert!(!info.tx1_status);
    assert!(info.tx2_status);
    assert_eq!(info.tx1_battery, 0);
    assert_eq!(info.tx2_battery, 0x5D);
    assert!(info.light_on); // payload[11] == 0 (inverted per APK)
    assert_eq!(info.eq_level, 3);
    assert_eq!(info.reverb_level, 2);
    // multi-TX tail absent in the certified 17-byte shape
    assert_eq!(info.tx3_status, None);
    assert_eq!(info.tx4_mute, None);

    // TX byte-exactness: the poll frame we wrote must equal capture E1's TX.
    assert_eq!(c.transport().written(), &[HEART_TX]);
}

/// SET path end-to-end: set_noise(false) after a state fetch must emit E2's
/// exact TX bytes and accept E2's exact ACK bytes.
#[test]
fn set_noise_off_roundtrip_wire() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // state fetch (level 0)
    mock.push_reply(SET_ACK_E2); // E2 ack
    let mut c = control(mock);

    c.set_noise(false).expect("ack 00 must be accepted");

    let written = c.transport().written();
    // first write: the GET poll, second: the SET frame
    assert_eq!(&written[1], &SET_OFF_TX_E2, "SET TX must be byte-exact E2");
}

/// set_noise(true) mirrors E4's TX with isOpen=01 (level from device state).
#[test]
fn set_noise_on_tx_matches_e4() {
    // E4 TX = E2 TX with byte 7 = 0x01.
    let mut set_on_tx = SET_OFF_TX_E2;
    set_on_tx[7] = 0x01;

    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    mock.push_reply(SET_ACK_E2);
    let mut c = control(mock);

    c.set_noise(true).expect("ack 00 must be accepted");
    assert_eq!(&c.transport().written()[1], &set_on_tx);
}

/// Nonzero (never-captured-shape) ack status must not be treated as success
/// (`ProSetNoise.receiveData`: only data[0] == 0 signals setStatus ok).
#[test]
fn set_noise_rejects_nonzero_ack() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_ACK_E2;
    ack[7] = 0x01; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);

    let err = c.set_noise(false).expect_err("nonzero ack must fail");
    assert!(matches!(err, Error::Rejected { status: 0x01 }), "{err:?}");
}

/// ack payload with a length other than 1 is rejected (no shape assumption):
/// a zero-byte ack and a 2-byte "value echo" ack would both be inventions.
#[test]
fn set_noise_rejects_malformed_ack_lengths() {
    for len in [0usize, 2] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        let mut ack = SET_ACK_E2;
        // len bytes starting at payload offset 7 (7+len area)
        ack[6] = len as u8; // payload length field (BE low byte)
        mock.push_reply(ack);
        let mut c = control(mock);

        let err = c.set_noise(false).expect_err("wrong ack shape must fail");
        assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
    }
}

/// Command-echo abort gate (capture tool behavior): reply with the wrong cmd.
#[test]
fn cmd_echo_mismatch_aborts() {
    let mut mock = MockTransport::new();
    let mut bad = HEART_RX_E1;
    bad[4] = cmd::SET_NOISE; // device "echoed" the wrong command
    mock.push_reply(bad);
    let mut c = control(mock);

    let err = c.get_heart_info().expect_err("echo mismatch must abort");
    assert!(
        matches!(
            err,
            Error::CmdMismatch {
                got: 0x13,
                want: 0x1F
            }
        ),
        "{err:?}"
    );
}

/// Host-direction echo (AA) in a reply never happens on the wire; treat it as
/// an abort just like the unknown-magic case.
#[test]
fn aa_direction_reply_aborts() {
    let mut mock = MockTransport::new();
    let mut aa = SET_ACK_E2;
    aa[2] = 0xAA;
    mock.push_reply(aa);
    let mut c = control(mock);

    let err = c.get_heart_info().expect_err("AA reply must abort");
    assert!(
        matches!(err, Error::UnknownDirection { found: 0xAA }),
        "{err:?}"
    );
}

/// Short heart payload: the app's own gate needs 17 bytes for signalMode;
/// anything shorter is malformed (abort rule, no synthesized defaults).
#[test]
fn short_heart_payload_aborts() {
    let mut mock = MockTransport::new();
    let mut short = HEART_RX_E1;
    short[6] = 16; // len field = 16 < 17
    mock.push_reply(short);
    let mut c = control(mock);

    let err = c.get_heart_info().expect_err("16-byte payload must abort");
    assert!(matches!(err, Error::HeartInfoShort { len: 16 }), "{err:?}");
}

/// Toggle congruence on the wire vectors: reading E1 then E3 flips ONLY
/// noise_switch — every other parsed field is unchanged (Task 1's certified
/// state-change property, now asserted through the typed struct).
#[test]
fn heart_toggle_flips_only_noise_switch() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    mock.push_reply(HEART_RX_E3);
    let mut c = control(mock);

    let on = c.get_heart_info().expect("E1 reply must parse");
    let off = c.get_heart_info().expect("E3 reply must parse");

    assert!(on.noise_switch);
    assert!(!off.noise_switch);
    assert_eq!(on.noise_level, off.noise_level);
    assert_eq!(on.tx2_battery, off.tx2_battery);
    assert_eq!(on.eq_level, off.eq_level);
    assert_eq!(on.reverb_level, off.reverb_level);
    assert_eq!(on.signal_mode, off.signal_mode);
    assert_eq!(on.raw.len(), off.raw.len());
}

/// SET_NOISE preserves the noise level byte from the device's own state —
/// never a made-up value: after a 17-byte state poll with level 3, the SET
/// payload's second byte must be that 3.
#[test]
fn set_noise_level_comes_from_device_state() {
    let mut mock = MockTransport::new();
    let mut state = HEART_RX_E1;
    state[14] = 0x03; // payload[7] = noiseLevel = 3
    mock.push_reply(state);
    mock.push_reply(SET_ACK_E2);
    let mut c = control(mock);

    c.set_noise(false).expect("ack must be accepted");
    let written = c.transport().written();
    let set_tx = &written[written.len() - 1];
    assert_eq!(set_tx[4], cmd::SET_NOISE);
    assert_eq!(set_tx[5], 0x00);
    assert_eq!(set_tx[6], 0x02);
    assert_eq!(set_tx[7], 0x00); // isOpen = off
    assert_eq!(set_tx[8], 0x03); // level, straight from the heart reply
    assert_eq!(set_tx[9], 0xEF);
}

/// Task-4a stale-cache regression: an *earlier* `get_heart_info` result must
/// never leak into a later SET frame. Script: GET reply A (level 0), then a
/// different, newer GET reply B (level 3), then the ack. `set_noise` must
/// issue its own GET (reply B) and take the level from that fresh reply —
/// under the removed cache the SET would have carried A's level 0.
#[test]
fn set_noise_uses_fresh_state_not_cached() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // reply A: level 0, NC on
    let mut state_b = HEART_RX_E1;
    state_b[13] = 0x00; // payload[6] = noiseSwitch 01 -> 00
    state_b[14] = 0x03; // payload[7] = noiseLevel 00 -> 03
    mock.push_reply(state_b); // reply B: the state the SET must be built from
    mock.push_reply(SET_ACK_E2);

    let mut c = control(mock);
    // Older call: caches level 0 under the pre-4a implementation.
    let cached = c.get_heart_info().expect("reply A must parse");
    assert!(cached.noise_switch && cached.noise_level == 0);

    c.set_noise(false).expect("ack must be accepted");
    let written = c.transport().written();
    // writes: poll A, poll B (fresh, inside set_noise), SET frame.
    let set_tx = &written[written.len() - 1];
    assert_eq!(set_tx[4], cmd::SET_NOISE);
    assert_eq!(set_tx[7], 0x00); // isOpen = off
    assert_eq!(
        set_tx[8], 0x03,
        "SET level must come from the fresh GET (reply B), not the cached reply A"
    );
}
