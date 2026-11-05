//! Actor tests — NO hardware. Deterministic `Core` drives plus one
//! real-thread loop test for wiring. All byte vectors are the certified
//! capture buffers (identical to holly-usb's tests/mock.rs):
//!   captures/2026-11-05-set-noise.txt           (E1-E5, session 2)
//!   captures/2026-11-05-hearinfo-first-wire.txt (session 1)
//! Expectations mirror docs/protocol.md §5/§6 and the actor safety rules
//! (abort on malformed, no retry loops, Rejected/BadAck surfaced).

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use holly_app::vm::view_model;
use holly_app::{Command, ConnStatus, Core, Hub, NoiseWrite, OpenFn, Snapshot, StatusSource, vm};
use holly_usb::{Error, HollyControl, Transport};

const REPORT: usize = 64;

// ---- certified vectors (verbatim from holly-usb/tests/mock.rs / captures/) --

/// E1 RX (heartbeat, session 2): nc on, tx2 connected, battery 0x5D = 93.
const HEART_E1: [u8; REPORT] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x01, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// E3 RX: identical payload except offset 6 — noiseSwitch 01 -> 00.
const HEART_E3: [u8; REPORT] = [
    0x05, 0x03, 0xBB, 0xDD, 0x1F, 0x00, 0x11, 0x00, 0x01, 0x00, 0x5D, 0x00, 0x00, 0x00, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// E2/E4 RX: SET ack — one status byte 0x00, residue 0x75 behind it.
const SET_ACK_OK: [u8; REPORT] = [
    0x05, 0x03, 0xBB, 0xDD, 0x13, 0x00, 0x01, 0x00, 0x75, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Never-captured shape: nonzero ack status byte. Per
/// `ProSetNoise.receiveData` [code], only `data[0] == 0` is success — the
/// actor must not swallow whatever the device means by nonzero.
fn ack_rejected(status: u8) -> [u8; REPORT] {
    let mut ack = SET_ACK_OK;
    ack[7] = status;
    ack
}

/// Never-captured shape: ack payload length 2 instead of the certified
/// 1-byte status shape → [`Error::BadAck`] (open question 5: no invented
/// semantics for other lengths).
fn ack_bad_len() -> [u8; REPORT] {
    let mut ack = SET_ACK_OK;
    ack[6] = 0x02;
    ack
}

// ---- shared scriptable transport -------------------------------------------

/// Queue of scripted control-transfer replies (Ok = a reply frame, Err =
/// an injected transport/protocol error).
type ReplyQueue = VecDeque<Result<[u8; REPORT], Error>>;

/// A mock transport whose queues stay reachable from the TEST even after the
/// source was boxed into the actor — the armable-mid-session need.
#[derive(Debug, Default, Clone)]
struct MockShared {
    written: Arc<Mutex<Vec<[u8; REPORT]>>>,
    replies: Arc<Mutex<ReplyQueue>>,
}

impl MockShared {
    fn push_reply(&self, reply: [u8; REPORT]) {
        self.replies.lock().unwrap().push_back(Ok(reply));
    }
    fn push_error(&self, error: Error) {
        self.replies.lock().unwrap().push_back(Err(error));
    }
    fn written(&self) -> Vec<[u8; REPORT]> {
        self.written.lock().unwrap().clone()
    }
}

#[derive(Debug)]
struct ScriptedTransport(MockShared);

impl Transport for ScriptedTransport {
    fn report_out(&mut self, report: &[u8; REPORT]) -> Result<(), Error> {
        self.0.written.lock().unwrap().push(*report);
        Ok(())
    }

    fn report_in(&mut self) -> Result<[u8; REPORT], Error> {
        // Never a live-mock: a dry queue is a scripted-authoring bug in
        // these tests — surface it plainly.
        self.0
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(Error::Usb(std::io::Error::other("scripted: empty queue"))))
    }
}

/// Wrap one scripted session into the actor's StatusSource box.
fn source_of(shared: &MockShared) -> Box<dyn StatusSource + Send> {
    Box::new(HollyControl::new(ScriptedTransport(shared.clone())))
}

/// Factory stepping through a scripted session list: Ok(session) | Err(claim error).
/// Exhausting the script reports NotFound ("no dongle"), which is exactly
/// what a test relying on that fact is asserting.
fn factory_of(steps: VecDeque<Result<MockShared, Error>>) -> OpenFn {
    let mut steps = steps;
    Box::new(move || match steps.pop_front() {
        Some(Ok(shared)) => Ok(source_of(&shared)),
        Some(Err(e)) => Err(e),
        None => Err(Error::NotFound),
    })
}

/// Deterministic core harness. The subscriber registers BEFORE `Core::new`,
/// so the stream contains the full baseline sequence (NotFound …).
fn core_of(steps: VecDeque<Result<MockShared, Error>>) -> (Core, Receiver<Snapshot>) {
    let hub = Arc::new(Hub::default());
    let rx = hub.subscribe();
    let core = Core::new(factory_of(steps), hub);
    (core, rx)
}

fn drain(rx: &mut Receiver<Snapshot>) -> Vec<Snapshot> {
    let mut snaps = Vec::new();
    while let Ok(s) = rx.try_recv() {
        snaps.push(s);
    }
    snaps
}

// ---- core tests ---------------------------------------------------------------

/// Clean connect + poll. ONE tick runs the whole Connecting → Connected
/// transition (the claim attempt happens inside the tick); the subscriber
/// stream records every intermediate state.
#[test]
fn connects_and_polls() {
    let session = MockShared::default();
    session.push_reply(HEART_E1);
    let (mut core, mut rx) = core_of(VecDeque::from([Ok(session.clone())]));

    core.tick();
    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Connected);
    let heart = snap.heart.expect("connected snapshots carry a heart");
    assert!(heart.noise_switch, "E1 nc on");
    assert_eq!(heart.tx2_battery, 0x5D, "certified battery byte");
    assert_eq!(heart.noise_level, 0);
    assert_eq!(heart.tx3_status, None, "17-byte reply has no tail");
    assert_eq!(session.written().len(), 1, "one GET poll so far");

    // Stream: baseline NotFound, Connecting, Connected — nothing after.
    let stream = drain(&mut rx);
    let statuses: Vec<ConnStatus> = stream.iter().map(|s| s.status.clone()).collect();
    assert_eq!(
        statuses,
        vec![
            ConnStatus::NotFound,
            ConnStatus::Connecting,
            ConnStatus::Connected
        ]
    );
    assert!(drain(&mut rx).is_empty(), "no extra emissions");
}

