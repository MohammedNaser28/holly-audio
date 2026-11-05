//! holly-app — UI-agnostic device core.
//!
//! ONE actor thread owns the claimed dongle (`HollyControl`); everything
//! else observees through [`DeviceHandle::subscribe`]. No UI, async-runtime,
//! or windowing dependency: std threads and channels only, so any frontend
//! (gpui, Tauri, CLI, ...) adapts it by driving a
//! `frontends/.../main.rs` against [`start`] / [`DeviceHandle`].
//!
//! Safety posture is unchanged from holly-usb: only GET_HEART_INFO and
//! SET_NOISE are ever issued; no OTA/reset/pairing; Rejected/BadAck are
//! surfaced, never swallowed; a malformed heartbeat is an abort, not a
//! retry (docs/protocol.md §2/§5).
//!
//! Layering (project rule — logic never duplicates across layers):
//! - framing: `holly-proto` (untouched here)
//! - transport/protocol gates: `holly-usb` (called, never bent)
//! - this crate: connection state machine, snapshot emission, view-model

use std::io::ErrorKind;
use std::time::Duration;

use serde::Serialize;

use holly_usb::{Error, HeartInfo, HollyControl, Transport};

pub mod actor;
pub mod vm;

pub use actor::{Command, Core, DeviceHandle, Hub, start, start_with_factory};

/// Default poll interval for the connected state (1 s, per spec).
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Default reconnect backoff when the claim is not (yet) available (1 s).
pub const RECONNECT_BACKOFF: Duration = Duration::from_secs(1);

/// Connection state machine as seen from the outside.
///
/// `NotFound -> Connecting -> Connected`; the three extra states are
/// terminal-at-a-moment conditions the UI renders as banners. Any of the
/// non-connected states decays back into the connect loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnStatus {
    /// No dongle 3547:0407 enumerated (or it vanished mid-session).
    NotFound,
    /// A connect attempt is in progress.
    Connecting,
    /// Claim succeeded; heart is the latest poll.
    Connected,
    /// Claim failed because another process holds IF0. Evidence that this
    /// is distinguishable: live run 2026-11-05 produced
    /// `Error::Usb(Custom { kind: ResourceBusy, error: "interface is busy (errno 16)" })`
    /// when two claims raced (holly-usb commit 0adcd2e), via nusb's
    /// ErrorKind::Busy mapped by transport.rs::usb_err (nusb error.rs).
    Busy,
    /// Claim denied — missing/too-restrictive udev grant. Observed live as
    /// `PermissionDenied (errno 13)` mapped to std kind PermissionDenied.
    PermissionDenied,
    /// Protocol-level failure that does NOT imply the device left
    /// (malformed reply etc. — abort, no retry; docs/protocol.md).
    Error,
}

/// What the actor sends subscribers. Emitted on change only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub status: ConnStatus,
    /// Short human text explaining the current status (error lines);
    /// `None` for the plain states.
    pub detail: Option<String>,
    /// Latest valid heartbeat while connected; also the fresh re-read
    /// after a failed SET ("show the error AND the real device state").
    pub heart: Option<HeartInfo>,
    /// A SET the actor accepted and is executing / will execute (latest
    /// request wins). This is pending-state information the frontend only
    /// ever RENDERS — never invents.
    pub pending_set: Option<bool>,
}

impl Snapshot {
    // (constructors deleted: the actor's Core composes snapshots from its
    // canonical state field set — see snapshot() in actor.rs)
}

/// Classify an OPEN-stage failure (enumeration + IF0 claim) into a
/// [`ConnStatus`]. Only std-level error kinds are consulted — both mapped
/// kinds were verified live on this hardware (comments on the variants)
/// and both preserved by `holly-usb`'s `usb_err` (nusb source-cited).
pub fn classify_claim(e: &Error) -> ConnStatus {
    match e {
        Error::NotFound => ConnStatus::NotFound,
        Error::Usb(io) if io.kind() == ErrorKind::ResourceBusy => ConnStatus::Busy,
        Error::Usb(io) if io.kind() == ErrorKind::PermissionDenied => ConnStatus::PermissionDenied,
        Error::Usb(_)
        | Error::Decode(_)
        | Error::Encode(_)
        | Error::CmdMismatch { .. }
        | Error::UnknownDirection { .. }
        | Error::Rejected { .. }
        | Error::BadAck { .. }
        | Error::HeartInfoShort { .. } => ConnStatus::Error,
    }
}

/// Session-stage error classification for the actor: `Usb` transport errors
/// mean the device is gone (unplug) — drop the claim so Drop restores the
/// interface-/driver state (holly-usb Drop semantics, nusb v0.2.9
/// LinuxInterface::drop); anything else is protocol-level and the device is
/// deemed present but misbehaving (abort; no retry).
pub fn device_gone(e: &Error) -> bool {
    matches!(e, Error::Usb(_))
}

/// The trusted window into one claimed dongle — the ONLY surface the actor
/// touches. Kept object-safe + `Send` so the actor thread can hold a boxed
/// one while tests script it over [`holly_usb::MockTransport`].
pub trait StatusSource {
    /// One GET_HEART_INFO round trip.
    fn poll(&mut self) -> Result<HeartInfo, Error>;
    /// One SET_NOISE round trip (fresh GET inside, per holly-usb's set_noise).
    fn set_noise(&mut self, on: bool) -> Result<(), Error>;
}

impl<T: Transport> StatusSource for HollyControl<T> {
    fn poll(&mut self) -> Result<HeartInfo, Error> {
        self.get_heart_info()
    }

    fn set_noise(&mut self, on: bool) -> Result<(), Error> {
        HollyControl::set_noise(self, on)
    }
}

/// Factory that TRIES to open+claim one dongle (production: enumeration
/// via `HollyControl::open`). Scripted in tests to exercise every
/// claim-stage outcome.
pub type OpenFn = Box<dyn FnMut() -> Result<Box<dyn StatusSource + Send>, Error> + Send>;
