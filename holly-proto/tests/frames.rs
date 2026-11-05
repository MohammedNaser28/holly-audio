//! Byte-exact tests for holly-proto.
//!
//! Every expected byte sequence below is recomputed from the evidence chain in
//! `docs/protocol.md` (§2 frame format) — NOT copied from any device capture.
//! When captures/ frames become available, each should get a replay test here
//! (decode → re-encode → assert byte equality) so no capture-derived behavior
//! stays untested.

use holly_proto::{CONTROL_BUF_SIZE, Frame, HEADER, cmd};

/// docs/protocol.md §4 example: SET_VOICE_LEVEL value=5.
/// Layout: HEADER(05 03 AA DD) + cmd 03 + len 00 01 + payload 05 + EF.
/// Trace: ProtocolV2.getData() offsets; payload {5} from UsbMessengerV2.java:180.
#[test]
fn set_voice_level_value_5_exact() {
    let frame = Frame::new(cmd::SET_VOICE_LEVEL, vec![0x05]);
    let buf = frame.encode_buf().unwrap();
    let mut expected = [0u8; CONTROL_BUF_SIZE];
    expected[..9].copy_from_slice(&[0x05, 0x03, 0xAA, 0xDD, 0x03, 0x00, 0x01, 0x05, 0xEF]);
    assert_eq!(buf, expected);
}

/// docs/protocol.md §4 example: GET_VOICE_LEVEL.
/// Empty payload: length bytes stay 0x00, EF lands at offset 7
/// (getData() writes len+payload+EF at 5+i/7+len only for non-empty payloads).
#[test]
fn get_voice_level_exact_empty_payload() {
    let frame = Frame::new(cmd::GET_VOICE_LEVEL, vec![]);
    let buf = frame.encode_buf().unwrap();
    let mut expected = [0u8; CONTROL_BUF_SIZE];
    expected[..8].copy_from_slice(&[0x05, 0x03, 0xAA, 0xDD, 0x04, 0x00, 0x00, 0xEF]);
    assert_eq!(buf, expected);
}

/// SET_NOISE (cmd 19) with {isOpen=1, level=5}:
/// UsbMessengerV2.java:173 — payload [isOpen ? 1 : 0, level].
#[test]
fn set_noise_payload_layout() {
    let frame = Frame::new(cmd::SET_NOISE, vec![0x01, 0x05]);
    let bytes = frame.encode().unwrap();
    assert_eq!(
        bytes,
        vec![0x05, 0x03, 0xAA, 0xDD, 0x13, 0x00, 0x02, 0x01, 0x05, 0xEF]
    );
}

/// SET_TX_MUTE (cmd 22) with {txIndex=0, muted=1}:
/// UsbMessengerV2.java:209 — payload [coerce(tx-1), muted].
#[test]
fn set_tx_mute_payload_layout() {
    let frame = Frame::new(cmd::SET_TX_MUTE, vec![0x00, 0x01]);
    let bytes = frame.encode().unwrap();
    assert_eq!(
        bytes,
        vec![0x05, 0x03, 0xAA, 0xDD, 0x16, 0x00, 0x02, 0x00, 0x01, 0xEF]
    );
}

#[test]
fn decode_reverses_encode() {
    let cases = [
        Frame::new(cmd::GET_HEART_INFO, vec![]),
        Frame::new(cmd::SET_VOICE_MODE, vec![0x02]),
        Frame::new(cmd::SET_NOISE, vec![0x00, 0x03]),
        Frame::new(
            cmd::GET_DEVICE_VERSION,
            (0u8..=0x37).collect::<Vec<u8>>(), // 56-byte max payload
        ),
    ];
    for frame in cases {
        let bytes = frame.encode().unwrap();
        let decoded = Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
        // The padded 64-byte transfer buffer must decode identically.
        let padded = frame.encode_buf().unwrap();
        assert_eq!(Frame::decode(&padded).unwrap(), frame);
    }
}

#[test]
fn decode_rejects_bad_magic() {
    // Byte 2 is direction-selective since the [wire] capture: AA = host frames,
    // BB = device replies (captures/2026-11-05-hearinfo-first-wire.txt). An
    // unknown value there is bad magic.
    let mut bytes = Frame::new(cmd::GET_VOICE_LEVEL, vec![]).encode().unwrap();
    bytes[2] = 0xCC;
    assert!(Frame::decode(&bytes).is_err());
}

