//! Hollyland 6108 USB control transport — device I/O layer.
//!
//! This crate owns the *transport* only: claiming the HID control interface
//! (IF0) and moving 64-byte feature reports over `SET_REPORT`/`GET_REPORT`
//! control transfers. All framing/semantics live in [`holly-proto`]; nothing
//! protocol-level is invented here.
//!
//! Evidence chain (same rule as the rest of the repo — every constant cites):
//!   inputs/lsusb-v.txt            device topology, VID:PID, IF0 endpoints
//!   docs/device.md                interface map (IF0 = control, IF1/2 = UAC
//!                                 audio — never claimed, IF3 = vendor)
//!   docs/protocol.md §1           transport parameter provenance
//!   captures/2026-11-05-hearinfo-first-wire.txt   GET path [wire]
//!   captures/2026-11-05-set-noise.txt             SET path [wire]
//!   re/apk/...                    decompiled HollyAudio 3.1.0 (never committed)
//!
//! Hard rules carried over from the project:
//! - Only IF0 is claimed. The UAC audio interfaces (IF1/IF2) and the vendor
//!   bulk interface (IF3) are never touched; dropping this handle releases
//!   IF0 and reattaches the kernel `usbhid` driver it replaced.
//! - No firmware / OTA / pairing / reset commands exist in the API surface.
//!   OTA/reboot/pairing commands in `holly_proto::cmd` are *data* exported by
//!   the framing crate; this crate must never route them.
//!
//! Confidence flags ([code]/[wire]) follow the same meaning as docs/protocol.md.

mod device;
mod error;
mod heart;
mod transport;

pub use device::HollyControl;
pub use error::Error;
pub use heart::HeartInfo;
pub use transport::{MockTransport, NusbTransport, Transport};

pub use holly_proto::cmd;