/// Claim keeps failing with NotFound — state stays NotFound and a queued
/// SetNoise request while disconnected is DROPPED (the UI disables its
/// toggle when no heart exists; the actor must not fake a pending state).
#[test]
fn startup_not_found_drops_set_requests() {
    let (mut core, _) = core_of(VecDeque::from([Err(Error::NotFound)]));

    core.tick();
    assert_eq!(core.snapshot().status, ConnStatus::NotFound);
    assert!(core.snapshot().heart.is_none());

    core.pump([Command::SetNoise(true)]);
    assert_eq!(core.snapshot().status, ConnStatus::NotFound);
    assert!(
        core.snapshot().pending_set.is_none(),
        "dropped while disconnected"
    );
}

/// Claim-stage Busy distinguishes itself from every other failure —
/// classification happens on the preserved std kind (ResourceBusy), which
/// the live parallel-claim run produced (holly-usb commit 0adcd2e,
/// "interface is busy", errno 16).
#[test]
fn claim_busy_maps() {
    let (mut core, _) = core_of(VecDeque::from([Err(Error::Usb(std::io::Error::from(
        std::io::ErrorKind::ResourceBusy,
    )))]));
    core.tick();
    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Busy);
    assert!(snap.detail.as_ref().unwrap().contains("busy"), "{snap:?}");
}

/// Claim-stage PermissionDenied likewise (live errno 13, udev grant missing).
#[test]
fn claim_permission_denied_maps() {
    let (mut core, _) = core_of(VecDeque::from([Err(Error::Usb(std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )))]));
    core.tick();
    assert_eq!(core.snapshot().status, ConnStatus::PermissionDenied);
}