#[test]
fn decode_accepts_device_reply_magic() {
    // BB in byte 2 means device→host; such frames need no EF terminator.
    let mut bytes = Frame::new(cmd::GET_VOICE_LEVEL, vec![]).encode().unwrap();
    bytes[2] = 0xBB;
    assert!(Frame::decode(&bytes).is_ok());
}

#[test]
fn decode_rejects_bad_tail() {
    let mut bytes = Frame::new(cmd::GET_VOICE_LEVEL, vec![]).encode().unwrap();
    *bytes.last_mut().unwrap() = 0xFE;
    assert!(Frame::decode(&bytes).is_err());
}

#[test]
fn decode_rejects_truncated() {
    // Claims 2-byte payload but tail is cut off.
    let mut bytes = Frame::new(cmd::SET_NOISE, vec![0x01, 0x05])
        .encode()
        .unwrap();
    bytes.pop();
    assert!(Frame::decode(&bytes).is_err());
    // Dangling length pointing past the buffer.
    let junk = [0x05, 0x03, 0xAA, 0xDD, 0x03, 0x00, 0x20, 0xEF];
    assert!(Frame::decode(&junk).is_err());
}

#[test]
fn decode_rejects_short_input() {
    assert!(Frame::decode(&HEADER[..3]).is_err());
    assert!(Frame::decode(&[]).is_err());
}

/// Maximum legal payload (56 bytes) still encodes inside one 64-byte buffer;
/// one byte more must be refused. Derived constant, see MAX_PAYLOAD_LEN docs.
#[test]
fn payload_length_limits() {
    assert!(
        Frame::new(
            cmd::GET_DEVICE_VERSION,
            vec![0u8; holly_proto::MAX_PAYLOAD_LEN]
        )
        .encode_buf()
        .is_ok()
    );
    let err = Frame::new(
        cmd::GET_DEVICE_VERSION,
        vec![0u8; holly_proto::MAX_PAYLOAD_LEN + 1],
    )
    .encode()
    .unwrap_err();
    assert!(matches!(
        err,
        holly_proto::EncodeError::PayloadTooLong { .. }
    ));
}

/// The app's XOR checksum (ProtocolV2.crcCalculate) is a straight fold.
/// It is not on the wire for normal frames; this pins the semantics we
/// documented in case capture analysis needs it for OTA blocks.
#[test]
fn xor_checksum_semantics() {
    assert_eq!(holly_proto::xor_checksum(&[]), 0);
    assert_eq!(holly_proto::xor_checksum(&[0x05, 0x03, 0xAA, 0xDD]), 0x71);
    assert_eq!(holly_proto::xor_checksum(&[0xAA, 0x55]), 0xFF);
}

#[test]
fn command_names_cover_known_opcodes() {
    assert_eq!(cmd::name(cmd::GET_HEART_INFO), Some("GET_HEART_INFO"));
    assert_eq!(cmd::name(cmd::SET_VOICE_LEVEL), Some("SET_VOICE_LEVEL"));
    // Opcodes never enumerated by the app must not get invented names.
    assert_eq!(cmd::name(0), None);
    assert_eq!(cmd::name(255), None);
}

/// Live [wire] capture replay: the dongle's actual response to the status poll
/// (cmd 31 GET_HEART_INFO), captured 2026-11-05 on /dev/bus/usb/006/002 via
/// usbfs SET_REPORT/GET_REPORT (Feature id 5). Source of truth:
/// `captures/2026-11-05-hearinfo-first-wire.txt`, layout analysis in
/// docs/protocol.md §2 (RX framing) and §5 (field map).
///
/// First live proof points this test pins:
///   - reply preamble is `05 03 BB DD` (device→host `BB`, not the host `AA`)
///   - length field `00 11` = 17 payload bytes (hi=0: BE encode and the
///     APK's `hi*16+lo` decode coincide; see decode() in src)
///   - NO `0xEF` terminator in the reply (would-be tail byte is `0x09`)
#[test]
fn decode_live_device_reply_get_heart_info() {
    let reply: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x63, 0x00, 0x00, 0x01, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let frame = Frame::decode(&reply).expect("live reply must decode");
    assert_eq!(frame.command, cmd::GET_HEART_INFO);
    // 17 payload bytes = §5 field map, contiguous: tx1Status, tx2Status,
    // tx1Battery, tx2Battery(=0x63=99!), ... through signalMode at off 16.
    assert_eq!(
        frame.payload,
        vec![
            0x00, 0x01, 0x00, 0x63, 0x00, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x00,
            0x02, 0x00, 0x00,
        ]
    );
}

