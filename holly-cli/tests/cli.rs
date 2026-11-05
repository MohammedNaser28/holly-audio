//! holly-cli tests: argument parsing, output formatting, exit codes, and
//! runner behavior through holly-usb's MockTransport. No hardware.

use holly_cli::{
    EXIT_NOT_FOUND, EXIT_OTHER, EXIT_PERMISSION, EXIT_PROTOCOL, Failure, NcError, StatusJson,
    Watcher, exit_code, hex, onoff, run_nc, run_status, status_json, status_text,
};
use holly_usb::{Error, HollyControl, MockTransport};

// ---- capture vectors (captures/2026-11-05-*.txt, session 2) -----------------

const HEART_TX: [u8; 64] = [
    0x05, 0x03, 0xAA, 0xDD, 0x1F, 0x00, 0x00, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const HEART_RX_E1: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const SET_ACK_E2: [u8; 64] = [
    0x05, 0x03, 0xBB, 0xDD, 0x13, 0x00, 0x01, 0x00, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// E1 payload (17 bytes), for direct formatting tests.
fn e1_payload() -> Vec<u8> {
    HEART_RX_E1[7..24].to_vec()
}

// ---- argument parsing -------------------------------------------------------

#[test]
fn status_parses_with_and_without_json() {
    use clap::Parser;
    let cli = holly_cli::Cli::try_parse_from(["holly", "status"]).expect("parses");
    assert!(matches!(
        cli.command,
        holly_cli::Command::Status { json: false }
    ));
    let cli = holly_cli::Cli::try_parse_from(["holly", "status", "--json"]).expect("parses");
    assert!(matches!(
        cli.command,
        holly_cli::Command::Status { json: true }
    ));
}

#[test]
fn nc_parses_on_off_case_insensitively_and_rejects_other() {
    use clap::Parser;
    let cli = holly_cli::Cli::try_parse_from(["holly", "nc", "on"]).expect("parses");
    assert!(matches!(
        cli.command,
        holly_cli::Command::Nc {
            state: holly_cli::NcState::On
        }
    ));
    let cli = holly_cli::Cli::try_parse_from(["holly", "nc", "OFF"]).expect("parses");
    assert!(matches!(
        cli.command,
        holly_cli::Command::Nc {
            state: holly_cli::NcState::Off
        }
    ));
    let parsed = holly_cli::Cli::try_parse_from(["holly", "nc", "loudly"]);
    assert!(parsed.is_err(), "non on/off must be rejected by the parser");
    assert_eq!(holly_cli::parse_onoff("on"), Ok(holly_cli::NcState::On));
    assert_eq!(holly_cli::parse_onoff("off"), Ok(holly_cli::NcState::Off));
    assert!(holly_cli::parse_onoff("whatever").is_err());
}

#[test]
fn watch_interval_defaults_and_enforces_minimum() {
    use clap::Parser;
    let cli = holly_cli::Cli::try_parse_from(["holly", "watch"]).expect("parses");
    let holly_cli::Command::Watch { interval_ms } = cli.command else {
        panic!("expected Watch");
    };
    assert_eq!(interval_ms, 1000);
    let cli = holly_cli::Cli::try_parse_from(["holly", "watch", "--interval-ms", "249"]);
    assert!(cli.is_err(), "below minimum 250 must be rejected");
    let cli = holly_cli::Cli::try_parse_from(["holly", "watch", "--interval-ms", "250"])
        .expect("parses at minimum");
    assert!(matches!(
        cli.command,
        holly_cli::Command::Watch { interval_ms: 250 }
    ));
}

// exactly three subcommands exist — nothing may ever be added beyond these
#[test]
fn only_the_three_certified_subcommands_parse() {
    use clap::Parser;
    for args in [
        vec!["holly", "reboot"],
        vec!["holly", "ota"],
        vec!["holly", "pair"],
    ] {
        assert!(
            holly_cli::Cli::try_parse_from(&args).is_err(),
            "{args:?} must NOT parse (no such subcommand)"
        );
    }
}

#[test]
fn formatting_status_text_matches_certified_payload() {
    let info = holly_usb::HeartInfo::from_payload(&e1_payload()).expect("E1 payload parses");
    let text = status_text(&info);
    for fragment in [
        "Hollyland 6108 (3547:0407) status",
        "TX1: disconnected",
        "TX2: connected",
        "battery  93%",
        "Noise cancel: on (level 0)",
        "Light: on",
    ] {
        assert!(text.contains(fragment), "missing {fragment:?} in:\n{text}");
    }
    // multi-TX fields absent in the certified 17-byte shape
    assert!(!text.contains("TX3"), "no TX3 line expected:\n{text}");
}

#[test]
fn formatting_status_json_roundtrips() {
    let info = holly_usb::HeartInfo::from_payload(&e1_payload()).expect("E1 payload parses");
    let json = status_json(&info);
    let back: StatusJson = serde_json::from_str(&json).expect("our own JSON parses");
    assert!(back.noise_switch);
    assert_eq!(back.tx2_battery, 0x5D);
    assert!(json.contains("\"noise_switch\":true"));
    assert!(
        !json.contains("tx3"),
        "absent multi-TX fields must not appear"
    );
}

#[test]
fn formatting_onoff_and_hex() {
    assert_eq!(onoff(true), "on");
    assert_eq!(onoff(false), "off");
    assert_eq!(hex(&[0x05, 0x03, 0xBB, 0xDD]), "05 03 bb dd");
}

// ---- exit codes -------------------------------------------------------------

#[test]
fn exit_code_mapping() {
    assert_eq!(exit_code(&Error::NotFound), EXIT_NOT_FOUND);
    assert_eq!(
        exit_code(&Error::Usb(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied"
        ))),
        EXIT_PERMISSION
    );
    assert_eq!(
        exit_code(&Error::Usb(std::io::Error::other("gone"))),
        EXIT_OTHER
    );
    let ack = SET_ACK_E2;
    let raw = {
        let mut r = ack;
        r[7] = 0x01;
        r
    };
    assert_eq!(
        exit_code(&Error::Rejected { status: 0x01, raw }),
        EXIT_PROTOCOL
    );
    assert_eq!(exit_code(&Error::BadAck { len: 0, raw }), EXIT_PROTOCOL);
    assert_eq!(
        exit_code(&Error::CmdMismatch {
            got: 0x13,
            want: 0x1F
        }),
        EXIT_PROTOCOL
    );
}

/// --verbose detail: Rejected and BadAck failures carry the raw 64-byte
/// reply frame (hex), never swallowed.
#[test]
fn failure_detail_carries_raw_frame_for_rejected_and_bad_ack() {
    let mut raw = SET_ACK_E2;
    raw[7] = 0x02;
    let e = Error::Rejected { status: 0x02, raw };
    let f = Failure::from(&e);
    assert_eq!(f.code, EXIT_PROTOCOL);
    assert!(f.line.contains("rejected the SET"), "line: {}", f.line);
    let detail = f.detail.expect("Rejected must carry detail");
    assert!(
        detail.contains(&hex(&raw[..12])),
        "hex of raw frame in detail"
    );

    let e = Error::BadAck { len: 2, raw };
    let f = Failure::from(&e);
    assert!(f.detail.is_some(), "BadAck must carry detail too");
}

// ---- runners through MockTransport -----------------------------------------

fn control(mock: MockTransport) -> HollyControl<MockTransport> {
    HollyControl::new(mock)
}

#[test]
fn run_status_text_and_json_through_mock() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut c = control(mock);
    let text = run_status(&mut c, false).expect("status must succeed");
    assert!(text.contains("Noise cancel: on"), "{text}");

    // separate session for json (poll again)
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut c = control(mock);
    let json = run_status(&mut c, true).expect("status must succeed");
    assert!(json.starts_with('{') && json.ends_with('}'), "{json}");
}