/// Mid-session unplug: a transport error while connected drops the claim
/// (holly-usb's Drop path restores interface state) and the machine
/// re-enters NotFound with no stale heart; the next tick reconnects via a
/// fresh session and shows the new device state (E3: nc off now).
#[test]
fn mid_session_unplug_then_reconnect() {
    let session1 = MockShared::default();
    session1.push_reply(HEART_E1); // connect poll
    session1.push_error(Error::Usb(std::io::Error::from(
        std::io::ErrorKind::NotConnected,
    ))); // next poll = unplug
    let session2 = MockShared::default();
    session2.push_reply(HEART_E3); // reconnect poll

    let (mut core, _) = core_of(VecDeque::from([Ok(session1), Ok(session2)]));

    core.tick(); // Connecting -> Connected (E1)
    assert!(core.snapshot().heart.as_ref().unwrap().noise_switch);

    core.tick(); // poll -> NotConnected -> claim dropped
    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::NotFound);
    assert_eq!(snap.heart, None, "claim dropped; no stale heart displayed");

    core.tick(); // Connecting again (fresh claim, session 2)
    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Connected);
    assert!(!snap.heart.unwrap().noise_switch, "E3 nc off");
}

/// One SET in flight at a time, latest-wins coalescing: a mixed batch of
/// three SetNoise commands collapses into ONE SET frame carrying the LAST
/// value, then the actor's post-SET poll pushes the DEVICE's verdict.
///
/// Frame arithmetic per holly-usb's set_noise internals (fresh GET + SET =
/// 2 writes) plus the actor's verification poll:
///   writes = [GET poll, GET fresh, SET, GET verify]
#[test]
fn set_coalesces_to_latest() {
    let session = MockShared::default();
    session.push_reply(HEART_E1); // connect poll (nc on, level 0)
    session.push_reply(HEART_E1); // fresh GET inside set_noise
    session.push_reply(SET_ACK_OK); // ack
    session.push_reply(HEART_E3); // post-SET verification poll

    let (mut core, mut rx) = core_of(VecDeque::from([Ok(session.clone())]));
    core.tick(); // Connected
    assert_eq!(core.snapshot().status, ConnStatus::Connected);

    core.pump([
        Command::SetNoise(false),
        Command::SetNoise(true),
        Command::SetNoise(true),
    ]);

    // The ONLY SET frame carries the latest request (true); the level byte
    // comes from the fresh GET inside set_noise (E1: level 0) — payload
    // shape `13 00 02 {isOpen} {level} EF` [wire E4].
    let written = session.written();
    assert_eq!(written.len(), 4, "GET, fresh GET, SET, verify-GET");
    assert_eq!(&written[0][..4], &[0x05, 0x03, 0xAA, 0xDD], "TX magic");
    assert_eq!(written[0][4], 0x1F, "GET cmd");
    assert_eq!(written[1][4], 0x1F, "fresh GET cmd");
    assert_eq!(written[2][4], 0x13, "cmd 19 = SET_NOISE");
    assert_eq!(written[2][5], 0x00, "len BE");
    assert_eq!(written[2][6], 0x02, "payload len 2");
    assert_eq!(written[2][7], 0x01, "isOpen = true (latest-wins)");
    assert_eq!(written[2][8], 0x00, "level from the fresh GET");
    assert_eq!(written[2][9], 0xEF, "TX tail [wire]");
    assert_eq!(written[3][4], 0x1F, "verification GET");

    // Final snapshot shows the DEVICE's verdict, not the request: the
    // scripted verification reply (E3) says OFF even though we SET on — a
    // deliberate scripted disagreement that must be reported unvarnished.
    let final_snap = core.snapshot();
    assert_eq!(final_snap.status, ConnStatus::Connected);
    assert!(!final_snap.heart.expect("connected").noise_switch);
    assert_eq!(final_snap.pending_set, None, "no SET left in flight");

    // Pending became visible in the stream BEFORE execution: the pending
    // emission precedes the fresh-GET/SET/verify sequence.
    let stream = drain(&mut rx);
    let seq: Vec<(ConnStatus, Option<NoiseWrite>)> = stream
        .iter()
        .map(|s| (s.status.clone(), s.pending_set))
        .collect();
    assert_eq!(
        seq,
        vec![
            (ConnStatus::NotFound, None),
            (ConnStatus::Connecting, None),
            (ConnStatus::Connected, None), // first heart poll
            (
                ConnStatus::Connected,
                Some(NoiseWrite::Switch(true)) // pending BEFORE the SET
            ),
            (ConnStatus::Connected, None), // device verdict after the SET
        ]
    );
}

