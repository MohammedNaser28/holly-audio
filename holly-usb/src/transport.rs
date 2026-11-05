//! Transport trait + certified nusb implementation + CI mock.
//!
//! The trait is deliberately transport-shaped, not protocol-shaped: it moves
//! one 64-byte feature report per call. That keeps room for a future
//! interrupt-EP reader (docs/device.md: EP 0x81 IN) as an additional trait
//! method or sibling type without reworking the control path.

use std::time::Duration;

use nusb::MaybeFuture;

use crate::Error;

/// Size of one feature-report control transfer. Holds the same value as
/// [`holly_proto::CONTROL_BUF_SIZE`] (64) — restated as a distinct transport
/// constant so this crate does not reach through for a framing name.
/// Report size from `inputs/lsusb-v.txt` (report 5 control buffer, 0x40) and
/// both captures: every TX leg writes 64 bytes, every RX leg reads 64 [wire].
pub const REPORT_SIZE: usize = 64;

/// HID control-transfer stage timeouts, from the app's own controlTransfer
/// calls as documented in docs/protocol.md §1 [code], wire-compatible since
/// both certified captures used them [wire].
pub const SET_TIMEOUT: Duration = Duration::from_millis(3_000);
pub const GET_TIMEOUT: Duration = Duration::from_millis(1_000);

/// Host→device HID class request. `SET_REPORT` = 9 (HID class requests,
/// `Device Control Requests` of the USB HID spec) [code], routed as
/// documented in docs/protocol.md §1.
pub const SET_REPORT: u8 = 9;
/// Device→host HID class request. `GET_REPORT` = 1 [code].
pub const GET_REPORT: u8 = 1;

/// One blocking, transport-level exchange surface.
pub trait Transport {
    /// Send one host→device feature report (`SET_REPORT(Feature)`), 64 bytes.
    fn report_out(&mut self, report: &[u8; REPORT_SIZE]) -> Result<(), Error>;
    /// Request one device→host feature report (`GET_REPORT(Feature)`), 64 bytes.
    fn report_in(&mut self) -> Result<[u8; REPORT_SIZE], Error>;
}

/// Real transport: an already-claimed `nusb::Interface` (IF0 of the dongle).
///
/// The nusb API (v0.2.9) is used exactly as documented on
/// docs.rs/nusb/0.2.9 (`Interface::control_in` / `::control_out`, with
/// `ControlType::Class` + `Recipient::Interface` for HID class requests —
/// docs note that for `Recipient::Interface`, `index` is the interface
/// number). Transfer semantics verified against the two [wire] captures:
///   - TX leg: `SET_REPORT(0x21,9,0x0305,64,3000)`
///   - RX leg: `GET_REPORT(0xA1,1,0x0305,64,1000)`
///
/// Sources: docs/protocol.md §1; captures/2026-11-05-hearinfo-first-wire.txt
/// header.
pub struct NusbTransport {
    iface: nusb::Interface,
    control_iface: u8,
}

impl NusbTransport {
    /// Wrap an already-claimed control interface. `control_iface` is the
    /// interface number the claim was made on (IF0 = 0, docs/device.md).
    pub fn new(iface: nusb::Interface, control_iface: u8) -> Self {
        Self {
            iface,
            control_iface,
        }
    }
}

impl Transport for NusbTransport {
    fn report_out(&mut self, report: &[u8; REPORT_SIZE]) -> Result<(), Error> {
        self.iface
            .control_out(
                nusb::transfer::ControlOut {
                    control_type: nusb::transfer::ControlType::Class,
                    recipient: nusb::transfer::Recipient::Interface,
                    request: SET_REPORT,
                    // Feature report type (3) << 8 | report id (5)
                    value: holly_proto::REPORT_ID_WVALUE_NORMAL,
                    index: u16::from(self.control_iface),
                    data: report,
                },
                SET_TIMEOUT,
            )
            .wait()
            .map_err(transfer_err)?;
        Ok(())
    }

