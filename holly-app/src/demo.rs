//! Demo device — a scripted stand-in typed as [`StatusSource`] so the real
//! actor (and every UI built on it) can run with NO hardware, by choice and
//! visibly: the frontend renders an explicit "DEMO" marker while this is in
//! play. Never claims any USB interface, never announces itself as real
//! hardware data (project rule: mock data is never presented as device
//! status).
//!
//! Semantics mirror the real device where it is wire-known ([wire]
//! sessions 1-3 of `captures/2026-11-05-noise-level-medium.txt` +
//! `holly-usb`'s ops): the SET path behaves like `HollyControl::set_noise*`
//! — a switch flip preserves the level byte, a level write preserves the
//! switch byte — and `poll()` answers with the current state. Values are
//! the certified session-3 frame (ON, level 1, tx2 online at 93 %).
//!
//! This is intentionally dumb: no disconnects, no protocol-level error
//! injection. Broken transport / busy / permission / malformed branches are
//! the job of the actor tests (tests/actor.rs), not of a UI demo — the one
//! exception is the BUSY SCENARIO below, which replays exactly one real
//! OS claim failure so a UI can be photographed in the busy-banner state
//! without a second process holding the dongle.

use std::sync::Mutex;
use std::time::Duration;

use crate::{DeviceHandle, OpenFn, StatusSource};
use holly_usb::{Error, HeartInfo};

/// Certified session-3 heart payload (17 bytes): nc ON, level 1 (medium),
/// tx1 off / tx2 connected, battery 0x5D = 93, gain 3, eq 3, reverb
/// off (level 2). Same vector the actor/vm tests parse.
const DEMO_PAYLOAD: [u8; 17] = [0, 1, 0, 93, 0, 0, 1, 1, 3, 0, 0, 0, 3, 0, 2, 0, 0];

/// How long a SET stalls in the [`DemoScenario::Pending`] scenario. The
/// actor publishes the pending snapshot BEFORE the blocking SET (actor.rs
/// `pump`), so the frontend shows "Applying" over the previous device state
/// for this long, then lands the write and resumes — a bounded window a
/// screenshot run can rely on, with no timer of its own.
const PENDING_STALL: Duration = Duration::from_secs(20);

/// The scripted session a `--demo` flag selects. Parsing lives here so
/// every frontend spells the same words the same way (single source).
///
/// - `connected` — the certified session-3 state, SETs instantly applied.
/// - `pending`   — same state, but each SET stalls for [`PENDING_STALL`]:
///   the UI can be photographed with an in-flight write (dimmed pad,
///   "Applying") and the write still completes to real state afterwards.
/// - `busy`      — every open attempt fails with the real OS busy error
///   (`ErrorKind::ResourceBusy`, the exact shape lib.rs documents); the
///   actor classifies it like a dongle held by another process. No claim,
///   no USB, busy banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoScenario {
    Connected,
    Pending,
    Busy,
}

impl DemoScenario {
    /// One `--demo` word -> scenario; the bare flag maps to `connected`.
    /// `None` = unrecognized word (the frontend decides what to do: usage
    /// error, never a silent fallback to a session the user did not pick).
    pub fn from_word(word: Option<&str>) -> Option<Self> {
        match word.unwrap_or("") {
            "" | "connected" => Some(Self::Connected),
            "pending" => Some(Self::Pending),
            "busy" => Some(Self::Busy),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct DemoDevice {
    heart: Mutex<HeartInfo>,
    /// ONE-SET stall ([`DemoScenario::Pending`] only): lets a UI be
    /// photographed with an in-flight write. `Duration::ZERO` for every
    /// other session — `thread::sleep(ZERO)` returns immediately, so
    /// `connected` keeps the real device's instant verdict.
    stall: Duration,
}

impl DemoDevice {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            heart: Mutex::new(HeartInfo::from_payload(&DEMO_PAYLOAD)?),
            stall: Duration::ZERO,
        })
    }

    /// The pending scenario's device: same certified state, SETs stall for
    /// `stall` before mutating (the actor's pending snapshot is already
    /// out by then).
    pub fn stalled(stall: Duration) -> Result<Self, Error> {
        Ok(Self {
            heart: Mutex::new(HeartInfo::from_payload(&DEMO_PAYLOAD)?),
            stall,
        })
    }
}

impl StatusSource for DemoDevice {
    fn poll(&mut self) -> Result<HeartInfo, Error> {
        Ok(self.heart.lock().unwrap().clone())
    }

