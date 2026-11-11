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

/// Live RX (2026-11-05, session 3): the phone app set the A6108 NC level from
/// strong (2) to medium (1) offline-side and the dongle was replugged — the
/// GET now answers with `noise_level = 1`, `noise_switch = 01`, battery
/// 0x63 = 99. Nonzero level byte on the [wire]:
/// captures/2026-11-05-noise-level-medium.txt. Residue 0x08 (third distinct
/// heart residue; docs/protocol.md open question 2).
const HEART_RX_LEVEL_MED: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x63, 0x00, 0x00, 0x01, 0x01, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
    assert!(
        matches!(err, Error::Rejected { status: 0x01, .. }),
        "{err:?}"
    );
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
fn set_noise_carries_devices_current_level() {
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

/// The certified level-1 heart reply (session 3, live) parses with
/// noise_level = 1 and every other field in its certified slots.
#[test]
fn heart_level_med_reply_parses() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_LEVEL_MED);
    let mut c = control(mock);

    let info = c.get_heart_info().expect("level-1 reply must parse");
    assert!(info.noise_switch);
    assert_eq!(info.noise_level, 1);
    assert_eq!(info.tx2_battery, 0x63); // 0x63 = 99
    assert_eq!(info.voice_level, 3);
    assert_eq!(info.eq_level, 3);
    assert_eq!(info.reverb_level, 2);
}

/// set_noise_level(1) (medium) against the live level-1 state (`isOpen` from
/// the fresh GET = 01): TX must be the APK-computed medium frame
/// `05 03 AA DD 13 00 02 01 01 EF` [code UsbMessengerV2.java:172-173 with the
/// level buttons in HLDevice6302Activity.java:1610], byte-exact to the
/// shape E2 established [wire].
#[test]
fn set_noise_level_medium_roundtrip() {
    const SET_MED_TX: [u8; 64] = [
        0x05, 0x03, 0xAA, 0xDD, 0x13, 0x00, 0x02, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];

    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_LEVEL_MED); // state fetch (switch on, level 1)
    mock.push_reply(SET_ACK_E2); // ack (status byte, residue shape [wire E2])
    let mut c = control(mock);

    c.set_noise_level(1).expect("ack 00 must be accepted");

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(&written[1], &SET_MED_TX, "level SET must be the APK frame");
}

/// Strong (level 2) is the APK's third value (HLDevice6302Activity.java:1593);
/// with the same switch-on state the only TX difference is the level byte.
#[test]
fn set_noise_level_strong_deltas_only_level_byte() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_LEVEL_MED);
    mock.push_reply(SET_ACK_E2);
    let mut c = control(mock);

    c.set_noise_level(2).expect("ack 00 must be accepted");
    let set_tx = &c.transport().written()[1];
    assert_eq!(set_tx[4], cmd::SET_NOISE);
    assert_eq!(set_tx[6], 0x02);
    assert_eq!(set_tx[7], 0x01); // switch preserved from the fresh GET
    assert_eq!(set_tx[8], 0x02); // strong
}

/// Freshness rule parity with set_noise: `isOpen` comes from the reply issued
/// INSIDE set_noise_level, never from an earlier poll. Script replies A
/// (switch on) then B (switch off, level 0), then the ack; the SET frame must
/// carry B's isOpen.
#[test]
fn set_noise_level_uses_fresh_switch_state() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // A: switch on, level 0
    mock.push_reply(HEART_RX_E3); // B: switch off, level 0
    mock.push_reply(SET_ACK_E2);
    let mut c = control(mock);

    let stale = c.get_heart_info().expect("reply A must parse");
    assert!(stale.noise_switch);

    c.set_noise_level(2).expect("ack must be accepted");
    let set_tx = &c.transport().written()[2]; // poll A, poll B, SET
    assert_eq!(set_tx[7], 0x00, "isOpen must come from reply B, not A");
    assert_eq!(set_tx[8], 0x02);
}

/// `set_noise_level` shares set_noise's ack gates: nonzero status = Rejected,
/// wrong ack length = BadAck. Reason: the ack shape has only ever been seen
/// as `[00]` [wire E2/E4]; treating anything else as success would invent.
#[test]
fn set_noise_level_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_LEVEL_MED);
    let mut ack = SET_ACK_E2;
    ack[7] = 0x02;
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_noise_level(1).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x02, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_LEVEL_MED);
    let mut ack = SET_ACK_E2;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_noise_level(1).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── EQ (cmd 7, SET_EQ) — Phase 1B route (b) ─────────────────────────────────
// Owner rule (2026-11-09, docs/gui-roadmap.md §1(b)): Mock vectors are
// RECOMPUTED FROM THE DOCS until the hardware run; real usbmon frames join
// as replay vectors after it. Every frame below is computed from docs only:
//   frame template       docs/protocol.md §2 (ProtocolV2.getData [code])
//   cmd 7, payload       docs/set-gate-checklist.md §3 EQ row ([{value}])
//   eqLevel map 1..=3    docs/protocol.md §5 + §5.2 (= Low/Bright/
//                        "Equalization" [HLDevice6302Activity.java:729-749])
// The heart reply is the wire-certified E1 heart (captures/
// 2026-11-05-hearinfo-first-wire.txt, which already rests at eqLevel 3)
// with only the value byte moved — the 17-byte heart layout is [wire].

/// SET_EQ of pad 2 (BRIGHT) recomputed from the docs:
/// `05 03 AA DD 07 00 01 02 EF` + zero padding to 64 (big-endian len 00 01).
const SET_EQ_2_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x07, 0x00, 0x01, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_EQ ack recomputed from the docs: reply header `05 03 BB DD` + cmd
/// echo 07 + len 00 01 + status `00` (checklist §3 ACK rule: one status
/// byte, `data[0] == 0` = success — uniform across `ProSet*`). The byte
/// behind the payload follows the reply checksum rule
/// XOR(frame[0..7+len)) ⊕ 0x07 — for cmd 07 that is `0x61`, matching the
/// observed wire ack byte-for-byte (`SET_EQ_ACK_WIRE`, replay test) and
/// NOT the NC family's 0x75 (that goes with cmd 13). The byte stays
/// UNINTERPRETED by the crate — it must ignore everything past `7+len`,
/// so the vector still doubles as an ignore-the-residue regression.
const SET_EQ_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x07, 0x00, 0x01, 0x00, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with eqLevel (payload offset 12 = frame byte 19)
/// moved 3 → 2: layout stays [wire] (E1); the flipped value byte is the
/// doc-recomputed delta side of route (b), and the residue drops to 0x36
/// per the reply checksum rule (XOR(payload[0..16)) ⊕ 0x68 — the LAST
/// payload byte, signalMode, is NOT in the fold: 2026-11-11 correction,
/// signalMode was 0 in E1 so both readings give the same byte; value 03^02
/// flips that fold, 0x5F⊕0x68=0x37 → 0x5E⊕0x68=0x36;
/// tools/residue_check.py verifies both).
const HEART_RX_EQ2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_eq(2) ("Bright" — the neighbouring pad of the certified rest state
/// 3): the op must issue its own fresh GET first (uniform SET discipline),
/// then the byte-exact doc-recomputed SET frame, and hand back the fresh
/// GET's pad as the pre-write state.
#[test]
fn set_eq_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (device rests at eq 3)
    mock.push_reply(SET_EQ_ACK);
    let mut c = control(mock);

    let previous = c.set_eq(2).expect("ack 00 must be accepted");
    assert_eq!(
        previous, 3,
        "fresh GET inside set_eq = the pad before write"
    );

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(
        &written[1], &SET_EQ_2_TX,
        "SET EQ must be the doc-recomputed pad-2 frame"
    );
}

/// The identity value space (1 = OVERCAST/"Low", 2 = BRIGHT, 3 = BALANCE/
/// "Equalization" — checklist §3, `HLDevice6302Activity.java:729/:739/:749`):
/// every SET differs from the pad-2 frame in the value byte only.
#[test]
fn set_eq_value_space_changes_only_the_value_byte() {
    for value in [1u8, 3] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        mock.push_reply(SET_EQ_ACK);
        let mut c = control(mock);

        let previous = c.set_eq(value).expect("ack 00 must be accepted");
        assert_eq!(previous, 3, "fresh GET reported pad {value} INCOMING");
        let mut want = SET_EQ_2_TX;
        want[7] = value; // value byte sits at frame offset 7 (tail EF at 8)
        assert_eq!(&c.transport().written()[1], &want, "pad {value}");
    }
}

/// Full doc-vector round trip (route (b), Mock half): device rests at
/// eqLevel 3 ("Equalization" — E1 IS that state [wire]), write the
/// neighbouring pad 2 ("Bright"), ack `00`, heart settles eqLevel → 2
/// (§5 offset 12); restore pad 3, heart settles back. Frames asserted both
/// ways round trip; the restore TARGET comes from set_eq's own return
/// (the fresh GET inside the write), not a caller-side poll.
#[test]
fn set_eq_roundtrip_doc_vectors() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner GET of set_eq(2): device at 3
    mock.push_reply(SET_EQ_ACK); // ack for SET_EQ [02]
    mock.push_reply(HEART_RX_EQ2); // post-SET fresh GET: delta = 2
    mock.push_reply(HEART_RX_EQ2); // inner GET of set_eq(prev): device at 2
    mock.push_reply(SET_EQ_ACK); // ack for SET_EQ [03]
    mock.push_reply(HEART_RX_E1); // post-restore fresh GET: delta = 3

    let mut c = control(mock);
    let previous = c.set_eq(2).expect("EQ write must be accepted");
    assert_eq!(previous, 3, "pre-write pad from the op's own fresh GET");
    let after = c.get_heart_info().expect("post-SET GET");
    assert_eq!(after.eq_level, 2, "heart delta must equal the written pad");
    let restore_previous = c.set_eq(previous).expect("restore must be accepted");
    assert_eq!(restore_previous, 2, "fresh GET before restore saw pad 2");
    let restored = c.get_heart_info().expect("post-restore GET");
    assert_eq!(restored.eq_level, 3, "heart must be back at the start pad");

    let written = c.transport().written();
    let mut set3 = SET_EQ_2_TX;
    set3[7] = previous; // value byte at frame offset 7 (tail EF at 8)
    assert_eq!(&written[1], &SET_EQ_2_TX, "write leg is pad 2");
    assert_eq!(&written[4], &set3, "restore leg is set_eq's OWN return");
}

/// Ack gates shared with the SET_NOISE family (checklist §3: uniform
/// `data[0] == 0` success across all `ProSet*` classes): nonzero status =
/// [`Error::Rejected`], wrong ack length = [`Error::BadAck`]. Reason same
/// as `set_noise_level`: the ack shape has only ever been seen as `[00]`
/// [wire E2/E4].
#[test]
fn set_eq_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_EQ_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_eq(2).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_EQ_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_eq(2).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── Phase 1B route (b): REAL wire frames join as replay vectors ─────────────
// Full 64-byte buffers from the owner-executed hardware run of
// set_eq_roundtrip_guarded, captured 2026-11-09 (captures/raw/
// set_eq_roundtrip.pcap → captures/set_eq_roundtrip.jsonl, bus 6 dev 2).
// Owner hand-check + frame numbers from the export:
//   #17/#21        heart, eqLevel 3, battery 0x54, residue byte 3C
//   #22            SET_EQ `05 03 AA DD 07 00 01 02 EF`
//   #25/#37        ack `05 03 BB DD 07 00 01 00 61` (status 00)
//   #29/#33        heart, eqLevel 2, byte 3D
//   #34            SET_EQ restore `05 03 AA DD 07 00 01 03 EF`
//   #41/#45        heart, eqLevel 3, byte 3C
// The byte AFTER the payload (frame 24) follows the reply checksum rule
// XOR(payload[0..len-1)) ⊕ 0x68 — the LAST payload byte (signalMode, §5
// offset 16) is NOT in the fold. Verified on EVERY reply captured so far
// (14 hearts over 5 sessions at salt 0x68 incl. the 2026-11-11 reverb run
// whose signalMode=1 hearts first split the two readings, plus 4 ACK
// shapes over the whole 7+len prefix at salt 0x07) by
// `python3 tools/residue_check.py` [wire]; the crate ignores the byte
// (past 7+len), which the replay below also proves.