/// A SET the device REJECTS (nonzero ack status) is never swallowed: status
/// flips to Error, the detail keeps the Rejected text, and the snapshot
/// embeds a FRESH verification read — the real device state, not the request.
#[test]
fn rejected_set_is_surfaced_with_real_state() {
    let session = MockShared::default();
    session.push_reply(HEART_E1); // connect poll
    session.push_reply(HEART_E1); // fresh GET inside set_noise
    session.push_reply(ack_rejected(0x02)); // Rejected
    session.push_reply(HEART_E1); // post-failure verification GET

    let (mut core, _) = core_of(VecDeque::from([Ok(session)]));
    core.tick(); // Connected
    core.pump([Command::SetNoise(false)]);

    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Error);
    assert!(
        snap.detail.as_ref().unwrap().contains("rejected"),
        "Rejected text surfaced: {:?}",
        snap.detail
    );
    // Real state alongside the error — E1 says nc on.
    assert!(snap.heart.as_ref().expect("verification read").noise_switch);
    assert_eq!(snap.pending_set, None);
    // Device stays claimed (not NotFound): next poll arbitrates recovery.
}

/// A SET ack with an unexpected payload length (BadAck) surfaces the same way.
#[test]
fn bad_ack_set_is_surfaced_with_real_state() {
    let session = MockShared::default();
    session.push_reply(HEART_E1); // connect poll
    session.push_reply(HEART_E1); // fresh GET
    session.push_reply(ack_bad_len()); // BadAck
    session.push_reply(HEART_E3); // verification GET

    let (mut core, _) = core_of(VecDeque::from([Ok(session)]));
    core.tick();
    core.pump([Command::SetNoise(true)]);

    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Error);
    assert!(
        snap.detail.as_ref().unwrap().contains("unexpected length"),
        "{:?}",
        snap.detail
    );
    assert!(!snap.heart.as_ref().expect("verification read").noise_switch);
}

/// Malformed heartbeat mid-session is Error (abort; no retry — docs/protocol.md
/// §2), NOT a NotFound drop: the device is still physically there.
#[test]
fn malformed_heart_stays_claimed_as_error() {
    let session = MockShared::default();
    session.push_reply(HEART_E1);
    session.push_error(Error::HeartInfoShort { len: 5 });

    let (mut core, _) = core_of(VecDeque::from([Ok(session)]));
    core.tick();
    assert!(
        core.snapshot().detail.as_ref().is_none(),
        "connect poll was clean"
    );
    core.tick(); // malformed poll
    let snap = core.snapshot();
    assert_eq!(snap.status, ConnStatus::Error);
    assert!(
        snap.detail.as_ref().unwrap().contains("too short"),
        "HeartInfoShort text surfaced: {:?}",
        snap.detail
    );
}

/// Refresh while disconnected triggers an immediate reconnect attempt
/// (the factory's next step succeeds → Connected in the same pump).
#[test]
fn refresh_works_while_disconnected() {
    let s_ok = MockShared::default();
    s_ok.push_reply(HEART_E1);
    let (mut core, _) = core_of(VecDeque::from([Err(Error::NotFound), Ok(s_ok)]));

    core.tick();
    assert_eq!(core.snapshot().status, ConnStatus::NotFound);
    core.pump([Command::Refresh]);
    assert_eq!(core.snapshot().status, ConnStatus::Connected);
}