    fn set_noise(&mut self, on: bool) -> Result<(), Error> {
        // Real device semantics: the switch write does not touch the level
        // byte (reader-side proof: capture session 3b + APK switch path
        // `HLDevice6302Activity.java:620-646` [code]).
        // Pending scenario: stall BEFORE mutation and outside the lock, so
        // `poll()` is never blocked behind a sleeping setter.
        std::thread::sleep(self.stall);
        self.heart.lock().unwrap().noise_switch = on;
        Ok(())
    }

    fn set_noise_level(&mut self, level: u8) -> Result<(), Error> {
        // Real device semantics: the level write leaves the switch byte
        // untouched (guarded round-trip test, capture session 3a).
        std::thread::sleep(self.stall);
        self.heart.lock().unwrap().noise_level = level;
        Ok(())
    }

    fn set_eq(&mut self, pad: u8) -> Result<u8, Error> {
        // Same shape as holly-usb's set_eq: the RETURN is the pad the heart
        // held BEFORE the write (guarded flows restore from it); the write
        // mutates only the eqLevel byte, everything else stays scripted.
        // Pending scenario: stall BEFORE mutation and outside the lock, so
        // `poll()` is never blocked behind a sleeping setter.
        let mut heart = self.heart.lock().unwrap();
        let previous = heart.eq_level;
        std::thread::sleep(self.stall);
        heart.eq_level = pad;
        Ok(previous)
    }

    fn set_gain(&mut self, stop: u8) -> Result<u8, Error> {
        // Same shape as holly-usb's set_gain: the RETURN is the stop the
        // heart held BEFORE the write (guarded flows restore from it); the
        // write mutates only the voiceLevel byte (§5 offset 8 — voiceMode
        // offset 9 is a separate heart field), everything else stays
        // scripted. Pending scenario: stall BEFORE mutation and outside
        // the lock, so `poll()` is never blocked behind a sleeping setter.
        let mut heart = self.heart.lock().unwrap();
        let previous = heart.voice_level;
        std::thread::sleep(self.stall);
        heart.voice_level = stop;
        Ok(previous)
    }

    fn set_reverb(&mut self, switch: bool, gear: u8) -> Result<(bool, u8), Error> {
        // Same shape as holly-usb's set_reverb: the RETURN is the (switch,
        // gear) pair the heart held BEFORE the write (guarded flows restore
        // from it); the write mutates BOTH reverb bytes — the real SET
        // carries `[isOpen, gear]` and the 2026-11-11 wire round trip
        // settled the pair on every leg (§5 offsets 13/14) — everything
        // else stays scripted. Pending scenario: stall BEFORE mutation, so
        // the pending snapshot is always observable first (same shape as
        // set_gain).
        let mut heart = self.heart.lock().unwrap();
        let previous = (heart.reverb_switch, heart.reverb_level);
        std::thread::sleep(self.stall);
        heart.reverb_switch = switch;
        heart.reverb_level = gear;
        Ok(previous)
    }
}

/// The OS busy condition, byte-for-byte the real claim failure shape
/// (lib.rs documents `Error::Usb(Custom { kind: ResourceBusy, error:
/// "interface is busy (errno 16)" })` and nusb maps errno 16 the same
/// way — transport.rs::usb_err); `classify_claim` turns exactly this
/// kind into [`crate::ConnStatus::Busy`].
fn busy_error() -> Error {
    Error::Usb(std::io::Error::new(
        std::io::ErrorKind::ResourceBusy,
        "interface is busy (errno 16)",
    ))
}

/// Actor over [`DemoDevice`] with the standard 1 s loop — same
/// [`DeviceHandle`] API as real hardware, zero USB.
pub fn start_demo() -> DeviceHandle {
    start_demo_scenario(DemoScenario::Connected)
}