/// Real SET_EQ pad-2 frame of the run — byte-identical to the
/// doc-recomputed [`SET_EQ_2_TX`] (cross-checked in the replay test).
const SET_EQ_2_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x07, 0x00, 0x01, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_EQ pad-3 restore frame (export frame 34).
const SET_EQ_3_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x07, 0x00, 0x01, 0x03, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at eqLevel 3 (export frame 17; frames 21/41/45
/// identical): rest state — TX1 connected 0x54 = 84, TX2 disconnected, NC on
/// level 0, voice 3/0, light on (0x00 inverted), reverb off gear 0.
const HEART_WIRE_EQ3: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x54, 0x00, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at eqLevel 2 (export frame 29; frame 33 identical):
/// eqLevel byte moved 3 → 2 and the XOR checksum byte 3C → 3D, everything
/// else untouched.
const HEART_WIRE_EQ2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x54, 0x00, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x3D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_EQ ack (export frames 25/37): 1-byte status 00 + session-residue
/// byte 0x61 — the run's ack shape, identical to the E2/E4
/// `… 13 00 01 00 75` family modulo cmd/values.
const SET_EQ_ACK_WIRE: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x07, 0x00, 0x01, 0x00, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Full real-session replay (route (b) closure): the export's control frames
/// replayed through the op end-to-end. TX side asserts byte-exact equality
/// with the captured SET frames; the lineage pin asserts the doc-recomputed
/// vector EQUALS the observed wire frame; the restore leg targets the value
/// set_eq itself returned from its fresh GET.
#[test]
fn set_eq_roundtrip_replays_the_real_session() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_WIRE_EQ3); // op's inner GET (= frames 17/21)
    mock.push_reply(SET_EQ_ACK_WIRE); // ack of frame 22's SET [02]
    mock.push_reply(HEART_WIRE_EQ2); // test's settle GET (frames 29/33)
    mock.push_reply(HEART_WIRE_EQ2); // op's inner GET of the restore
    mock.push_reply(SET_EQ_ACK_WIRE); // ack of frame 34's SET [03]
    mock.push_reply(HEART_WIRE_EQ3); // test's post-restore GET (frames 41/45)

    let mut c = control(mock);
    let previous = c
        .set_eq(2)
        .expect("real ack [00]+61 must be accepted (residue ignored)");
    assert_eq!(previous, 3, "frames 17/21 rest at pad 3");
    let after = c.get_heart_info().expect("real heart (residue 3d) parses");
    assert_eq!(after.eq_level, 2, "heart delta = written pad");
    assert_eq!(after.tx1_battery, 0x54, "run's battery byte");
    assert!(!after.tx2_status, "TX2 disconnected in the run");
    let restore_previous = c.set_eq(previous).expect("real restore must be accepted");
    assert_eq!(restore_previous, 2, "restore's fresh GET saw pad 2");
    let restored = c.get_heart_info().expect("heart back at pad 3");
    assert_eq!(restored.eq_level, 3, "restore delta verified");

    let written = c.transport().written();
    assert_eq!(&written[1], &SET_EQ_2_WIRE, "SET TX byte-exact frame 22");
    assert_eq!(
        &written[4], &SET_EQ_3_WIRE,
        "restore TX byte-exact frame 34"
    );
    // Lineage pin (roadmap §1(b)): the doc-recomputed vectors and the
    // hardware-observed frames are the SAME bytes.
    assert_eq!(SET_EQ_2_TX, SET_EQ_2_WIRE, "doc vector == observed wire");
    assert_eq!(SET_EQ_ACK, SET_EQ_ACK_WIRE, "ack lineage pin");
}

// ── Gain (cmd 3, SET_VOICE_LEVEL) — Phase 1B route (b) ──────────────────────
// Owner rule (2026-11-10): Mock vectors are RECOMPUTED FROM THE DOCS until
// the hardware run; real usbmon frames join as replay vectors after it —
// the same flow as EQ (roadmap §1 route (b)). Every frame below is computed
// from docs only:
//   frame template  docs/protocol.md §2 (ProtocolV2.getData [code])
//   cmd 3, payload  docs/set-gate-checklist.md §3 Gain row ([{value}]:
//                   identity — the seekbar's select index, byte N = display
//                   stop N+1; HLDevice6302Activity.java:840-846 [code])
//   value space     0..=5 = the READ space (§5.2 Volume row + §5.1: the
//                   bar works in a 0-based index space, touch coerceIn
//                   (0..5), draws "1".."6" — write space == read space, no
//                   inversion; session 3b read voiceLevel 03 ↔ phone stop
//                   "4" [wire captures/2026-11-05-noise-level-medium.txt])
// The heart reply is the wire-certified E1 heart (captures/
// 2026-11-05-hearinfo-first-wire.txt) with only the value byte moved: the
// 17-byte heart layout is [wire] and E1 already rests at the session-3b
// voice level (payload[8] = 03 → frame byte 15).

/// SET_GAIN of stop 2 (display stop "3" — one below the certified rest
/// state 3/"4") recomputed from the docs:
/// `05 03 AA DD 03 00 01 02 EF` + zero padding to 64 (big-endian len 00 01).
const SET_GAIN_2_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x03, 0x00, 0x01, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_GAIN ack recomputed from the docs: reply header `05 03 BB DD` + cmd
/// echo 03 + len 00 01 + status `00` (checklist §3 ACK rule: one status
/// byte, `data[0] == 0` = success — uniform across `ProSet*` incl.
/// `ProSetVoiceLevel.receiveData` [code]). The byte behind the payload
/// follows the reply-checksum rule the EQ session certified on the wire:
/// XOR(frame[0..7+len)) ⊕ 0x07 — for cmd 03 the prefix `05 03 BB DD 03 00
/// 01 00` folds to 0x62, so the vector carries `0x65` (EQ: cmd 07 → 0x61;
/// NC: cmd 13 → 0x75; both wire-matched). docs/protocol.md open question 2
/// still calls the ACK tail a per-session constant with the classification
/// open — under THAT reading this byte is unknowable pre-run. Either way
/// the crate stays byte-agnostic: everything past `7+len` is discarded, so
/// the vector doubles as an ignore-the-residue regression and the byte is
/// NOT part of the doc-recomputed contract.
const SET_GAIN_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x03, 0x00, 0x01, 0x00, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with voiceLevel (payload offset 8 = frame byte
/// 15) moved 3 → 2: layout stays [wire] (E1); the flipped value byte is the
/// doc-recomputed delta side of route (b), and the residue drops to 0x36
/// per the heart reply checksum rule (XOR(payload[0..16)) ⊕ 0x68 — the
/// LAST payload byte, signalMode (§5 offset 16), is NOT in the fold;
/// signalMode was 0 in E1 so both readings give the same byte — the
/// 03→02 flip lands the fold 0x5F → 0x5E, 0x5E⊕0x68 = 0x36; the identical
/// arithmetic the EQ-2 heart vector verified, since EQ2 flipped 03→02 too).
/// voiceMode (payload[9], §5 offset 9) is a SEPARATE field and stays 0.
const HEART_RX_GAIN2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_gain(2) (display stop "3" — the neighbouring stop BELOW the
/// certified rest state 3/"4", quieter): the op must issue its own fresh
/// GET first (uniform SET discipline), then the byte-exact doc-recomputed
/// SET frame, and hand back the fresh GET's stop as the pre-write state.
#[test]
fn set_gain_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (device rests at voice 3)
    mock.push_reply(SET_GAIN_ACK);
    let mut c = control(mock);

    let previous = c.set_gain(2).expect("ack 00 must be accepted");
    assert_eq!(
        previous, 3,
        "fresh GET inside set_gain = the stop before write"
    );

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(
        &written[1], &SET_GAIN_2_TX,
        "SET gain must be the doc-recomputed stop-2 frame"
    );
}

/// The identity value space (stops 0..=5 = display "1".."6" — §3 Gain row
/// write space == §5.2 read space): every SET differs from the stop-2
/// frame in the value byte only (cmd stays 03; no second payload byte —
/// voiceMode is a separate heart field and has no payload slot).
#[test]
fn set_gain_value_space_changes_only_the_value_byte() {
    for value in [0u8, 1, 3, 4, 5] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        mock.push_reply(SET_GAIN_ACK);
        let mut c = control(mock);

        let previous = c.set_gain(value).expect("ack 00 must be accepted");
        assert_eq!(previous, 3, "fresh GET reported stop {value} INCOMING");
        let mut want = SET_GAIN_2_TX;
        want[7] = value; // value byte sits at frame offset 7 (tail EF at 8)
        assert_eq!(&c.transport().written()[1], &want, "stop {value}");
    }
}

/// Full doc-vector round trip (route (b), Mock half): device rests at
/// voiceLevel 3 (display "4" — E1 IS that state [wire session 3b]), write
/// the neighbouring stop 2 below it ("3", quieter), ack `00`, heart
/// settles voiceLevel → 2 (§5 offset 8) with voiceMode untouched; restore
/// stop 3, heart settles back. Frames asserted both ways round trip; the
/// restore TARGET comes from set_gain's own return (the fresh GET inside
/// the write), not a caller-side poll.
#[test]
fn set_gain_roundtrip_doc_vectors() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner GET of set_gain(2): device at 3
    mock.push_reply(SET_GAIN_ACK); // ack for SET_GAIN [02]
    mock.push_reply(HEART_RX_GAIN2); // post-SET fresh GET: delta = 2
    mock.push_reply(HEART_RX_GAIN2); // inner GET of set_gain(prev): device at 2
    mock.push_reply(SET_GAIN_ACK); // ack for SET_GAIN [03]
    mock.push_reply(HEART_RX_E1); // post-restore fresh GET: delta = 3

    let mut c = control(mock);
    let previous = c.set_gain(2).expect("gain write must be accepted");
    assert_eq!(previous, 3, "pre-write stop from the op's own fresh GET");
    let after = c.get_heart_info().expect("post-SET GET");
    assert_eq!(
        after.voice_level, 2,
        "heart delta must equal the written stop"
    );
    assert_eq!(
        after.voice_mode, 0,
        "voiceMode (§5 offset 9) must never move"
    );
    let restore_previous = c.set_gain(previous).expect("restore must be accepted");
    assert_eq!(restore_previous, 2, "fresh GET before restore saw stop 2");
    let restored = c.get_heart_info().expect("post-restore GET");
    assert_eq!(
        restored.voice_level, 3,
        "heart must be back at the start stop"
    );
    assert_eq!(restored.voice_mode, 0, "voiceMode still untouched");

    let written = c.transport().written();
    let mut set3 = SET_GAIN_2_TX;
    set3[7] = previous; // value byte at frame offset 7 (tail EF at 8)
    assert_eq!(&written[1], &SET_GAIN_2_TX, "write leg is stop 2");
    assert_eq!(&written[4], &set3, "restore leg is set_gain's OWN return");
}

