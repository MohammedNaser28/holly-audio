//! Device handle: owns one Transport, offers the two certified operations.
//!
//! Nothing here invents protocol bytes — TX frames are built by
//! [`holly_proto::Frame`] (APK-exact encoder), replies are gated the same way
//! the capture tooling gates them, and the two operations are
//!   - `get_heart_info()`  (cmd 31, GET path [wire] since the first capture)
//!   - `set_noise(bool)`   (cmd 19, SET path [wire] since Task 1's capture)

use nusb::MaybeFuture;

use holly_proto::{Frame, REPLY_HEADER, cmd};

use crate::Error;
use crate::heart::HeartInfo;
use crate::transport::{NusbTransport, Transport};

/// Dongle identity — `inputs/lsusb-v.txt` [descriptor], restated by
/// docs/device.md: Shenzhen Hollyland Technology, "Wireless Microphone".
pub const VENDOR_ID: u16 = 0x3547;
/// See [`VENDOR_ID`].
pub const PRODUCT_ID: u16 = 0x0407;

/// The control interface. IF0 only — HID class 3 with report endpoints
/// (`docs/device.md` §IF0). IF1/IF2 are UAC audio and are NEVER claimed;
/// IF3 is vendor bulk and also out of scope.
pub const CONTROL_IFACE: u8 = 0;

/// One claimed dongle. Dropping it releases the interface; when nusb's claim
/// detached the kernel driver (usbhid on Linux), the Drop path reattaches it
/// (verified against nusb v0.2.9 sources, linux_usbfs `LinuxInterface::drop`:
/// `release_interface` + `attach_kernel_driver`), so external resources are
/// restored without rework in this crate.
///
/// No observable device state is cached: every SET frame is built from a
/// fresh GET issued *inside* the SET call, so a state change made by the
/// device itself (TX button) or another client can never be written back
/// over. (Task 4a: the earlier `noise_level` cache is removed — it was only
/// consumed by `set_noise` and was stale-state risk.) [wire
/// captures/2026-11-05-set-noise.txt: heart replies carried the level; the
/// SET frame echoes the fetch from *that* capture's immediately preceding
/// GET]
#[derive(Debug)]
pub struct HollyControl<T: Transport> {
    transport: T,
}

impl<T: Transport> HollyControl<T> {
    /// Wrap a transport (claimed IF0) into a control handle.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Read-only status poll: `cmd 31 GET_HEART_INFO` [wire
    /// captures/2026-11-05-hearinfo-first-wire.txt]. TX carries an empty
    /// payload; reply payload is parsed with the APK's incremental gates.
    pub fn get_heart_info(&mut self) -> Result<HeartInfo, Error> {
        let request = Frame::new(cmd::GET_HEART_INFO, Vec::new());
        let (reply, _raw) = self.exchange(request)?;
        HeartInfo::from_payload(&reply.payload)
    }

    /// Toggle noise cancellation. Payload `[isOpen, level]` [code
    /// UsbMessengerV2.java:172-173]. The level always comes from a **fresh**
    /// `get_heart_info()` issued here — nothing is cached across calls, so a
    /// state change by the device (TX button) or another client between two
    /// of our own calls can never be overwritten by a stale SET frame.
    /// (The SET frame carries only `[isOpen, level]` — no other SET-command
    /// fields exist in this frame, per the [wire] capture shape
    /// `13 00 02 {isOpen} {level} EF` in captures/2026-11-05-set-noise.txt.)
    /// Device ack is a 1-byte status, `0x00` = success
    /// (`ProSetNoise.receiveData`) [wire captures/2026-11-05-set-noise.txt
    /// E2/E4]; nonzero status is surfaced as [`Error::Rejected`] — its
    /// real-world shape has never been captured (docs/protocol.md
    /// open question 5), so nothing is assumed beyond "not zero means not
    /// success".
    pub fn set_noise(&mut self, enabled: bool) -> Result<(), Error> {
        // The SET frame's fields derive from this fresh reply only.
        let level = self.get_heart_info()?.noise_level;
        let request = Frame::new(cmd::SET_NOISE, vec![u8::from(enabled), level]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(()),
            [status] => Err(Error::Rejected {
                status: *status,
                raw,
            }),
            other => Err(Error::BadAck {
                len: other.len(),
                raw,
            }),
        }
    }
}

impl HollyControl<NusbTransport> {
    /// Enumerate certified VID/PID, open the device, claim IF0 (first
    /// matching dongle). Enumeration and claim are the only USB bookkeeping
    /// this crate does; the UAC interfaces are untouched.
    ///
    /// Requires write access to the `/dev/bus/usb` node — install
    /// `packaging/70-hollyland.rules` (MODE 0660 + uaccess) so this works
    /// without root.
    pub fn open() -> Result<Self, Error> {
        for info in nusb::list_devices()
            .wait()
            .map_err(crate::transport::usb_err)?
        {
            if info.vendor_id() != VENDOR_ID || info.product_id() != PRODUCT_ID {
                continue;
            }
            let device = info.open().wait().map_err(crate::transport::usb_err)?;
            let iface = device
                .claim_interface(CONTROL_IFACE)
                .wait()
                .map_err(crate::transport::usb_err)?;
            return Ok(Self::new(NusbTransport::new(iface, CONTROL_IFACE)));
        }
        Err(Error::NotFound)
    }
}

impl<T: Transport> HollyControl<T> {
    /// One certified TX→RX round trip: SET_REPORT the encoded request, then
    /// GET_REPORT the reply, gated on:
    ///
    /// 1. reply magic = `05 03 BB DD` (device→host; the app never echoes
    ///    the host `AA` direction — both captures) [wire],
    /// 2. frame decode validity (holly-proto, APK-exact),
    /// 3. command echo equality (the capture tool's abort gate).
    ///
    /// Returns the decoded frame AND the untouched raw 64-byte reply (the
    /// raw copy lets callers attach full-frame diagnostics — e.g. hex
    /// printing at --verbosity — to gate failures like [`Error::Rejected`]).
    ///
    /// No retries anywhere: a transport error or gate failure is returned as
    /// `Err` immediately. The three gates replicate the abort-and-stop
    /// behavior of `tools/capture_set_noise.py` (`parse_reply`).
    fn exchange(&mut self, request: Frame) -> Result<(Frame, [u8; 64]), Error> {
        let buf = request.encode_buf()?;
        self.transport.report_out(&buf)?;

        let raw = self.transport.report_in()?;
        if raw[..4] != REPLY_HEADER {
            // Not `05 03 BB DD`. Byte 2 decides the abort reason: `AA` means
            // the device echoed our TX direction (never observed on the
            // wire); anything else is an unknown magic — both are aborts
            // (capture tool's parse gate, no retry).
            return Err(Error::UnknownDirection { found: raw[2] });
        }
        let reply = Frame::decode(&raw)?;
        // Decode is direction-agnostic (it validates AA frames' 0xEF tail and
        // BB replies taillessly); the gate above fixed the direction, and
        // this call re-asserts length/shape only.
        if reply.command != request.command {
            return Err(Error::CmdMismatch {
                got: reply.command,
                want: request.command,
            });
        }
        Ok((reply, raw))
    }

    /// Borrow the transport (e.g. for tests or byte-level inspection).
    pub fn transport(&self) -> &T {
        &self.transport
    }
}