/// Actor over a chosen [`DemoScenario`] — same [`DeviceHandle`] API for
/// every one of them; only the scripted factory differs.
pub fn start_demo_scenario(sc: DemoScenario) -> DeviceHandle {
    let factory: OpenFn = match sc {
        DemoScenario::Connected => {
            Box::new(|| DemoDevice::new().map(|dev| Box::new(dev) as Box<dyn StatusSource + Send>))
        }
        DemoScenario::Pending => Box::new(|| {
            DemoDevice::stalled(PENDING_STALL)
                .map(|dev| Box::new(dev) as Box<dyn StatusSource + Send>)
        }),
        DemoScenario::Busy => Box::new(|| Err(busy_error())),
    };
    crate::start_with_factory(factory)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{DemoDevice, DemoScenario, busy_error};
    use crate::{Command, ConnStatus, Core, Hub, StatusSource};

    fn demo_factory() -> crate::OpenFn {
        Box::new(|| DemoDevice::new().map(|dev| Box::new(dev) as Box<dyn StatusSource + Send>))
    }

    fn demo_core() -> Core {
        Core::new(demo_factory(), Arc::new(Hub::default()))
    }
    #[test]
    fn demo_connects_and_reports_the_certified_state() {
        let mut core = demo_core();
        core.tick(); // open_attempt + first poll
        let snap = core.snapshot();
        assert_eq!(snap.status, ConnStatus::Connected);
        assert!(snap.received_at.is_some(), "poll success stamps the time");
        let heart = snap.heart.expect("Connected always carries a heart");
        assert!(heart.noise_switch);
        assert_eq!(heart.noise_level, 1);
        assert_eq!(heart.tx2_battery, 93);
    }

    #[test]
    fn demo_set_noise_flips_only_the_switch_and_clears_pending() {
        let mut core = demo_core();
        core.tick();
        core.pump([Command::SetNoise(false)]);
        // The SET already ran and the fresh poll came back inside pump:
        let snap = core.snapshot();
        assert_eq!(snap.status, ConnStatus::Connected);
        assert_eq!(snap.pending_set, None, "executed SET is no longer pending");
        let heart = snap.heart.unwrap();
        assert!(!heart.noise_switch);
        assert_eq!(heart.noise_level, 1, "level byte preserved");
    }

    #[test]
    fn demo_set_noise_level_flips_only_the_level() {
        let mut core = demo_core();
        core.tick();
        core.pump([Command::SetNoiseLevel(2)]);
        let heart = core.snapshot().heart.unwrap();
        assert!(heart.noise_switch, "switch byte preserved");
        assert_eq!(heart.noise_level, 2);
    }

    /// The `--demo` words are a fixed contract so every frontend and the
    /// README agree; an unknown word must NOT silently become a session the
    /// user did not pick (usage error instead).
    #[test]
    fn scenario_words_are_the_flag_contract() {
        assert_eq!(DemoScenario::from_word(None), Some(DemoScenario::Connected));
        assert_eq!(
            DemoScenario::from_word(Some("connected")),
            Some(DemoScenario::Connected)
        );
        assert_eq!(
            DemoScenario::from_word(Some("pending")),
            Some(DemoScenario::Pending)
        );
        assert_eq!(
            DemoScenario::from_word(Some("busy")),
            Some(DemoScenario::Busy)
        );
        assert_eq!(DemoScenario::from_word(Some("remote")), None);
    }

    /// The pending scenario's device with a zero stall behaves exactly like
    /// the connected one end-to-end (full SET flow through the real Core) —
    /// the ONLY variable the scenario adds is the stall duration, and the
    /// pending-becomes-visible-before-execution ordering is already proven
    /// by tests/actor.rs (SET round trip), so it is NOT retested here.
    #[test]
    fn pending_scenario_zero_stall_matches_connected_semantics() {
        let factory: crate::OpenFn = Box::new(|| {
            DemoDevice::stalled(Duration::ZERO)
                .map(|dev| Box::new(dev) as Box<dyn StatusSource + Send>)
        });
        let mut core = Core::new(factory, Arc::new(Hub::default()));
        core.tick();
        core.pump([Command::SetNoiseLevel(2)]);
        let heart = core.snapshot().heart.unwrap();
        assert!(heart.noise_switch, "switch byte preserved");
        assert_eq!(heart.noise_level, 2);
    }

    /// The busy scenario replays one real OS claim failure and nothing
    /// else: classify_claim must put it in ConnStatus::Busy (the real
    /// busy-banner state), the detail carries the OS words, and no heart
    /// may exist — the UI renders its banner from that, having never
    /// talked to any device.
    #[test]
    fn busy_scenario_carries_the_real_classification() {
        let factory: crate::OpenFn = Box::new(|| Err(busy_error()));
        let mut core = Core::new(factory, Arc::new(Hub::default()));
        core.tick();
        let snap = core.snapshot();
        assert_eq!(snap.status, ConnStatus::Busy);
        assert!(snap.heart.is_none(), "busy means never claimed");
        let detail = snap.detail.expect("busy carries the OS detail");
        assert!(detail.contains("busy (errno 16)"), "{detail}");
        assert!(
            snap.pending_set.is_none(),
            "a failed open has nothing in flight"
        );
    }
}