/// Ack gates shared with the SET_NOISE/SET_EQ families (checklist §3:
/// uniform `data[0] == 0` success across all `ProSet*` classes —
/// `ProSetVoiceLevel.receiveData` [code]): nonzero status =
/// [`Error::Rejected`], wrong ack length = [`Error::BadAck`]. Reason same
/// as the EQ tests: the ack shape has only ever been seen as `[00]`
/// [wire E2/E4].
#[test]
fn set_gain_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_GAIN_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_gain(2).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_GAIN_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_gain(2).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── Gain route (b): REAL wire frames join as replay vectors ─────────────────
// Full 64-byte buffers from the owner-executed hardware run of
// set_gain_roundtrip_guarded, captured 2026-11-10 (captures/raw/
// set_gain_roundtrip.pcap → captures/set_gain_roundtrip.jsonl, bus 6 dev 5).
// Frame numbers from the export:
//   #17/#21/#41/#45  heart, voice 2, battery 0x2B, tail 42 (#41 = the
//                    settle heart, #45 = the guard's exit poll; #54/#57 is
//                    the owner's post-run `holly status` 13.5 s later —
//                    voice 2 again, state restored)
//   #22              SET_GAIN `05 03 AA DD 03 00 01 01 EF`
//   #25/#37          ack `05 03 BB DD 03 00 01 00 65` (status 00)
//   #29/#33          heart, voice 1, tail 41
//   #34              SET_GAIN restore `05 03 AA DD 03 00 01 02 EF`
// The run's live start state was wire stop 2 (NOT the certified rest
// 3/"4" — the phone had drifted): the guard computed the neighbour below
// from the fresh GET, so the write leg landed on value 01 and the restore
// on 02. Heart tail bytes follow XOR(payload[0..len-1)) ⊕ 0x68 — the LAST
// payload byte (signalMode) excluded; the rule read as XOR(payload[0..17])
// ⊕ 0x68 was the same byte while signalMode stayed 0 (0x42 at voice
// 2, 0x41 at voice 1, both verified); the ack tail 0x65 is the 4th command
// shape matching XOR(whole 7+len prefix) ⊕ 0x07 — it was doc-recomputed
// pre-run and byte-matched on the wire (SET_GAIN_ACK == SET_GAIN_ACK_WIRE),
// refuting the "per-session constant" reading of protocol.md open
// question 2 for this shape.

/// Real SET_GAIN stop-1 frame of the run (export frame 22) — the
/// neighbouring stop below the live start 2. Same layout as
/// [`SET_GAIN_2_TX`] with the value byte moved.
const SET_GAIN_1_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x03, 0x00, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_GAIN stop-2 RESTORE frame (export frame 34) — byte-identical to
/// the doc-recomputed [`SET_GAIN_2_TX`] (cross-checked in the replay test).
const SET_GAIN_2_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x03, 0x00, 0x01, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at voice 2 (export frames 17/21/41/45): TX1
/// connected battery 0x2B, NC on level 2, voice 2/0, EQ pad 1, light on
/// (0x00 inverted), reverb off, tail 0x42.
const HEART_WIRE_VOICE2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x2B, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at voice 1 (export frames 29/33): the voice byte
/// moved 2 → 1 and the tail byte 42 → 41, everything else untouched.
const HEART_WIRE_VOICE1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x2B, 0x00, 0x00, 0x00, 0x01, 0x02, 0x01,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_GAIN ack (export frames 25/37): 1-byte status 00 + tail 0x65 —
/// equal to the doc-recomputed [`SET_GAIN_ACK`], which closes the residue
/// question for this command shape (see the section comment).
const SET_GAIN_ACK_WIRE: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x03, 0x00, 0x01, 0x00, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Full real-session replay (gain's route (b) closure): the export's control
/// frames replayed through the op end-to-end. TX side asserts byte-exact
/// equality with the captured SET frames; the lineage pin asserts the
/// doc-recomputed vector EQUALS the observed wire frame; the restore leg
/// targets the value set_gain itself returned from its fresh GET.
#[test]
fn set_gain_roundtrip_replays_the_real_session() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_WIRE_VOICE2); // op's inner GET (= frames 18/21)
    mock.push_reply(SET_GAIN_ACK_WIRE); // ack of frame 22's SET [01]
    mock.push_reply(HEART_WIRE_VOICE1); // test's settle GET (frames 26/29)
    mock.push_reply(HEART_WIRE_VOICE1); // op's inner GET of the restore
    mock.push_reply(SET_GAIN_ACK_WIRE); // ack of frame 34's SET [02]
    mock.push_reply(HEART_WIRE_VOICE2); // test's post-restore GET (frames 38/41)

    let mut c = control(mock);
    let previous = c
        .set_gain(1)
        .expect("real ack [00]+65 must be accepted (residue ignored)");
    assert_eq!(previous, 2, "frames 18/21 rest at voice 2");
    let after = c.get_heart_info().expect("real heart (residue 41) parses");
    assert_eq!(after.voice_level, 1, "heart delta = written stop");
    assert_eq!(
        after.voice_mode, 0,
        "voiceMode (§5 offset 9) must never move"
    );
    assert_eq!(after.tx1_battery, 0x2B, "run's battery byte");
    assert!(!after.tx2_status, "TX2 disconnected in the run");
    let restore_previous = c.set_gain(previous).expect("real restore must be accepted");
    assert_eq!(restore_previous, 1, "restore's fresh GET saw voice 1");
    let restored = c.get_heart_info().expect("heart back at voice 2");
    assert_eq!(restored.voice_level, 2, "restore delta verified");

    let written = c.transport().written();
    assert_eq!(&written[1], &SET_GAIN_1_WIRE, "SET TX byte-exact frame 22");
    assert_eq!(
        &written[4], &SET_GAIN_2_WIRE,
        "restore TX byte-exact frame 34"
    );
    // Lineage pin (roadmap §1(b)): the doc-recomputed vectors and the
    // hardware-observed frames are the SAME bytes — including the ack tail
    // 0x65 the checksum rule predicted before the run.
    assert_eq!(
        SET_GAIN_2_TX, SET_GAIN_2_WIRE,
        "doc vector == observed wire"
    );
    assert_eq!(SET_GAIN_ACK, SET_GAIN_ACK_WIRE, "ack lineage pin");
}

// ── Reverb (cmd 5, SET_REVERB) — Phase 1B route (b) ─────────────────────────
// Owner rule (2026-11-11): Mock vectors are RECOMPUTED FROM THE DOCS until
// the hardware run; real usbmon frames join as replay vectors after it —
// the same flow as EQ/gain (roadmap §1 route (b)). Every frame below is
// computed from docs only:
//   frame template  docs/protocol.md §2 (ProtocolV2.getData [code]); cmd 5 =
//                   ProSetReverb [code ProtocolFactoryV2 dispatch]
//   payload         [isOpen, gear] — switch FIRST [code
//                   UsbMessengerV2.setReverb(int value, boolean isOpen)
//                   builds {isOpen?1:0, value}, UsbMessengerV2.java:192-195]
//   gear space      0/1/2 = GEAR_ONE/GEAR_TWO/GEAR_THREE [code
//                   HLDevice6302Activity.java:629-632]; identity read side
//                   = heart payload[14] (§5 offset 14, §5.2)
// The heart reply is the wire-certified E1 heart (captures/
// 2026-11-05-hearinfo-first-wire.txt, line 19: payload[13] = 00 (reverb
// off), payload[14] = 02 — hearts keep reporting the gear while the switch
// is off, §5.2 sessions 1-4 [wire]) with only the reverb bytes moved:
// heart payload offset k = frame byte 7+k, so switch = frame byte 20 and
// gear = frame byte 21. The toggles pass the STARTING gear (E1's 02)
// through exactly like the APK does — gear buttons send isOpen=true with
// their gear (HLDevice6302Activity.java:759/:769/:779) and the toggle-off
// path sends its remembered current gear
// (setReverb(this.reverbType, false), :792-793) [code].

/// SET_REVERB toggle-ON with E1's resting gear 2 recomputed from the docs:
/// `05 03 AA DD 05 00 02 01 02 EF` + zero padding to 64 (big-endian len
/// 00 02; switch byte at 7, gear byte at 8, tail EF at 9).
const SET_REVERB_ON_2_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x05, 0x00, 0x02, 0x01, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_REVERB toggle-OFF with the same gear — the APK toggle-off shape
/// [`SET_REVERB_ON_2_TX`] with byte 7 (isOpen) 0x01 → 0x00.
const SET_REVERB_OFF_2_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x05, 0x00, 0x02, 0x00, 0x02, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_REVERB ack recomputed from the docs: reply header `05 03 BB DD` +
/// cmd echo 05 + len 00 01 + status `00` (checklist §3 ACK rule: one
/// status byte, `data[0] == 0` = success — uniform across `ProSet*` [code]).
/// The byte behind the payload follows the reply-checksum rule EQ/gain
/// certified on the wire: XOR(frame[0..7+len)) ⊕ 0x07 — the cmd-05 prefix
/// `05 03 BB DD 05 00 01 00` folds to 0x64, so the vector carries
/// `0x63` (EQ: cmd 07 → 0x61; gain: cmd 03 → 0x65; NC: cmd 13 → 0x75; all
/// wire-matched). protocol.md open question 2 stays the doc line until
/// the run; either way the crate discards everything past `7+len`, so the
/// vector doubles as an ignore-the-residue regression.
const SET_REVERB_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x05, 0x00, 0x01, 0x00, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with ONLY the reverb switch byte (payload[13] =
/// frame byte 20) moved 00 → 01: gear stays 02 — the docs' certified
/// gear-keeps-value-through-a-toggle [wire §5.2]. Layout stays [wire]
/// (E1); the residue drops 0x37 → 0x36 per the heart reply checksum rule
/// (XOR(payload[0..16)) ⊕ 0x68 — the LAST payload byte, signalMode, is
/// NOT in the fold: 2026-11-11 correction, signalMode was 0 in E1 so both
/// readings give the same byte — the 00→01 flip moves fold 0x5F → 0x5E,
/// 0x5E⊕0x68 = 0x36).
const HEART_RX_REVERB_ON: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x01, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with reverb ON gear 1: frame byte 20 = 01 (switch)
/// AND byte 21 = payload[14] moved 02 → 01 (a real gear delta — the
/// toggle-off-style no-move already covered by [`HEART_RX_REVERB_ON`]).
/// Fold: 0x5F ⊕ 0x01 ⊕ 0x03 = 0x5D → tail 0x5D⊕0x68 = 0x35.
const HEART_RX_REVERB_GEAR1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x00, 0x35, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_reverb(ON, gear 2): the op must issue its own fresh GET first
/// (uniform SET discipline), then the byte-exact doc-recomputed SET frame,
/// and hand back the fresh GET's PAIR as the pre-write state. E1 IS a
/// "(false, gear 2)" heart [wire], so the return doubles as the
/// toggle-safe restore datum.
#[test]
fn set_reverb_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (device rests off, gear 2)
    mock.push_reply(SET_REVERB_ACK);
    let mut c = control(mock);

    let previous = c.set_reverb(true, 2).expect("ack 00 must be accepted");
    assert_eq!(
        previous,
        (false, 2),
        "fresh GET inside set_reverb = the pair before write"
    );

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(
        &written[1], &SET_REVERB_ON_2_TX,
        "SET reverb must be the doc-recomputed on+gear-2 frame"
    );
    assert_eq!(written[1][4], cmd::SET_REVERB, "cmd echo = 5 [code]");
}

