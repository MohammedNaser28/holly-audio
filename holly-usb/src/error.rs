//! Errors produced by the Hollyland USB transport and certified operations.

/// Transport-level and protocol-gate errors.
///
/// The transfer timeouts (SET 3000 ms / GET 1000 ms, per docs/protocol.md §1)
/// live in [`crate::NusbTransport`] — a timeout surfaces here as an ordinary
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
    /// `raw` is the full 64-byte reply frame as received, for diagnostics.
    #[error("SET ack payload has unexpected length {len} (expected 1 status byte)")]
    BadAck { len: usize, raw: [u8; 64] },

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
    /// open question 5) — this variant exists to not swallow unobserved
    /// behavior, not to claim its shape is known. `raw` is the full 64-byte
    /// reply frame as received, for diagnostics.
    #[error("device rejected the SET (status {status:#04x})")]
    Rejected { status: u8, raw: [u8; 64] },

    /// Device sent a heart-info payload shorter than the app's own gating
    /// would need for the single-TX field set (17 bytes observed on the wire).
    #[error("heart-info payload too short: {len} bytes (need >= 17)")]
    HeartInfoShort { len: usize },
}
