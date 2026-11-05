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
//! This is intentionally dumb: no error injection, no disconnects. Broken
//! transport / busy / permission / malformed branches are the job of the
//! actor tests (tests/actor.rs), not of a UI demo.

use std::sync::Mutex;

use crate::{DeviceHandle, StatusSource};
use holly_usb::{Error, HeartInfo};

/// Certified session-3 heart payload (17 bytes): nc ON, level 1 (medium),
/// tx1 off / tx2 connected, battery 0x5D = 93, gain 3, eq 3, reverb
/// off (level 2). Same vector the actor/vm tests parse.
const DEMO_PAYLOAD: [u8; 17] = [0, 1, 0, 93, 0, 0, 1, 1, 3, 0, 0, 0, 3, 0, 2, 0, 0];

#[derive(Debug)]
pub struct DemoDevice {
    heart: Mutex<HeartInfo>,
}

impl DemoDevice {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            heart: Mutex::new(HeartInfo::from_payload(&DEMO_PAYLOAD)?),
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
        self.heart.lock().unwrap().noise_switch = on;
        Ok(())
    }

    fn set_noise_level(&mut self, level: u8) -> Result<(), Error> {
        // Real device semantics: the level write leaves the switch byte
        // untouched (guarded round-trip test, capture session 3a).
        self.heart.lock().unwrap().noise_level = level;
        Ok(())
    }
}

/// Actor over [`DemoDevice`] with the standard 1 s loop — same
/// [`DeviceHandle`] API as real hardware, zero USB.
pub fn start_demo() -> DeviceHandle {
    let factory =
        Box::new(|| DemoDevice::new().map(|dev| Box::new(dev) as Box<dyn StatusSource + Send>));
    crate::start_with_factory(factory)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::DemoDevice;
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
}