/// Both bytes are caller-supplied independently (no preserve byte in this
/// payload — unlike the SET_NOISE family): the 6 (switch, gear) shapes of
/// the certified space all differ from the template ONLY in bytes 7
/// (isOpen, switch FIRST) and 8 (gear), cmd/tail unchanged.
#[test]
fn set_reverb_switch_and_gear_change_independently() {
    for (switch, gear) in [
        (false, 0u8),
        (false, 1),
        (false, 2),
        (true, 0),
        (true, 1),
        (true, 2),
    ] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        mock.push_reply(SET_REVERB_ACK);
        let mut c = control(mock);

        let previous = c.set_reverb(switch, gear).expect("ack 00 must be accepted");
        assert_eq!(
            previous,
            (false, 2),
            "fresh GET reported ({}, gear {}) INCOMING",
            switch,
            gear
        );
        let mut want = SET_REVERB_ON_2_TX;
        want[7] = u8::from(switch); // isOpen at frame offset 7 (switch FIRST)
        want[8] = gear; // gear at frame offset 8 (tail EF at 9)
        assert_eq!(
            &c.transport().written()[1],
            &want,
            "(switch {}, gear {})",
            switch,
            gear
        );
    }
}

/// Full doc-vector round trip (route (b), Mock half) in the shape the
/// guarded hardware test rehearses on an OFF device: toggle ON with the
/// starting gear (E1 rests off gear 2 [wire]), heart settles ON with that
/// gear untouched; neighbour gear (2→1) written while ON; heart settles on
/// gear 1; restore = the STARTING pair (off, gear 2) — the APK toggle-off
/// shape; heart settles back. Frames asserted both ways round trip; every
/// restore TARGET comes from the ops' own fresh-GET returns, not a
/// caller-side poll.
#[test]
fn set_reverb_roundtrip_doc_vectors() {
    let mut set_on_gear1 = SET_REVERB_ON_2_TX;
    set_on_gear1[8] = 0x01; // neighbour gear while ON (gear byte only)

    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner GET of the toggle: (off, gear 2)
    mock.push_reply(SET_REVERB_ACK); // ack for SET_REVERB [01, 02]
    mock.push_reply(HEART_RX_REVERB_ON); // settle GET: gear kept its value
    mock.push_reply(HEART_RX_REVERB_ON); // inner GET of the gear write
    mock.push_reply(SET_REVERB_ACK); // ack for SET_REVERB [01, 01]
    mock.push_reply(HEART_RX_REVERB_GEAR1); // settle GET: gear 1
    mock.push_reply(HEART_RX_REVERB_GEAR1); // inner GET of the restore
    mock.push_reply(SET_REVERB_ACK); // ack for SET_REVERB [00, 02]
    mock.push_reply(HEART_RX_E1); // final GET: back to (off, gear 2)

    let mut c = control(mock);
    let previous = c
        .set_reverb(true, 2)
        .expect("toggle write must be accepted");
    assert_eq!(previous, (false, 2), "pre-write pair from the op's own GET");
    let after_toggle = c.get_heart_info().expect("post-toggle GET");
    assert!(
        after_toggle.reverb_switch && after_toggle.reverb_level == 2,
        "heart delta = switch on, GEAR UNTOUCHED (§5.2 [wire])"
    );
    let gear_previous = c
        .set_reverb(true, 1)
        .expect("neighbour-gear write must be accepted");
    assert_eq!(
        gear_previous,
        (true, 2),
        "the gear write's fresh GET saw the toggle's effect"
    );
    let after_gear = c.get_heart_info().expect("post-gear GET");
    assert!(
        after_gear.reverb_switch && after_gear.reverb_level == 1,
        "heart delta = gear moved on the on state"
    );
    let restore_previous = c.set_reverb(false, 2).expect("restore must be accepted");
    assert_eq!(
        restore_previous,
        (true, 1),
        "the restore's fresh GET saw the gear write's effect"
    );
    let restored = c.get_heart_info().expect("post-restore GET");
    assert!(
        !restored.reverb_switch && restored.reverb_level == 2,
        "heart must be back at the STARTING pair (off, gear 2)"
    );

    let written = c.transport().written();
    assert_eq!(&written[1], &SET_REVERB_ON_2_TX, "toggle leg = [01, 02]");
    assert_eq!(&written[4], &set_on_gear1, "gear leg = [01, 01]");
    assert_eq!(
        &written[7], &SET_REVERB_OFF_2_TX,
        "restore leg = the starting pair [00, 02]"
    );
}

/// Ack gates shared with the SET_NOISE/SET_EQ/SET_GAIN families (checklist
/// §3: uniform `data[0] == 0` success across all `ProSet*` classes [code]):
/// nonzero status = [`Error::Rejected`], wrong ack length =
/// [`Error::BadAck`]. Reason same as the EQ/gain tests: the ack shape has
/// only ever been seen as `[00]` [wire E2/E4].
#[test]
fn set_reverb_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_REVERB_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_reverb(true, 2).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_REVERB_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_reverb(true, 2).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── Reverb route (b): REAL wire frames join as replay vectors ───────────────
// Full 64-byte buffers from the owner-executed hardware run of
// set_reverb_roundtrip_guarded, captured 2026-11-11 (captures/raw/
// set_reverb_roundtrip.pcap → captures/set_reverb_roundtrip.jsonl,
// bus 6 dev 3). Frame numbers from the export:
//   #17/#21          heart, reverb off gear 0, battery 0x3B, tail 51
//                    (rA; #53/#57 = the restore-state hearts, byte-identical)
//   #22              SET_REVERB `05 03 AA DD 05 00 02 01 00 EF` — toggle ON
//                    with the STARTING gear 0 ([isOpen, gear])
//   #25/#37/#49      ack `05 03 BB DD 05 00 01 00 63` (status 00) — the
//                    doc-predicted tail 0x63 byte-matched pre-run, closing
//                    the cmd⊕0x66 rule over 4 command shapes
//   #29/#33          heart, reverb on gear 0 (switch flipped, GEAR UNTOUCHED),
//                    tail 50
//   #34              SET_REVERB `05 03 AA DD 05 00 02 01 01 EF` — the
//                    neighbouring gear (0→1) while on
//   #41/#45          heart, reverb on gear 1, tail 51
//   #46              SET_REVERB `05 03 AA DD 05 00 02 00 00 EF` — restore =
//                    the starting pair (off, gear 0), the APK toggle-off
//                    shape with the remembered gear
//   #53/#57          heart, reverb off gear 0 — full starting pair restored
// The run's live start state was reverb OFF gear 0 (NOT the E1 mock base's
// gear 2): the guard computed both legs from the fresh GETs, so the write
// legs landed on gear 00 → 01 and the restore on [00, 00] — the doc layout
// byte-matches in every leg. NEW: these are the first hearts on record
// with signalMode (payload[16]) = 1 — their tails (51/50/51) REFUTE the
// old XOR(payload[0..17]) ⊕ 0x68 wording and fit XOR(payload[0..16)) ⊕
// 0x68, the LAST payload byte excluded (tools/residue_check.py 14/14
// hearts; protocol.md §2 wording corrected).

/// Real SET_REVERB toggle-ON frame of the run (export frame 22) —
/// [isOpen=1, gear=0]: the starting gear passes through the toggle.
const SET_REVERB_ON_0_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x05, 0x00, 0x02, 0x01, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_REVERB neighbour-gear frame (export frame 34) — [01, 01]:
/// gear moved 0 → 1 while the switch stayed on.
const SET_REVERB_ON_1_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x05, 0x00, 0x02, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_REVERB RESTORE frame (export frame 46) — [00, 00]: the APK
/// toggle-off shape with the starting gear as the value byte.
const SET_REVERB_OFF_0_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x05, 0x00, 0x02, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at reverb off gear 0 (export frames 17/21/53/57):
/// TX1 connected battery 0x3B, TX2 disconnected, NC on level 2, voice 2/0,
/// light on (0x00 inverted), EQ pad 2, signalMode 1, tail 0x51.
const HEART_WIRE_REVERB_OFF0: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x3B, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x51, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at reverb on gear 0 (export frames 29/33): the
/// switch byte moved 00 → 01 (§5 offset 13), GEAR UNTOUCHED (offset 14
/// stays 00 — gear-keeps-value through a toggle, [wire] now on the write
/// path too), tail 0x50.
const HEART_WIRE_REVERB_ON0: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x3B, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
    0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00, 0x01, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at reverb on gear 1 (export frames 41/45): the
/// gear byte moved 00 → 01 on the on state, tail 0x51.
const HEART_WIRE_REVERB_ON1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x3B, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
    0x00, 0x00, 0x00, 0x02, 0x01, 0x01, 0x00, 0x01, 0x51, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_REVERB ack (export frames 25/37/49): 1-byte status 00 + tail
/// 0x63 — equal to the doc-recomputed [`SET_REVERB_ACK`], which closes the
/// residue question for this command shape (see the section comment).
const SET_REVERB_ACK_WIRE: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x05, 0x00, 0x01, 0x00, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Full real-session replay (reverb's route (b) closure): the export's
/// control frames replayed through the op end-to-end in the guarded test's
/// three-leg shape. TX side asserts byte-exact equality with the captured
/// SET frames; the lineage pins assert the doc-recomputed layout EQUALS
/// the observed wire frames (gear byte only); the restore leg targets the
/// pair set_reverb itself returned from its fresh GET.
#[test]
fn set_reverb_roundtrip_replays_the_real_session() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_WIRE_REVERB_OFF0); // op's inner GET (= frames 17/21)
    mock.push_reply(SET_REVERB_ACK_WIRE); // ack of frame 22's SET [01,00]
    mock.push_reply(HEART_WIRE_REVERB_ON0); // test's settle GET (frames 29/33)
    mock.push_reply(HEART_WIRE_REVERB_ON0); // op's inner GET of the gear leg
    mock.push_reply(SET_REVERB_ACK_WIRE); // ack of frame 34's SET [01,01]
    mock.push_reply(HEART_WIRE_REVERB_ON1); // test's settle GET (frames 41/45)
    mock.push_reply(HEART_WIRE_REVERB_ON1); // op's inner GET of the restore
    mock.push_reply(SET_REVERB_ACK_WIRE); // ack of frame 46's SET [00,00]
    mock.push_reply(HEART_WIRE_REVERB_OFF0); // test's final GET (frames 53/57)

    let mut c = control(mock);
    let previous = c
        .set_reverb(true, 0)
        .expect("real ack [00]+63 must be accepted (residue ignored)");
    assert_eq!(previous, (false, 0), "frames 17/21 rest at (off, gear 0)");
    let after_toggle = c.get_heart_info().expect("real heart (residue 50) parses");
    assert!(
        after_toggle.reverb_switch && after_toggle.reverb_level == 0,
        "heart delta = switch on, GEAR UNTOUCHED (frames 29/33)"
    );
    assert_eq!(after_toggle.tx1_battery, 0x3B, "run's battery byte");
    assert!(!after_toggle.tx2_status, "TX2 disconnected in the run");
    assert_eq!(
        after_toggle.signal_mode, 1,
        "the run's hearts carry signalMode=1 — the checksum split"
    );
    let gear_previous = c
        .set_reverb(true, 1)
        .expect("real neighbour-gear write must be accepted");
    assert_eq!(
        gear_previous,
        (true, 0),
        "the gear leg's fresh GET saw the toggle's effect"
    );
    let after_gear = c.get_heart_info().expect("real heart (residue 51) parses");
    assert!(
        after_gear.reverb_switch && after_gear.reverb_level == 1,
        "heart delta = gear moved on the on state"
    );
    let restore_previous = c
        .set_reverb(false, 0)
        .expect("real restore must be accepted");
    assert_eq!(
        restore_previous,
        (true, 1),
        "the restore's fresh GET saw the gear leg's effect"
    );
    let restored = c.get_heart_info().expect("heart back at the start pair");
    assert!(
        !restored.reverb_switch && restored.reverb_level == 0,
        "restore delta verified — full starting pair (off, gear 0)"
    );

    let written = c.transport().written();
    assert_eq!(
        &written[1], &SET_REVERB_ON_0_WIRE,
        "SET TX byte-exact frame 22"
    );
    assert_eq!(
        &written[4], &SET_REVERB_ON_1_WIRE,
        "gear-leg TX byte-exact frame 34"
    );
    assert_eq!(
        &written[7], &SET_REVERB_OFF_0_WIRE,
        "restore TX byte-exact frame 46"
    );
    // Lineage pins (roadmap §1(b)): the doc-recomputed layout and the
    // hardware-observed frames are the SAME bytes — the run's gear differs
    // from the E1 mock base (00 vs 02), so the pins recompute the doc
    // vectors' gear byte and assert frame equality; the ack tail 0x63 the
    // checksum rule predicted pre-run is byte-equal as-is.
    let mut doc_on0 = SET_REVERB_ON_2_TX;
    doc_on0[8] = 0x00;
    assert_eq!(
        SET_REVERB_ON_0_WIRE, doc_on0,
        "doc layout == observed wire (toggle leg)"
    );
    let mut doc_on1 = SET_REVERB_ON_2_TX;
    doc_on1[8] = 0x01;
    assert_eq!(
        SET_REVERB_ON_1_WIRE, doc_on1,
        "doc layout == observed wire (gear leg)"
    );
    let mut doc_off0 = SET_REVERB_OFF_2_TX;
    doc_off0[8] = 0x00;
    assert_eq!(
        SET_REVERB_OFF_0_WIRE, doc_off0,
        "doc layout == observed wire (restore leg)"
    );
    assert_eq!(SET_REVERB_ACK, SET_REVERB_ACK_WIRE, "ack lineage pin");
}