/// Same capture, TX side: the host's poll frame must decode as well (AA + EF),
/// and its 64-byte padded shape must round-trip through decode().
#[test]
fn decode_live_host_poll_get_heart_info() {
    let sent: [u8; CONTROL_BUF_SIZE] = {
        let mut b = [0u8; CONTROL_BUF_SIZE];
        b[..8].copy_from_slice(&[0x05, 0x03, 0xAA, 0xDD, 0x1F, 0x00, 0x00, 0xEF]);
        b
    };
    let frame = Frame::decode(&sent).expect("host poll frame must decode");
    assert_eq!(frame.command, cmd::GET_HEART_INFO);
    assert!(frame.payload.is_empty());
}

/// Device replies must NOT demand the host terminator: the same 17-byte reply
/// with AA instead of BB would fail the tail check; with BB and a garbage tail
/// byte it must still decode (the app's parse() ignores magic/tail too).
#[test]
fn reply_direction_has_no_tail_requirement() {
    let frame = Frame::new(cmd::GET_HEART_INFO, vec![0x42]);
    let mut wire = frame.encode().unwrap();
    assert_eq!(wire[2], 0xAA);
    // turn it into a device-shaped reply: swap AA->BB, drop EF, append junk
    wire[2] = 0xBB;
    wire.pop(); // remove EF
    wire.push(0x09);
    wire.push(0x00);
    let decoded = Frame::decode(&wire).unwrap();
    assert_eq!(decoded.payload, vec![0x42]);
}

/// Task-1 [wire] replays from captures/2026-11-05-set-noise.txt
/// (full SET_NOISE round trip; TX bytes and ACK reply bytes are generated from
/// the capture file's builders, verified 64 bytes each, byte-exact to what the
/// dongle exchanged on 2026-11-05 via usbfs SET_REPORT/GET_REPORT id 5).
///
/// E2 TX: SET_NOISE { isOpen: off, level: 0 } exactly as the app encodes it
/// (Payload of [isOpen?1:0, level] from UsbMessengerV2.java:173). Byte-exact:
/// encode() output must equal the captured 64-byte padded transfer buffer.
#[test]
fn set_noise_off_tx_live_replay() {
    let tx: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xAA, 0xDD, 0x13, 0x00, 0x02, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let frame = Frame::new(cmd::SET_NOISE, vec![0x00, 0x00]);
    assert_eq!(frame.encode_buf().unwrap(), tx);
    assert_eq!(Frame::decode(&tx).unwrap(), frame);
}

/// E4 TX: SET_NOISE { isOpen: on, level: 0 } (restore leg).
#[test]
fn set_noise_on_tx_live_replay() {
    let tx: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xAA, 0xDD, 0x13, 0x00, 0x02, 0x01, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let frame = Frame::new(cmd::SET_NOISE, vec![0x01, 0x00]);
    assert_eq!(frame.encode_buf().unwrap(), tx);
}

/// E2/E4 RX: SET_NOISE ACK - device->host frame with ONE status byte 0x00
/// (success). This pins the acknowledged status-only semantics: the value is
/// NOT echoed (ProSetNoise.receiveData reads data[0] only) [code+wire].
#[test]
fn set_noise_ack_live_replay() {
    let ack: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xBB, 0xDD, 0x13, 0x00, 0x01, 0x00, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let frame = Frame::decode(&ack).expect("ack must decode");
    assert_eq!(frame.command, cmd::SET_NOISE);
    assert_eq!(frame.payload, vec![0x00]);
    // status-only: a 2-byte-echo payload ([isOpen, level]) would be 2 long.
    assert_ne!(frame.payload.len(), 2);
}

/// E1/E3/E5: heart replies around the toggle - payload flips at offset 6 ONLY
/// (noiseSwitch 1->0->1); nothing else moves. Final state == start state.
#[test]
fn heart_reply_toggle_congruence_live_replay() {
    let base: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let off: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let fin: [u8; CONTROL_BUF_SIZE] = [
        0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    let b = Frame::decode(&base).unwrap();
    let o = Frame::decode(&off).unwrap();
    let f = Frame::decode(&fin).unwrap();
    for frame in [&b, &o, &f] {
        assert_eq!(frame.command, cmd::GET_HEART_INFO, "cmd echo must survive");
        assert_eq!(frame.payload.len(), 17);
    }
    let diffs: Vec<usize> = (0..17).filter(|&i| b.payload[i] != o.payload[i]).collect();
    assert_eq!(diffs, vec![6], "only noiseSwitch (payload off 6) moves");
    assert_eq!(f.payload, b.payload, "restore state == start state");
}