    fn report_in(&mut self) -> Result<[u8; REPORT_SIZE], Error> {
        let got = self
            .iface
            .control_in(
                nusb::transfer::ControlIn {
                    control_type: nusb::transfer::ControlType::Class,
                    recipient: nusb::transfer::Recipient::Interface,
                    request: GET_REPORT,
                    value: holly_proto::REPORT_ID_WVALUE_NORMAL,
                    index: u16::from(self.control_iface),
                    length: REPORT_SIZE as u16,
                },
                GET_TIMEOUT,
            )
            .wait()
            .map_err(transfer_err)?;

        // The device has always answered with the full 64-byte report in both
        // captures [wire]; a short answer is still buffered into the 64-byte
        // report shape so callers see ONE uniform type.
        let mut report = [0u8; REPORT_SIZE];
        let n = got.len().min(REPORT_SIZE);
        report[..n].copy_from_slice(&got[..n]);
        Ok(report)
    }
}

/// Map a nusb operation (non-transfer) error into an [`std::io::Error`],
/// PRESERVING its [`nusb::ErrorKind`] as far as std carries a matching kind —
/// callers (e.g. holly-cli's exit-code mapping) need to tell a
/// PermissionDenied (missing udev rule) apart from a Busy (device held).
pub(crate) fn usb_err(err: nusb::Error) -> std::io::Error {
    // ErrorKind is #[non_exhaustive], hence the catch-all arm.
    let kind = match err.kind() {
        nusb::ErrorKind::Disconnected => std::io::ErrorKind::NotConnected,
        nusb::ErrorKind::Busy => std::io::ErrorKind::ResourceBusy,
        nusb::ErrorKind::PermissionDenied => std::io::ErrorKind::PermissionDenied,
        nusb::ErrorKind::NotFound => std::io::ErrorKind::NotFound,
        nusb::ErrorKind::Unsupported => std::io::ErrorKind::Unsupported,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(kind, format!("{err}"))
}

/// Map a transfer-level error (control endpoints return these) into an
/// [`std::io::Error`]. TransferError carries no errno, so kinds are preserved
/// only where they are trustworthy (`Disconnected`, `InvalidArgument`);
/// everything else lands on `Other` with the variant name in the message
/// (e.g. `Cancelled` covers both cancellation and timeout, per nusb docs).
pub(crate) fn transfer_err(err: nusb::transfer::TransferError) -> std::io::Error {
    // TransferError is #[non_exhaustive] too.
    let kind = match err {
        nusb::transfer::TransferError::Disconnected => std::io::ErrorKind::NotConnected,
        nusb::transfer::TransferError::InvalidArgument => std::io::ErrorKind::InvalidInput,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(kind, format!("{err:?}"))
}

#[derive(Debug, Default)]
/// A scriptable, traffic-recording transport for CI tests. Replies are popped
/// from a queue in FIFO order; every written report is kept for byte-exact
/// assertions against capture vectors (`captures/2026-11-05-*.txt`).
pub struct MockTransport {
    written: Vec<[u8; REPORT_SIZE]>,
    replies: std::collections::VecDeque<Result<[u8; REPORT_SIZE], Error>>,
}

impl MockTransport {
    /// Create an empty mock: no scripted replies yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue one device reply for the next `report_in` call.
    pub fn push_reply(&mut self, reply: [u8; REPORT_SIZE]) {
        self.replies.push_back(Ok(reply));
    }

    /// Queue one advance-scripted failure for the next `report_in` call
    /// (simulates e.g. device unplugged mid-session).
    pub fn push_error(&mut self, error: Error) {
        self.replies.push_back(Err(error));
    }

    /// All reports written by callers so far, in order.
    pub fn written(&self) -> &[[u8; REPORT_SIZE]] {
        &self.written
    }
}

impl Transport for MockTransport {
    fn report_out(&mut self, report: &[u8; REPORT_SIZE]) -> Result<(), Error> {
        self.written.push(*report);
        Ok(())
    }

    fn report_in(&mut self) -> Result<[u8; REPORT_SIZE], Error> {
        // An exhausted queue is a *test design* error, not a runtime state:
        // fail loudly so scripted sessions stay explicit.
        self.replies
            .pop_front()
            .unwrap_or_else(|| Err(std::io::Error::other("mock: no scripted reply").into()))
    }
}