// ── Channel mode (cmd 1, SET_VOICE_MODE) — route (b), doc vectors ───────────
// Owner rule (roadmap §1(b), same as EQ/gain/reverb): Mock vectors are
// RECOMPUTED FROM THE DOCS until the hardware run; real usbmon frames join
// as replay vectors after it. Every frame below is computed from docs only:
//   frame template       docs/protocol.md §2 (ProtocolV2.getData [code])
//   cmd 1, payload       docs/set-gate-checklist.md §3 Channel-mode row
//                        ([{value}] — UsbMessengerV2.java:185-188)
//   value space 0/1      identity mono→0, stereo→1
//                        (HLDevice6302Activity.java:1651-1662); read side
//                        heart voiceMode §5 offset 9 (ProGetHearInfo.java:
//                        69-71), 0=Mono/1=Stereo (§5.2, :1803-1805)
// The heart reply is the wire-certified E1 heart (captures/
// 2026-11-05-hearinfo-first-wire.txt, which already rests at voiceMode 0)
// with only the value byte moved — the 17-byte heart layout is [wire].

/// SET_VOICE_MODE of Stereo (1) recomputed from the docs:
/// `05 03 AA DD 01 00 01 01 EF` + zero padding to 64 (big-endian len 00 01).
const SET_CHANNEL_MODE_1_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x01, 0x00, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_VOICE_MODE ack recomputed from the docs: reply header `05 03 BB DD`
/// with cmd echo 01, len 00 01, status `00` (checklist §3 ACK rule: one
/// status byte, `data[0] == 0` = success — `ProSetVoiceMode.receiveData`
/// reads `toInt(data[0]) == 0` [code :25-28]). The byte behind the payload
/// follows the reply checksum rule XOR(frame[0..7+len)) ⊕ 0x07 — for cmd 01
/// the prefix folds to 0x60, 0x60 ⊕ 0x07 = 0x67 (the same byte the 4-shape
/// cmd ⊕ 0x66 shortcut predicts: 0x01 ⊕ 0x66 = 0x67). The byte stays
/// UNINTERPRETED by the crate — it must ignore everything past `7+len`, so
/// the vector still doubles as an ignore-the-residue regression.
const SET_CHANNEL_MODE_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x01, 0x00, 0x01, 0x00, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with voiceMode (payload offset 9 = frame byte 16)
/// moved 0 → 1: layout stays [wire] (E1); the flipped value byte is the
/// doc-recomputed delta side of route (b), and the residue drops to 0x36
/// per the heart reply checksum rule (XOR(payload[0..16)) ⊕ 0x68 — the LAST
/// payload byte, signalMode, is NOT in the fold; signalMode was 0 in E1 so
/// both readings give the same byte; value 00^01 flips that fold,
/// 0x5F⊕0x68=0x37 → 0x5E⊕0x68=0x36). Every OTHER field stays put — gain
/// (voiceLevel, offset 8) stays 3 (the §3 Gain row deliberately left this
/// neighbouring field alone), EQ stays 3, reverb stays (off, gear 2).
const HEART_RX_MODE1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x01, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_channel_mode(1) (Stereo — the other documented value; the E1 heart
/// rests at Mono 0 [wire, session 1; §5.2 every heart so far read 0]): the
/// op must issue its own fresh GET first (uniform SET discipline), then the
/// byte-exact doc-recomputed SET frame, and hand back the fresh GET's mode
/// as the pre-write state.
#[test]
fn set_channel_mode_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (device rests at Mono 0)
    mock.push_reply(SET_CHANNEL_MODE_ACK);
    let mut c = control(mock);

    let previous = c.set_channel_mode(1).expect("ack 00 must be accepted");
    assert_eq!(
        previous, 0,
        "fresh GET inside set_channel_mode = the mode before write"
    );

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(
        &written[1], &SET_CHANNEL_MODE_1_TX,
        "SET channel mode must be the doc-recomputed stereo frame"
    );
    assert_eq!(
        written[1][4],
        cmd::SET_VOICE_MODE,
        "cmd echo = 1 [code ProSetVoiceMode.getCmd]"
    );
}

/// The documented value space is exactly two values (0=Mono, 1=Stereo —
/// identity on both the write caller `HLDevice6302Activity.java:1651-1662`
/// and the read renderer `:1803-1805`): each SET differs from the stereo
/// frame in the value byte only (cmd stays 01; no second payload byte).
#[test]
fn set_channel_mode_value_space_changes_only_the_value_byte() {
    for value in [0u8, 1] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        mock.push_reply(SET_CHANNEL_MODE_ACK);
        let mut c = control(mock);

        let previous = c.set_channel_mode(value).expect("ack 00 must be accepted");
        assert_eq!(previous, 0, "fresh GET reported mode {value} INCOMING");
        let mut want = SET_CHANNEL_MODE_1_TX;
        want[7] = value; // value byte sits at frame offset 7 (tail EF at 8)
        assert_eq!(&c.transport().written()[1], &want, "mode {value}");
    }
}

/// Full doc-vector round trip (route (b), Mock half) in the shape the
/// guarded hardware test rehearses: device rests at Mono 0 (E1 IS that
/// state [wire]), write the other documented value (Stereo 1), ack `00`,
/// heart settles voiceMode → 1 (§5 offset 9) with every neighbouring field
/// untouched (gain stays 3 — the mode byte is the ONLY delta); restore 0,
/// heart settles back. Frames asserted both ways round trip; the restore
/// TARGET comes from set_channel_mode's own return (the fresh GET inside
/// the write), not a caller-side poll.
#[test]
fn set_channel_mode_roundtrip_doc_vectors() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner GET of set_channel_mode(1): device at 0
    mock.push_reply(SET_CHANNEL_MODE_ACK); // ack for SET_VOICE_MODE [01]
    mock.push_reply(HEART_RX_MODE1); // post-SET fresh GET: delta = 1
    mock.push_reply(HEART_RX_MODE1); // inner GET of the restore: device at 1
    mock.push_reply(SET_CHANNEL_MODE_ACK); // ack for SET_VOICE_MODE [00]
    mock.push_reply(HEART_RX_E1); // post-restore fresh GET: delta = 0

    let mut c = control(mock);
    let previous = c
        .set_channel_mode(1)
        .expect("channel-mode write must be accepted");
    assert_eq!(previous, 0, "pre-write mode from the op's own fresh GET");
    let after = c.get_heart_info().expect("post-SET GET");
    assert_eq!(
        after.voice_mode, 1,
        "heart delta must equal the written mode"
    );
    assert_eq!(
        after.voice_level, 3,
        "voiceLevel (§5 offset 8, the neighbouring gain field) must never move"
    );
    assert_eq!(after.eq_level, 3, "eqLevel (§5 offset 12) must never move");
    assert_eq!(
        (after.reverb_switch, after.reverb_level),
        (false, 2),
        "reverb fields (§5 offsets 13/14) must never move"
    );
    let restore_previous = c
        .set_channel_mode(previous)
        .expect("restore must be accepted");
    assert_eq!(restore_previous, 1, "fresh GET before restore saw mode 1");
    let restored = c.get_heart_info().expect("post-restore GET");
    assert_eq!(
        restored.voice_mode, 0,
        "heart must be back at the start mode"
    );
    assert_eq!(restored.voice_level, 3, "voiceLevel still untouched");

    let written = c.transport().written();
    let mut set0 = SET_CHANNEL_MODE_1_TX;
    set0[7] = previous; // value byte at frame offset 7 (tail EF at 8)
    assert_eq!(&written[1], &SET_CHANNEL_MODE_1_TX, "write leg is stereo 1");
    assert_eq!(&written[4], &set0, "restore leg is the op's OWN return");
}

/// Ack gates shared with the SET_NOISE/SET_EQ/SET_GAIN/SET_REVERB families
/// (checklist §3: uniform `data[0] == 0` success across all `ProSet*`
/// classes — `ProSetVoiceMode.receiveData` [code :25-28]): nonzero status =
/// [`Error::Rejected`], wrong ack length = [`Error::BadAck`]. Reason same
/// as the other families: the ack shape has only ever been seen as `[00]`
/// [wire E2/E4].
#[test]
fn set_channel_mode_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_CHANNEL_MODE_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_channel_mode(1).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_CHANNEL_MODE_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_channel_mode(1).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── Channel mode route (b): REAL wire frames join as replay vectors ─────────
// Full 64-byte buffers from the owner-executed hardware run of
// set_channel_mode_roundtrip_guarded, captured 2026-11-11 (captures/raw/
// set_channel_mode_roundtrip.pcap → captures/set_channel_mode_roundtrip.jsonl,
// bus 6 dev 3; 16 data transfers, 65 ms). Row numbers from the export:
//   #2/#4            heart, voiceMode 0 (Mono), TX1 OFF / TX2 ON battery
//                    0x64, NC on level 2, voice 4, eq 3, reverb off gear 0,
//                    signalMode 0, tail 09 (#14/#16 = the restore-state
//                    hearts, byte-identical)
//   #5               SET_VOICE_MODE `05 03 AA DD 01 00 01 01 EF` — the OTHER
//                    documented value (Stereo 1); the run's live start was
//                    wire 0 (Mono, the last-known state)
//   #6/#12           ack `05 03 BB DD 01 00 01 00 67` (status 00) — the
//                    doc-predicted tail 0x67 byte-matched pre-run, closing
//                    the cmd⊕0x66 rule over 5 command shapes
//   #8/#10           heart, voiceMode 1 (Stereo) — the ONLY payload byte
//                    that moved across all six hearts (offset 9, §5), tail 08
//   #11              SET_VOICE_MODE `05 03 AA DD 01 00 01 00 EF` — restore =
//                    the starting Mono 0
//   #14/#16          heart, voiceMode 0 — start state restored (the second
//                    is the exit guard's own GET, already at original)
// No re-enumeration occurred: device address 3 throughout, 16/16 transfers
// succeeded, every settle poll matched on its first GET — the guarded test's
// reopen-by-VID:PID tolerance was NEVER exercised (it exists for the next
// run's risk, not this one's). Heart tails fit XOR(payload[0..16)) ⊕ 0x68
// (09/08) and the ack tail fits XOR(7+len prefix) ⊕ 0x07 — both rules'
// predictions byte-matched, protocol.md §2.