/// `nc on`: the SET TX must derive from the fresh GET (level 0 certified),
/// and the fresh GET after the ack decides the printed state.
#[test]
fn run_nc_applies_and_verifies_through_mock() {
    let mut mock = MockTransport::new();
    // inside set_noise: fresh GET (E1), SET ack (E2); then run_nc's verify GET.
    // BUT: run_nc itself does NOT verify separately — holly-usb's set_noise
    // is verified by holly_cli via its own fresh GET, so: E1, ACK, E1.
    mock.push_reply(HEART_RX_E1);
    mock.push_reply(SET_ACK_E2);
    mock.push_reply(HEART_RX_E1);
    let mut c = control(mock);

    let info = run_nc(&mut c, true).expect("nc on must succeed");
    assert!(
        info.noise_switch,
        "verify-GET must show the requested state"
    );

    // writes: poll (from set_noise), SET, poll (verification) — SET frame
    // byte-exact: [1, 0] with isOpen=1, level=0 (E4 TX shape).
    let written = c.transport().written();
    assert_eq!(&written[0], &HEART_TX);
    let set_tx = &written[1];
    assert_eq!(set_tx[4], 0x13);
    assert_eq!(set_tx[7], 0x01);
    assert_eq!(set_tx[8], 0x00);
    assert_eq!(&written[2], &HEART_TX);
}

/// ack success but fresh GET shows the other state → NcError::Mismatch
/// (the CLI exits 4 on this; asserted here at library level).
#[test]
fn run_nc_reports_mismatch_when_fresh_get_disagrees() {
    let mut mock = MockTransport::new();
    let mut off = HEART_RX_E1;
    off[13] = 0x00; // payload[6] noiseSwitch -> 00 (off)
    mock.push_reply(HEART_RX_E1); // fresh GET inside set_noise
    mock.push_reply(SET_ACK_E2); // ack ok
    mock.push_reply(off); // verify GET: disagrees with requested "on"
    let mut c = control(mock);

    let err = run_nc(&mut c, true).expect_err("mismatch must surface");
    match err {
        NcError::Mismatch(info) => assert!(!info.noise_switch),
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

#[test]
fn watcher_prints_baseline_then_only_changes() {
    let mut mock = MockTransport::new();
    mock.push_reply(HEART_RX_E1);
    let mut off = HEART_RX_E1;
    off[13] = 0x00; // NC switch flips
    mock.push_reply(off);
    mock.push_reply(HEART_RX_E1); // back to baseline — still a CHANGE vs `off`
    mock.push_reply(HEART_RX_E1); // identical to the previous poll — no change
    let mut w = Watcher::new(control(mock));

    let first = w
        .poll()
        .expect("poll 1")
        .expect("first poll always changes");
    assert!(first.contains("nc=on"), "{first}");
    assert!(
        w.poll()
            .expect("poll 2")
            .expect("switch flip is a change")
            .contains("nc=off")
    );
    assert!(
        w.poll()
            .expect("poll 3")
            .expect("back to on is a change")
            .contains("nc=on")
    );
    assert!(
        w.poll().expect("poll 4").is_none(),
        "identical state prints nothing"
    );
}
