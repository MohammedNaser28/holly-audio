//! Task-4c crash-path probe: open `HollyControl` (nusb claim of IF0) and
//! sleep until killed. `kill -9` never runs Drop handlers, so whatever USB
//! state remains afterwards shows exactly what the *kernel* does on a
//! crashed process — `tools/crash_test.sh` captures that.
//!
//! A short status poll happens before the sleep purely to prove the control
//! path is live; the probe itself is read-only.

use holly_usb::HollyControl;

fn main() {
    let mut ctl = match HollyControl::open() {
        Ok(ctl) => ctl,
        Err(e) => {
            eprintln!("open/claim failed: {e:?}");
            std::process::exit(1);
        }
    };
    match ctl.get_heart_info() {
        Ok(info) => eprintln!(
            "claim up: NC={} level={} battery(tx2)={}",
            info.noise_switch, info.noise_level, info.tx2_battery,
        ),
        Err(e) => eprintln!("claim up; initial GET failed ({e:?})"),
    }
    eprintln!("READY pid={} — sleeping until killed", std::process::id());
    loop {
        std::thread::park();
    }
}