/// Real SET_VOICE_MODE write frame of the run (export row 5) — value 01
/// (Stereo): the other documented value, written from Mono 0.
const SET_CHANNEL_MODE_1_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x01, 0x00, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_VOICE_MODE RESTORE frame (export row 11) — value 00 (Mono): the
/// starting state written back exactly.
const SET_CHANNEL_MODE_0_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x01, 0x00, 0x01, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at voiceMode 0 (export rows 2/4/14/16): TX1 OFF,
/// TX2 ON battery 0x64, TX1 battery 0, both unmuted, NC on level 2, voice 4,
/// Mono 0, shutdown 0, light on (0x00 inverted), eq 3, reverb off gear 0,
/// txIdentify 0, signalMode 0, tail 0x09.
const HEART_WIRE_MODE0: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x64, 0x00, 0x00, 0x01, 0x02, 0x04,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real heart of the run at voiceMode 1 (export rows 8/10): ONLY the mode
/// byte moved 00 → 01 (§5 offset 9 = frame byte 16) across all six hearts —
/// state-change congruence on the write path; tail 0x08 (the mode flip
/// folds 0x61 → 0x60, ⊕0x68 → 0x08).
const HEART_WIRE_MODE1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x64, 0x00, 0x00, 0x01, 0x02, 0x04,
    0x01, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_VOICE_MODE ack (export rows 6/12): 1-byte status 00 + tail
/// 0x67 — equal to the doc-recomputed [`SET_CHANNEL_MODE_ACK`], which
/// closes the residue question for this command shape (see the section
/// comment).
const SET_CHANNEL_MODE_ACK_WIRE: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x01, 0x00, 0x01, 0x00, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Full real-session replay (channel mode's route (b) closure): the export's
/// control frames replayed through the op end-to-end in the guarded test's
/// leg shape (write 1, settle, restore 0, settle, exit guard). TX side
/// asserts byte-exact equality with the captured SET frames; the lineage
/// pins assert the doc-recomputed layout EQUALS the observed wire frames;
/// the restore leg targets the mode set_channel_mode itself returned from
/// its fresh GET.
#[test]
fn set_channel_mode_roundtrip_replays_the_real_session() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_WIRE_MODE0); // op's inner GET (= rows 2/4)
    mock.push_reply(SET_CHANNEL_MODE_ACK_WIRE); // ack of row 5's SET [01]
    mock.push_reply(HEART_WIRE_MODE1); // test's settle GET (rows 8/10)
    mock.push_reply(HEART_WIRE_MODE1); // op's inner GET of the restore (row 10)
    mock.push_reply(SET_CHANNEL_MODE_ACK_WIRE); // ack of row 11's SET [00]
    mock.push_reply(HEART_WIRE_MODE0); // test's final GET (rows 14/16)

    let mut c = control(mock);
    let previous = c
        .set_channel_mode(1)
        .expect("real ack [00]+67 must be accepted (residue ignored)");
    assert_eq!(
        previous, 0,
        "rows 2/4 rest at Mono 0 (the last-known state)"
    );
    let after = c.get_heart_info().expect("real heart (residue 08) parses");
    assert_eq!(after.voice_mode, 1, "heart delta = Stereo (rows 8/10)");
    assert!(
        !after.tx1_status,
        "TX1 disconnected in the run (owner note)"
    );
    assert!(after.tx2_status, "TX2 connected in the run (owner note)");
    assert_eq!(after.tx2_battery, 0x64, "run's TX2 battery byte");
    assert_eq!(after.voice_level, 4, "run's gain byte (phone drift from 3)");
    assert_eq!(
        after.signal_mode, 0,
        "signalMode 0 in this run — both tail readings coincide"
    );
    let restore_previous = c
        .set_channel_mode(0)
        .expect("real restore must be accepted");
    assert_eq!(
        restore_previous, 1,
        "the restore's fresh GET saw the write leg's effect (row 10)"
    );
    let restored = c.get_heart_info().expect("heart back at the start mode");
    assert_eq!(
        restored.voice_mode, 0,
        "restore delta verified — Mono restored (row 14)"
    );

    let written = c.transport().written();
    assert_eq!(
        &written[1], &SET_CHANNEL_MODE_1_WIRE,
        "SET TX byte-exact row 5"
    );
    assert_eq!(
        &written[4], &SET_CHANNEL_MODE_0_WIRE,
        "restore TX byte-exact row 11"
    );
    // Lineage pins (roadmap §1(b)): the doc-recomputed layout and the
    // hardware-observed frames are the SAME bytes — the run started at the
    // same mode the doc vectors assumed (0), so the write-leg pin is
    // byte-equal as-is and the restore pin recomputes only the value byte;
    // the ack tail 0x67 the checksum rule predicted pre-run is byte-equal
    // as-is (n=5 for cmd ⊕ 0x66).
    assert_eq!(
        SET_CHANNEL_MODE_1_WIRE, SET_CHANNEL_MODE_1_TX,
        "doc layout == observed wire (write leg)"
    );
    let mut doc_0 = SET_CHANNEL_MODE_1_TX;
    doc_0[7] = 0x00;
    assert_eq!(
        SET_CHANNEL_MODE_0_WIRE, doc_0,
        "doc layout == observed wire (restore leg)"
    );
    assert_eq!(
        SET_CHANNEL_MODE_ACK, SET_CHANNEL_MODE_ACK_WIRE,
        "ack lineage pin"
    );
}

// ── SET_TX_MUTE (cmd 22): the per-mic Mute buttons, [code]-certified frames ──
// Route (b) doc vectors — the mute SET path has never been on the wire (§3
// TX-mute row stays EXTRA-BLOCKED until the guarded round trip runs). Frame
// layout from the APK builder [code UsbMessengerV2.java:206-210]: prefix
// `05 03 AA DD` + cmd 16 (= 22) + big-endian len 00 02 + payload
// `[{txIndex}, {muted}]` (one TX per frame — each mute click names one
// ordinal, HLDevice6302Activity.java:677/:691/:705/:719) + tail EF (the
// doc-recomputed SET tail every SET_* family vector carries). ACK: reply
// header + cmd echo + len 00 01 + status 00 (ProSetTxMute.receiveData reads
// `toInt(data[0]) == 0` [code :26-30]); the byte behind the payload follows
// the reply-checksum rule XOR(frame[0..7+len)) ⊕ 0x07 = cmd ⊕ 0x66 —
// 0x16 ⊕ 0x66 = 0x70, the 6th command shape this rule predicts. UNENFORCED
// until the run: the crate discards everything past `7+len`, so the vector
// doubles as an ignore-the-residue regression either way.

/// SET_TX_MUTE of TX1 muted (txIndex 0, muted 01) recomputed from the docs:
/// `05 03 AA DD 16 00 02 00 01 EF` + zero padding to 64 (big-endian len
/// 00 02; slot byte at 7, mute byte at 8, tail EF at 9).
const SET_TX_MUTE_ON_TX1: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x00, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The restore leg — TX1 unmuted: [`SET_TX_MUTE_ON_TX1`] with the mute byte
/// (frame byte 8) 0x01 → 0x00. The slot byte stays 00.
const SET_TX_MUTE_OFF_TX1: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_TX_MUTE of TX2 muted (txIndex 1, muted 01): [`SET_TX_MUTE_ON_TX1`]
/// with the SLOT byte (frame byte 7) 0x00 → 0x01 — length and tail
/// unchanged, the two payload bytes are independent (one TX per frame).
const SET_TX_MUTE_ON_TX2: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_TX_MUTE ack recomputed from the docs: reply header `05 03 BB DD` +
/// cmd echo 16 + len 00 01 + status `00` (checklist §3 ACK rule: one status
/// byte, `data[0] == 0` = success — `ProSetTxMute.receiveData` reads
/// `toInt(data[0]) == 0` [code :26-30]). The byte behind the payload follows
/// the reply checksum rule XOR(frame[0..7+len)) ⊕ 0x07 — the cmd-16 prefix
/// `05 03 BB DD 16 00 01 00` folds to 0x77, 0x77 ⊕ 0x07 = 0x70 (the same
/// byte the cmd ⊕ 0x66 shortcut predicts: 0x16 ⊕ 0x66 = 0x70). The byte
/// stays UNENFORCED until the run — the crate must ignore everything past
/// `7+len`, so the vector still doubles as an ignore-the-residue regression.
const SET_TX_MUTE_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x16, 0x00, 0x01, 0x00, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Route (b) START heart recomputed from the certified 17-byte layout
/// ([wire] E1, captures/2026-11-05-hearinfo-first-wire.txt) with TX1/TX2
/// swapped: TX1 CONNECTED at battery 0x5D (payload[2]), TX2 disconnected
/// (payload[1] = 00), BOTH mute bytes 00 (§5 offsets 4/5), every other
/// field at E1's resting values. Residue 0x37: fold XOR(payload[0..16)) ⊕
/// 0x68, the LAST payload byte (signalMode 0) excluded.
const HEART_RX_TX1_ON: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The write-leg heart: [`HEART_RX_TX1_ON`] with ONLY tx1_mute (payload[4]
/// = frame byte 11) moved 00 → 01 — TX2's mute byte (payload[5]) and every
/// other field stay put. The flipped byte is the doc-recomputed delta side
/// of route (b), and the residue drops 0x37 → 0x36 per the heart reply
/// checksum rule (XOR(payload[0..16)) ⊕ 0x68 — LAST payload byte excluded;
/// signalMode was 0 in E1 so both readings give the same byte; 00^01 flips
/// that fold, 0x5F⊕0x68=0x37 → 0x5E⊕0x68=0x36).
const HEART_RX_TX1_MUTED: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x00, 0x5D, 0x00, 0x01, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_tx_mute(0, true) — the per-mic mute write (TX1 muted): the op must
/// issue its own fresh GET first (uniform SET discipline), then the
/// byte-exact doc-recomputed SET frame, and hand back the fresh GET's mute
/// word for the ADDRESSED slot as the pre-write state.
#[test]
fn set_tx_mute_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_TX1_ON); // inner fresh GET (mutes rest at 00)
    mock.push_reply(SET_TX_MUTE_ACK);
    let mut c = control(mock);

    let previous = c.set_tx_mute(0, true).expect("ack 00 must be accepted");
    assert_eq!(
        previous,
        Some(false),
        "the fresh GET's tx1_mute 00 is the returned pre-write state"
    );
    assert_eq!(
        &c.transport().written()[1],
        &SET_TX_MUTE_ON_TX1,
        "SET TX byte-exact doc layout [txIndex 0, muted 01]"
    );
}

