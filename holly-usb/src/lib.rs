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
mod heart;
mod transport;

pub use device::HollyControl;
pub use heart::HeartInfo;
pub use transport::{MockTransport, NusbTransport, Transport};

pub use holly_proto::cmd;

/// Transport-level errors.
///
/// The transfer timeouts (SET 3000 ms / GET 1000 ms, per docs/protocol.md §1)
/// live in [`NusbTransport`] — a timeout surfaces here as an ordinary
/// [`std::io::Error`], same as any other OS-level failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// USB/OS-level failure of an open/claim/transfer. The wrapped
    /// [`std::io::Error`] carries the nusb `TransferError` or enumeration
    /// error (`std::io::Error::other` wrap).
    #[error("usb error: {0}")]
    Usb(#[from] std::io::Error),

    /// No dongle with the certified VID/PID is present on the bus.
    #[error("no Hollyland dongle 3547:0407 present")]
    NotFound,

    /// Reply bytes failed frame-level decode (bad magic / truncated / etc.).
    #[error("malformed device reply: {0}")]
    Decode(#[from] holly_proto::DecodeError),

    /// Request frame did not fit the certified single-report envelope.
    #[error("frame encode failed: {0}")]
    Encode(#[from] holly_proto::EncodeError),

    /// SET ack arrived with a payload length other than the observed 1-byte
    /// status shape. The 1-byte `00`-success shape is [wire]
    /// (captures/2026-11-05-set-noise.txt E2/E4); any other length is not
    /// assumed valid — error shapes were never captured (open question 5).
    #[error("SET ack payload has unexpected length {len} (expected 1 status byte)")]
    BadAck { len: usize },

    /// The device echoed a different command in its reply than was sent —
    /// abort gate from the capture tooling (no retry loop is performed).
    #[error("reply command echo mismatch: got {got:#04x}, want {want:#04x}")]
    CmdMismatch { got: u8, want: u8 },

    /// Reply magic did not start with `05 03 BB DD` (device→host preamble;
    /// capture gate — no retry). Byte 2 selects abort wording: `AA` means the
    /// device echoed our TX direction (never observed on the wire), anything
    /// else is an unknown magic.
    #[error("unknown reply direction (magic {found:#04x})")]
    UnknownDirection { found: u8 },

    /// SET ack reported failure: payload's status byte was nonzero
    /// (`ProSetNoise.receiveData` treats `data[0] == 0` as success) [code].
    /// NOTE: no error-shaped ack has ever been captured (docs/protocol.md
    /// open question 5) — this variant exists to *not* swallow unobserved
    /// behavior, not to claim its shape is known.
    #[error("device rejected the SET (status {status:#04x})")]
    Rejected { status: u8 },

    /// Device sent a heart-info payload shorter than the app's own gating
    /// would need for the single-TX field set (17 bytes observed on the wire).
    #[error("heart-info payload too short: {len} bytes (need >= 17)")]
    HeartInfoShort { len: usize },
}