/// The real thread loop wired end to end (start_with_factory): commands in,
/// snapshots out, the SET path flows through the actual 1 s loop.
/// Scripted reply budget: polls while idle use E1; the SET consumes one more
/// E1 (fresh GET) + ack; after the SET the remaining polls read E3.
#[test]
fn thread_loop_end_to_end() {
    let session1 = MockShared::default();
    for _ in 0..8 {
        session1.push_reply(HEART_E1);
    }
    session1.push_reply(HEART_E1); // fresh GET inside set_noise
    session1.push_reply(SET_ACK_OK);
    let session2 = MockShared::default();
    for _ in 0..8 {
        session2.push_reply(HEART_E3);
    }
    session2.push_reply(HEART_E3); // post-SET verification poll

    let handle =
        holly_app::start_with_factory(factory_of(VecDeque::from([Ok(session1), Ok(session2)])));
    let rx = handle.subscribe();

    // Wait for Connected (idle ticks run CONNECTING->CONNECTED within ~2 s).
    let mut got_connected = false;
    for _ in 0..10 {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(s) if s.status == ConnStatus::Connected => {
                got_connected = true;
                break;
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    assert!(got_connected, "never reached Connected; stream stalled");

    // Toggle through the handle; the connect-loop session (session1) holds
    // the claim, so the SET consumes ITS fresh-GET/ack pair; the actor's
    // poll afterwards reads the next scripted reply — E3 (nc off).
    handle.set_noise(false);
    let mut saw_off = false;
    for _ in 0..15 {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(s)
                if s.status == ConnStatus::Connected
                    && s.heart.as_ref().is_some_and(|h| !h.noise_switch) =>
            {
                saw_off = true;
                break;
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    assert!(saw_off, "device verdict nc=off never arrived");
}

/// JSON shape of the machine-readable snapshot — the published contract any
/// frontend (gpui/Tauri/…) mirrors. serde renames: enum variants snake_case;
/// HeartInfo fields verbatim (holly-usb serde feature).
#[test]
fn snapshot_json_shape() {
    let mock = MockShared::default();
    mock.push_reply(HEART_E1);
    let mut ctl = HollyControl::new(ScriptedTransport(mock));
    let heart = ctl.get_heart_info().expect("E1 parses");

    let snap = Snapshot {
        status: ConnStatus::Connected,
        detail: None,
        heart: Some(heart),
        pending_set: Some(NoiseWrite::Switch(true)),
    };
    let value: serde_json::Value = serde_json::to_value(&snap).expect("serializable");

    let expected = serde_json::json!({
        "status": "connected",
        "detail": null,
        "heart": {
            "tx1_status": false,
            "tx2_status": true,
            "tx1_battery": 0,
            "tx2_battery": 93,
            "tx1_mute": false,
            "tx2_mute": false,
            "noise_switch": true,
            "noise_level": 0,
            "voice_level": 3,
            "voice_mode": 0,
            "shutdown_time": 0,
            "light_on": true,
            "eq_level": 3,
            "reverb_switch": false,
            "reverb_level": 2,
            "tx_identify": false,
            "signal_mode": 0,
            "tx3_status": null,
            "tx4_status": null,
            "tx3_battery": null,
            "tx4_battery": null,
            "tx3_mute": null,
            "tx4_mute": null,
            "raw": [0, 1, 0, 93, 0, 0, 1, 0, 3, 0, 0, 0, 3, 0, 2, 0, 0]
        },
        "pending_set": {"switch": true}
    });
    assert_eq!(value, expected, "published JSON shape changed");
}

/// The view-model is the frontend's contract: pure Snapshot -> ViewModel.
/// (Deeper vm cases live in vm.rs's own unit tests; this one checks the
/// connected path against a real captured heart.)
#[test]
fn view_model_connected_shape() {
    let mock = MockShared::default();
    mock.push_reply(HEART_E1);
    let mut ctl = HollyControl::new(ScriptedTransport(mock));
    let heart = ctl.get_heart_info().expect("E1 parses");

    let vm = view_model(&Snapshot {
        status: ConnStatus::Connected,
        detail: None,
        heart: Some(heart),
        pending_set: Some(NoiseWrite::Switch(false)),
    });
    assert_eq!(vm.banner, None);
    assert_eq!(vm.banner_text, None);
    let tx2 = vm.tx2.expect("tx2 connected");
    assert!(tx2.connected);
    assert_eq!(tx2.battery, 93);
    assert_eq!(tx2.battery_class, vm::BatteryClass::Full);
    assert!(no_tail_txs(&vm), "17-byte replies have no tail TXs");
    assert_eq!(vm.nc.state, Some(true));
    assert_eq!(
        vm.nc.pending,
        Some(NoiseWrite::Switch(false)),
        "actor-reported pending passes through"
    );
    assert_eq!(vm.voice_level, Some(3));
    assert_eq!(vm.eq_level, Some(3));
    assert_eq!(
        vm.reverb,
        Some(vm::ReverbVm {
            enabled: false,
            level: 2
        })
    );
}

fn no_tail_txs(vm: &vm::ViewModel) -> bool {
    vm.tx3.is_none() && vm.tx4.is_none()
}

/// The level write path through the actor: a mixed batch collapses to ONE
/// `set_noise_level` SET carrying the LAST requested level; the fresh GET
/// inside (holly-usb) supplies the CURRENT switch state; the post-SET poll
/// reports the DEVICE's verdict. Scripted heart clones match E1 with only
/// payload[7] (noise level) changed — the level byte itself.
#[test]
fn set_noise_level_coalesces_to_latest() {
    let session = MockShared::default();
    session.push_reply(HEART_E1); // connect poll (nc on, level 0)
    session.push_reply(HEART_E1); // fresh GET inside set_noise_level
    session.push_reply(SET_ACK_OK); // ack
    let mut verdict = HEART_E1;
    verdict[14] = 0x02; // payload[7] = noiseLevel 00 -> 02 (strong)
    session.push_reply(verdict); // actor's verification poll

    let (mut core, mut rx) = core_of(VecDeque::from([Ok(session.clone())]));
    core.tick();
    assert_eq!(core.snapshot().status, ConnStatus::Connected);

    core.pump([
        Command::SetNoiseLevel(0),
        Command::SetNoise(true),
        Command::SetNoiseLevel(2), // latest-wins: only THIS write goes out
    ]);

    // writes = [GET poll, GET fresh, SET, verify-GET]; the ONLY SET carries
    // `{isOpen 01, level 02}` — APK strong [code HLDevice6302Activity:1593],
    // shape [wire E2/E4].
    let written = session.written();
    assert_eq!(written.len(), 4);
    assert_eq!(written[2][4], 0x13, "SET_NOISE");
    assert_eq!(written[2][6], 0x02, "payload len 2");
    assert_eq!(written[2][7], 0x01, "isOpen from the fresh GET");
    assert_eq!(written[2][8], 0x02, "level = latest request (strong)");
    assert_eq!(written[3][4], 0x1F, "verification GET");

    let final_snap = core.snapshot();
    assert_eq!(final_snap.status, ConnStatus::Connected);
    let heart = final_snap.heart.expect("connected");
    assert!(heart.noise_switch, "switch verdict stays the device's");
    assert_eq!(heart.noise_level, 2, "device verdict: level 2");
    assert_eq!(final_snap.pending_set, None);

    let stream = drain(&mut rx);
    let pendings: Vec<Option<NoiseWrite>> = stream.iter().map(|s| s.pending_set).collect();
    let last_pending = pendings.last().copied().flatten();
    assert_eq!(last_pending, None, "verdict snapshot clears pending");
    let seq: Vec<Option<NoiseWrite>> = pendings;
    let some_count = seq.iter().filter(|p| p.is_some()).count();
    assert_eq!(
        some_count, 1,
        "pending must appear exactly once in the stream"
    );
    assert_eq!(
        seq.iter().find(|p| p.is_some()).copied().flatten(),
        Some(NoiseWrite::Level(2)),
        "the single pending is the winning Level(2)"
    );
}