/// The payload's two bytes are INDEPENDENT — one TX per frame, no
/// broadcast variant: addressing TX2 (txIndex 1) keeps frame length and
/// tail unchanged, moving only the slot byte [code
/// HLDevice6302Activity.java:677/:691/:705/:719 — each mute click names
/// one ordinal]. The heart is the certified E1 (TX2 connected) so the
/// fresh GET reports TX2's resting mute as the pre-write state.
#[test]
fn set_tx_mute_slot_byte_is_independent() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (TX2 connected, mutes 00)
    mock.push_reply(SET_TX_MUTE_ACK);
    let mut c = control(mock);

    let previous = c.set_tx_mute(1, true).expect("ack 00 must be accepted");
    assert_eq!(
        previous,
        Some(false),
        "the addressed slot's resting mute is the pre-write state"
    );
    assert_eq!(
        &c.transport().written()[1],
        &SET_TX_MUTE_ON_TX2,
        "TX2 write = the same frame with the slot byte at 01"
    );
}

/// Route (b) doc round trip: mute TX1 → heart delta → restore. The SET
/// legs carry the byte-exact doc frames, the settle/final GETs carry the
/// doc hearts (start pair ↔ TX1-muted pair), and the addressed slot's
/// pre-write word comes back from each leg's fresh GET. TX2's mute byte
/// stays 00 across ALL hearts — the per-slot independence the docs
/// certify (one TX per frame). No lineage pins yet: the wire frames land
/// with the guarded run and join as replay vectors then.
#[test]
fn set_tx_mute_roundtrip_doc_vectors() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_TX1_ON); // op's inner GET (start: mutes off)
    mock.push_reply(SET_TX_MUTE_ACK); // ack of the mute SET
    mock.push_reply(HEART_RX_TX1_MUTED); // test's settle GET (heart delta)
    mock.push_reply(HEART_RX_TX1_MUTED); // op's inner GET of the restore
    mock.push_reply(SET_TX_MUTE_ACK); // ack of the restore SET
    mock.push_reply(HEART_RX_TX1_ON); // test's final GET (restored)

    let mut c = control(mock);
    let previous = c
        .set_tx_mute(0, true)
        .expect("doc ack [00]+70 must be accepted (residue unenforced)");
    assert_eq!(
        previous,
        Some(false),
        "the op's fresh GET rests at tx1_mute 00 (the last-known state)"
    );
    let after = c.get_heart_info().expect("doc heart (muted pair) parses");
    assert!(after.tx1_mute, "heart delta = TX1 muted (payload[4] = 01)");
    assert!(
        !after.tx2_mute,
        "TX2's mute byte stayed 00 — one TX per frame, no broadcast"
    );
    assert_eq!(
        after.tx1_battery, 0x5D,
        "TX1's battery byte sits at payload[2] in the doc hearts"
    );
    assert!(
        !after.tx2_status,
        "TX2 stays disconnected in the doc hearts"
    );

    let restore_previous = c
        .set_tx_mute(0, false)
        .expect("doc restore must be accepted");
    assert_eq!(
        restore_previous,
        Some(true),
        "the restore's fresh GET saw the write leg's effect (muted heart)"
    );
    let restored = c.get_heart_info().expect("heart back at the start pair");
    assert!(!restored.tx1_mute, "restore delta verified — TX1 unmuted");

    let written = c.transport().written();
    assert_eq!(
        &written[1], &SET_TX_MUTE_ON_TX1,
        "SET TX byte-exact doc layout (write leg)"
    );
    assert_eq!(
        &written[4], &SET_TX_MUTE_OFF_TX1,
        "restore TX byte-exact doc layout [txIndex 0, muted 00]"
    );
}

/// Ack gates shared with the SET_NOISE/SET_EQ/SET_GAIN/SET_REVERB/
/// SET_VOICE_MODE families (checklist §3: uniform `data[0] == 0` success
/// across all `ProSet*` classes — `ProSetTxMute.receiveData` [code
/// :26-30]): nonzero status = [`Error::Rejected`], wrong ack length =
/// [`Error::BadAck`]. Reason same as the other families: the ack shape
/// has only ever been seen as `[00]` [wire E2/E4].
#[test]
fn set_tx_mute_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_TX1_ON);
    let mut ack = SET_TX_MUTE_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_tx_mute(0, true).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_TX1_ON);
    let mut ack = SET_TX_MUTE_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_tx_mute(0, true).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}

// ── TX mute route (b): REAL wire frames join as replay vectors ──────────────
// Full 64-byte buffers from the owner-executed hardware run of
// set_tx_mute_roundtrip_guarded, captured 2026-11-11 (captures/raw/
// set_tx_mute_roundtrip.pcap → captures/set_tx_mute_roundtrip.jsonl,
// bus 6 dev 2; 16 data transfers). Frame numbers from the export:
//   #14/#18          GET poll `05 03 AA DD 1F 00 00 EF` (byte-identical to
//                    the session-1 HEART_TX)
//   #17/#21          heart, mutes off — TX1 CONNECTED battery 0x4B, TX2
//                    CONNECTED battery 0x51 (TX2 was POWERED during the
//                    run — reported only; its slot byte was never written
//                    and its mute byte stayed 00), NC on level 0, voice 4,
//                    eq 3, reverb off gear 0, signalMode 0, tail 74
//                    (#41/#45 = the restored hearts, byte-identical)
//   #22              SET_TX_MUTE `05 03 AA DD 16 00 02 00 01 EF` — the TX1
//                    slot muted (txIndex 0, muted 01)
//   #25/#37          ack `05 03 BB DD 16 00 01 00 70` (status 00) — the
//                    doc-predicted tail 0x70 byte-matched pre-run, closing
//                    the cmd⊕0x66 rule over 6 command shapes
//   #29/#33          heart, tx1_mute 01 — the ONLY payload byte that moved
//                    across all six hearts (offset 4, §5), tail 75
//   #34              SET_TX_MUTE `05 03 AA DD 16 00 02 00 00 EF` — restore =
//                    the starting byte (muted 00)
//   #38/#41          heart, mutes off — restore delta verified (the first
//                    settle GET already matched)
//   #42/#45          heart, mutes off — the exit guard's own GET, already
//                    at original ("nothing to do")
// No rejections, no retries: every settle poll matched on its first GET
// (like the channel-mode run). Heart tails fit XOR(payload[0..16)) ⊕ 0x68
// (74/75) and the ack tail fits XOR(7+len prefix) ⊕ 0x07 — both rules'
// predictions byte-matched, protocol.md §2.

/// Real SET_TX_MUTE write frame of the run (export frame 22) —
/// [txIndex=0, muted=01]: the TX1 slot muted.
const SET_TX_MUTE_ON_TX1_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x00, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real SET_TX_MUTE restore frame of the run (export frame 34) — the
/// starting byte (txIndex 0, muted 00).
const SET_TX_MUTE_OFF_TX1_WIRE: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real ack of the run (export frames 25/37) — status 00, tail 0x70.
const SET_TX_MUTE_ACK_WIRE: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x16, 0x00, 0x01, 0x00, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real start heart of the run (export frames 17/21; the restored hearts
/// #41/#45 are byte-identical) — TX1 connected battery 0x4B, TX2 connected
/// battery 0x51, BOTH mutes 00, NC on level 0, voice 4, eq 3, reverb off
/// gear 0, signalMode 0, tail 74 (XOR(payload[0..16)) ⊕ 0x68: 0x1C ⊕ 0x68).
const HEART_RX_RUN_MUTES_OFF: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x01, 0x4B, 0x51, 0x00, 0x00, 0x01, 0x00, 0x04,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Real muted heart of the run (export frames 29/33) — [`HEART_RX_RUN_MUTES_OFF`]
/// with ONLY tx1_mute (payload[4] = frame byte 11) moved 00 → 01: TX2's
/// mute byte (payload[5]) and every other field stay put, tail drops
/// 0x74 → 0x75 per the heart reply checksum rule (0x1D ⊕ 0x68).
const HEART_RX_RUN_TX1_MUTED: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x01, 0x01, 0x4B, 0x51, 0x01, 0x00, 0x01, 0x00, 0x04,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_tx_mute round trip replays the owner run's frames end to end —
/// pre-run GET, op's inner fresh GET, write leg + ack, settle GET, the
/// restore's inner GET, restore SET + ack, restore settle GET, exit-guard
/// GET (eight rounds exactly as the guarded test drove them).
#[test]
fn set_tx_mute_roundtrip_replays_the_real_session() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_RUN_MUTES_OFF); // pre-run GET (frames 14/17)
    mock.push_reply(HEART_RX_RUN_MUTES_OFF); // op's inner GET (frames 18/21)
    mock.push_reply(SET_TX_MUTE_ACK_WIRE); // ack of frame 22 [00 01]
    mock.push_reply(HEART_RX_RUN_TX1_MUTED); // settle GET (frames 26/29)
    mock.push_reply(HEART_RX_RUN_TX1_MUTED); // restore's inner GET (frames 30/33)
    mock.push_reply(SET_TX_MUTE_ACK_WIRE); // ack of frame 34 [00 00]
    mock.push_reply(HEART_RX_RUN_MUTES_OFF); // restore settle GET (frames 38/41)
    mock.push_reply(HEART_RX_RUN_MUTES_OFF); // exit-guard GET (frames 42/45)

    let mut c = control(mock);

    // Pre-run poll: the guarded test's own GET before the op — shape
    // check, TX1-connected check, and the starting mutes (frames 14/17).
    let start = c
        .get_heart_info()
        .expect("real heart (mutes-off pair, residue 74) parses");
    assert!(start.tx1_status, "guard refuses unless TX1 is connected");
    assert!(
        !start.tx1_mute && !start.tx2_mute,
        "the run started at mutes-off — both write legs byte-equal their doc vectors"
    );

    // Write leg: the op's inner fresh GET (frames 18/21), SET frame 22,
    // ack frame 25 [00]+70.
    let previous = c
        .set_tx_mute(0, true)
        .expect("real ack [00]+70 must be accepted (residue ignored)");
    assert_eq!(
        previous,
        Some(false),
        "frames 18/21 rest at tx1_mute 00 (the last-known state)"
    );
    // Settle poll (frames 26/29).
    let after = c
        .get_heart_info()
        .expect("real heart (muted pair, residue 75) parses");
    assert!(
        after.tx1_mute,
        "heart delta = TX1 muted — offset 4 is the ONLY byte that moved (frames 29/33)"
    );
    assert!(
        !after.tx2_mute,
        "TX2's mute byte stayed 00 — one TX per frame on the wire"
    );
    assert!(after.tx1_status, "TX1 connected through the run");
    assert!(
        after.tx2_status,
        "TX2 was powered during the run (battery 0x51) — read-side only, never written"
    );
    assert_eq!(after.tx1_battery, 0x4B, "run's TX1 battery byte");
    assert_eq!(after.voice_level, 4, "run's gain byte");
    assert_eq!(
        after.signal_mode, 0,
        "signalMode 0 in this run — both tail readings coincide"
    );

    // Restore leg: inner GET (frames 30/33 — the fresh GET sees the write
    // leg's effect), SET frame 34, ack frame 37.
    let restore_previous = c
        .set_tx_mute(0, false)
        .expect("real restore must be accepted");
    assert_eq!(
        restore_previous,
        Some(true),
        "the restore's fresh GET saw the write leg's effect (frames 30/33)"
    );
    // Restore settle poll (frames 38/41).
    let restored = c.get_heart_info().expect("heart back at the start pair");
    assert!(
        !restored.tx1_mute,
        "restore delta verified — mutes off (frames 38/41)"
    );
    // Exit-guard poll (frames 42/45): the guard's own GET, already at
    // original ("nothing to do").
    let exit = c.get_heart_info().expect("exit-guard GET parses");
    assert!(
        !exit.tx1_mute,
        "exit guard found TX1 already at its original mute"
    );

    let written = c.transport().written();
    assert_eq!(
        &written[2], &SET_TX_MUTE_ON_TX1_WIRE,
        "SET TX byte-exact frame 22"
    );
    assert_eq!(
        &written[5], &SET_TX_MUTE_OFF_TX1_WIRE,
        "restore TX byte-exact frame 34"
    );
    // Lineage pins (roadmap §1(b)): the doc-recomputed layout and the
    // hardware-observed frames are the SAME bytes — the run started at
    // mutes-off, so both write legs are byte-equal as-is, and the ack tail
    // 0x70 the checksum rule predicted pre-run is byte-equal as-is
    // (n=6 for cmd ⊕ 0x66).
    assert_eq!(
        SET_TX_MUTE_ON_TX1_WIRE, SET_TX_MUTE_ON_TX1,
        "doc layout == observed wire (write leg)"
    );
    assert_eq!(
        SET_TX_MUTE_OFF_TX1_WIRE, SET_TX_MUTE_OFF_TX1,
        "doc layout == observed wire (restore leg)"
    );
    assert_eq!(SET_TX_MUTE_ACK, SET_TX_MUTE_ACK_WIRE, "ack lineage pin");
}

// ── Shutdown timer (cmd 39 SET_SHUTDOWN_TIME) — doc-recomputed vectors ──────
//
// Route (b) discipline (owner rule, same as EQ/gain/reverb/channel-mode/mute):
// mock vectors are RECOMPUTED FROM THE DOCS until the hardware run; real
// usbmon frames join as replay vectors after it. Every frame below is
// computed from docs only:
//   frame template       docs/protocol.md §2 (ProtocolV2.getData [code])
//   cmd 39, payload      docs/set-gate-checklist.md §3 Shutdown-power-off row
//                        ([{value}] — UsbMessengerV2.java:157-161; cmd from
//                        ProSetShutdownTime.getCmd() = GET_MUTE → 39
//                        [code PostProtocol.java:27])
//   value space 0/1      identity quarter("15min")→0, never("Never")→1
//                        (HLDevice6302Activity.java:1667-1671/:1672-1676);
//                        read side heart shutdownTime §5 offset 10
//                        (ProGetHearInfo.java:73), same identity
//                        (§5.2, :1864-1867)
// The heart reply is the wire-certified E1 heart (captures/
// 2026-11-05-hearinfo-first-wire.txt, which already rests at shutdownTime 0
// = 15min [wire]) with only the value byte moved — the 17-byte heart layout
// is [wire]. ACK tail prediction, owner-corrected: the wire cmd byte is
// decimal 39 = hex 0x27, so 0x27 ⊕ 0x66 = 0x41 (NOT 0x5F — that came from
// XORing the decimal digits as if they were hex).

/// SET_SHUTDOWN_TIME of Never (1) recomputed from the docs:
/// `05 03 AA DD 27 00 01 01 EF` + zero padding to 64 (big-endian len 00 01).
const SET_SHUTDOWN_1_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x27, 0x00, 0x01, 0x01, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// SET_SHUTDOWN_TIME ack recomputed from the docs: reply header `05 03 BB DD`
/// with cmd echo 27, len 00 01, status `00` (checklist §3 ACK rule: one
/// status byte, `data[0] == 0` = success — `ProSetShutdownTime.receiveData`
/// reads `toInt(data[0]) == 0` [code :26-30]). The byte behind the payload
/// follows the reply checksum rule XOR(frame[0..7+len)) ⊕ 0x07 — for cmd 27
/// the prefix folds to 0x46, 0x46 ⊕ 0x07 = 0x41 (the same byte the 6-shape
/// cmd ⊕ 0x66 shortcut predicts: 0x27 ⊕ 0x66 = 0x41, owner-corrected —
/// wire byte 0x27, not the decimal digits). The byte stays UNENFORCED by
/// the crate — it must ignore everything past `7+len`, so the vector still
/// doubles as an ignore-the-residue regression.
const SET_SHUTDOWN_ACK: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x27, 0x00, 0x01, 0x00, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The certified E1 heart with shutdownTime (payload offset 10 = frame byte
/// 17) moved 0 → 1: layout stays [wire] (E1); the flipped value byte is the
/// doc-recomputed delta side of route (b), and the residue drops to 0x36
/// per the heart reply checksum rule (XOR(payload[0..16)) ⊕ 0x68 — the LAST
/// payload byte, signalMode, is NOT in the fold; value 00^01 flips that
/// fold, 0x5F⊕0x68=0x37 → 0x5E⊕0x68=0x36). Every OTHER field stays put —
/// mode (voiceMode, offset 9) stays 0, gain stays 3, EQ stays 3, reverb
/// stays (off, gear 2).
const HEART_RX_SHUTDOWN1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x01, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// set_shutdown_time(1) (Never — the other documented value; the E1 heart
/// rests at shutdownTime 0 = 15min [wire, session 1]): the op must issue its
/// own fresh GET first (uniform SET discipline), then the byte-exact
/// doc-recomputed SET frame, and hand back the fresh GET's timer value as
/// the pre-write state.
#[test]
fn set_shutdown_time_tx_frame_is_the_apk_layout() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner fresh GET (device rests at 15min 0)
    mock.push_reply(SET_SHUTDOWN_ACK);
    let mut c = control(mock);

    let previous = c.set_shutdown_time(1).expect("ack 00 must be accepted");
    assert_eq!(
        previous, 0,
        "fresh GET inside set_shutdown_time = the timer value before write"
    );

    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX, "op must issue its own GET first");
    assert_eq!(
        &written[1], &SET_SHUTDOWN_1_TX,
        "SET shutdown time must be the doc-recomputed Never frame"
    );
    assert_eq!(
        written[1][4],
        cmd::SET_SHUTDOWN_TIME,
        "cmd echo = 39 = 0x27 [code ProSetShutdownTime.getCmd]"
    );
}

/// The documented value space is exactly two values (0=15min, 1=Never —
/// identity on both the write caller `HLDevice6302Activity.java:1667-1671/
/// :1672-1676` and the read renderer `:1864-1867`): each SET differs from
/// the Never frame in the value byte only (cmd stays 27; no second payload
/// byte).
#[test]
fn set_shutdown_time_value_space_changes_only_the_value_byte() {
    for value in [0u8, 1] {
        let mut mock = MockTransport::new();
        mock.push_reply(HEART_RX_E1);
        mock.push_reply(SET_SHUTDOWN_ACK);
        let mut c = control(mock);

        let previous = c.set_shutdown_time(value).expect("ack 00 must be accepted");
        assert_eq!(previous, 0, "fresh GET reported timer {value} INCOMING");
        let mut want = SET_SHUTDOWN_1_TX;
        want[7] = value; // value byte sits at frame offset 7 (tail EF at 8)
        assert_eq!(&c.transport().written()[1], &want, "timer {value}");
    }
}

/// Full doc-vector round trip (route (b), Mock half) in the shape the
/// guarded hardware test rehearses: device rests at shutdownTime 0 = 15min
/// (E1 IS that state [wire]), write the other documented value (Never 1),
/// ack `00`, heart settles shutdownTime → 1 (§5 offset 10) with every
/// neighbouring field untouched (mode stays 0, gain stays 3 — the timer
/// byte is the ONLY delta); restore 0, heart settles back. Frames asserted
/// both ways round trip; the restore TARGET comes from set_shutdown_time's
/// own return (the fresh GET inside the write), not a caller-side poll.
#[test]
fn set_shutdown_time_roundtrip_doc_vectors() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1); // inner GET of set_shutdown_time(1): device at 0
    mock.push_reply(SET_SHUTDOWN_ACK); // ack for SET_SHUTDOWN_TIME [01]
    mock.push_reply(HEART_RX_SHUTDOWN1); // post-SET fresh GET: delta = 1
    mock.push_reply(HEART_RX_SHUTDOWN1); // inner GET of the restore: device at 1
    mock.push_reply(SET_SHUTDOWN_ACK); // ack for SET_SHUTDOWN_TIME [00]
    mock.push_reply(HEART_RX_E1); // post-restore fresh GET: delta = 0

    let mut c = control(mock);
    let previous = c
        .set_shutdown_time(1)
        .expect("shutdown-timer write must be accepted");
    assert_eq!(
        previous, 0,
        "pre-write timer value from the op's own fresh GET"
    );
    let after = c.get_heart_info().expect("post-SET GET");
    assert_eq!(
        after.shutdown_time, 1,
        "heart delta must equal the written timer value"
    );
    assert_eq!(
        after.voice_mode, 0,
        "voiceMode (§5 offset 9, the neighbouring mode field) must never move"
    );
    assert_eq!(
        after.voice_level, 3,
        "voiceLevel (§5 offset 8, the neighbouring gain field) must never move"
    );
    assert_eq!(after.eq_level, 3, "eqLevel (§5 offset 12) must never move");
    assert_eq!(
        (after.reverb_switch, after.reverb_level),
        (false, 2),
        "reverb fields (§5 offsets 13/14) must never move"
    );
    let restore_previous = c
        .set_shutdown_time(previous)
        .expect("restore must be accepted");
    assert_eq!(restore_previous, 1, "fresh GET before restore saw timer 1");
    let restored = c.get_heart_info().expect("post-restore GET");
    assert_eq!(
        restored.shutdown_time, 0,
        "heart must be back at the start timer value"
    );
    assert_eq!(restored.voice_mode, 0, "voiceMode still untouched");

    let written = c.transport().written();
    let mut set0 = SET_SHUTDOWN_1_TX;
    set0[7] = previous; // value byte at frame offset 7 (tail EF at 8)
    assert_eq!(&written[1], &SET_SHUTDOWN_1_TX, "write leg is Never 1");
    assert_eq!(&written[4], &set0, "restore leg is the op's OWN return");
}

/// Ack gates shared with the SET_NOISE/SET_EQ/SET_GAIN/SET_REVERB/
/// SET_VOICE_MODE/SET_TX_MUTE families (checklist §3: uniform
/// `data[0] == 0` success across all `ProSet*` classes —
/// `ProSetShutdownTime.receiveData` [code :26-30]): nonzero status =
/// [`Error::Rejected`], wrong ack length = [`Error::BadAck`]. Reason same
/// as the other families: the ack shape has only ever been seen as `[00]`
/// [wire E2/E4].
#[test]
fn set_shutdown_time_rejects_abnormal_acks() {
    // nonzero status
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_SHUTDOWN_ACK;
    ack[7] = 0x03; // status byte != 0
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_shutdown_time(1).expect_err("nonzero ack must fail");
    assert!(
        matches!(err, Error::Rejected { status: 0x03, .. }),
        "{err:?}"
    );

    // two-byte ack (echo shape) must not be accepted
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut ack = SET_SHUTDOWN_ACK;
    ack[6] = 0x02; // payload len 2
    mock.push_reply(ack);
    let mut c = control(mock);
    let err = c.set_shutdown_time(1).expect_err("2-byte ack must fail");
    assert!(matches!(err, Error::BadAck { .. }), "{err:?}");
}
