//! Device handle: owns one Transport, offers the two certified operations.
//!
//! Nothing here invents protocol bytes — TX frames are built by
//! [`holly_proto::Frame`] (APK-exact encoder), replies are gated the same way
//! the capture tooling gates them, and the operations are
//!   - `get_heart_info()`  (cmd 31, GET path [wire] since the first capture)
//!   - `set_noise(bool)`   (cmd 19, SET path [wire] since Task 1's capture)
//!   - `set_noise_level(u8)` (cmd 19 level-write; settled round trip 2026-11-05)
//!   - `set_eq(u8)` — cmd 7, returns the pad from ITS fresh GET (the value in
//!     force before the write); frames verified [wire] by the guarded
//!     PC-side round trip 2026-11-09 (captures/set_eq_roundtrip.jsonl).
//!   - `set_gain(u8)` — cmd 3, same set_* shape as set_eq (fresh GET inside,
//!     returns the pre-write stop); [code] until the guarded round trip runs.
//!   - `set_reverb(bool, u8)` — cmd 5, payload `[isOpen, gear]` (switch
//!     FIRST, per the APK builder); same set_* shape (fresh GET inside,
//!     returns the pre-write pair); [code] until the guarded round trip
//!     runs.

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

    /// Write the NC **level** (the second byte of cmd 19's payload) while
    /// keeping the switch as the fresh GET just reported. Same frame shape as
    /// [`HollyControl::set_noise`], `[isOpen, level]` [code
    /// UsbMessengerV2.java:172-173]; the app's own level buttons always send
    /// the switch *on* (`setNoiseLevel(2, true)` strong, `(1, true)` medium,
    /// `(0, true)` low — for this product `HLDevice6302Activity.java:1593,
    /// :1610, :1626-1631`), so 0/1/2 is the only value set the APK uses
    /// [code]; larger values have never been observed on any wire and are
    /// the caller's responsibility, not assumed here. The switch state is
    /// preserved instead of forced on so a restore can always write back the
    /// exact pre-test pair (switch, level) without a hidden ON.
    /// Ack gating is identical to `set_noise` (`00` = success; anything else
    /// surfaces as `Rejected`/`BadAck` — no zero-shaped retry).
    pub fn set_noise_level(&mut self, level: u8) -> Result<(), Error> {
        // isOpen derives from THIS fresh reply only — same freshness rule as
        // set_noise ("device changed it while we were waiting" hazard).
        let noise_switch = self.get_heart_info()?.noise_switch;
        let request = Frame::new(cmd::SET_NOISE, vec![u8::from(noise_switch), level]);
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

    /// Write the receiver EQ pad — cmd 7, single-byte payload `[{value}]`
    /// [code docs/set-gate-checklist.md §3 EQ row:
    /// `setEq(EQ.OVERCAST/BRIGHT/BALANCE.getType())` = 1/2/3
    /// (`HLDevice6302Activity.java:729/:739/:749`); read side
    /// `eqLevel` = docs/protocol.md §5 offset 12, wire-mapped
    /// 1..=3 = Low/Bright/"Equalization" (§5.2)]. Frames verified [wire]:
    /// guarded PC-side round trip 2026-11-09
    /// (captures/raw/set_eq_roundtrip.pcap → captures/
    /// set_eq_roundtrip.jsonl — `05 03 AA DD 07 00 01 {02,03} EF` → ack
    /// `05 03 BB DD 07 00 01 00 61` → hearts settled on the written pad).
    ///
    /// Returns the eq pad THIS call's fresh `get_heart_info()` reported —
    /// the value in force on the device immediately **before** the write.
    /// The fresh GET is the crate's uniform SET discipline (one fresh GET
    /// inside every SET call; no observable device state is ever cached),
    /// it gates the SET on a live, answering device, and its pad now goes
    /// back to the caller instead of being dropped: a guarded round trip's
    /// restore leg targets THAT value, not a possibly older pre-run poll —
    /// if another client moved EQ between the caller's own GET and this
    /// SET, the returned pad is the freshest memory the device has. (The
    /// SET frame itself still carries only `[{value}]` — no preserved byte
    /// exists in this payload, §3 EQ row.)
    ///
    /// Ack gating is identical to [`HollyControl::set_noise`] (`[00]` =
    /// success; anything else surfaces as [`Error::Rejected`]/[`Error::BadAck`]
    /// — no retry). Values outside the certified space are the caller's
    /// responsibility, not assumed here — mirror of
    /// [`HollyControl::set_noise_level`]'s stance on unobserved bytes.
    pub fn set_eq(&mut self, value: u8) -> Result<u8, Error> {
        // Fresh GET inside every SET (crate rule). The EQ payload has no
        // preserved byte (§3 EQ row); the snapshot's pad is returned to the
        // caller as the pre-write state.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_EQ, vec![value]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(state.eq_level),
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

    /// Write the receiver gain — the "Volume adjustment" control — cmd 3
    /// SET_VOICE_LEVEL, single-byte payload `[{value}]` [code
    /// docs/set-gate-checklist.md §3 Gain row: payload `[{value}]`, identity
    /// write semantics from the APK's own seekbar caller (`setVoice` →
    /// `ProSetVoiceLevel`, `HLDevice6302Activity.java:840-846`,
    /// docs/protocol.md §5.1); cmd number re-derived from
    /// `ProSetVoiceLevel.getCmd()`]. The write space EQUALS the read space
    /// (no inversion): heart `voiceLevel` (§5 offset 8) is the same
    /// 0..=5 index the APK's 6-stop bar maps to display stops "1".."6"
    /// (stop N+1 — §5.2 Volume row; §5.1). Session 3b read voiceLevel 03
    /// [wire] while the phone showed stop "4" — the UI-stop ↔ wire-byte
    /// equation's read side; the WRITE side (this frame) stays [code]
    /// until the guarded round trip captures it on the wire (that run is
    /// exactly what settles the §3 row).
    ///
    /// Returns the voice stop THIS call's fresh `get_heart_info()` reported —
    /// the value in force on the device immediately **before** the write.
    /// The fresh GET is the crate's uniform SET discipline (one fresh GET
    /// inside every SET call; no observable device state is ever cached),
    /// it gates the SET on a live, answering device, and its stop goes back
    /// to the caller instead of being dropped: a guarded round trip's
    /// restore leg targets THAT value, not a possibly older pre-run poll —
    /// if another client moved gain between the caller's own GET and this
    /// SET, the returned stop is the freshest memory the device has. (The
    /// SET frame itself still carries only `[{value}]` — this payload has
    /// no preserved byte, §3 Gain row. `voiceMode` is a separate heart
    /// field (§5 offset 9) and is never written.)
    ///
    /// Ack gating is identical to [`HollyControl::set_eq`] (checklist §3:
    /// the `data[0] == 0` status rule is uniform across all `ProSet*`
    /// classes; `ProSetVoiceLevel.receiveData` [code]): `[00]` = success;
    /// anything else surfaces as [`Error::Rejected`]/[`Error::BadAck`] — no
    /// retry. Reply-tail bytes stay uninterpreted: heart replies carry
    /// XOR(payload[0..16)) ⊕ 0x68 [wire — the LAST payload byte (§5
    /// offset 16, signalMode) is NOT in the fold; 2026-11-11 correction
    /// from the reverb run's signalMode=1 hearts, tools/residue_check.py],
    /// ACK tails follow XOR(whole 7+len prefix) ⊕ 0x07 = cmd ⊕ 0x66
    /// [wire, 4 command shapes] (docs/protocol.md §2 — decode discards
    /// everything past `7+len`, so no decode behaviour changes here
    /// either way). Values
    /// outside the certified 0..=5 stop space are the caller's
    /// responsibility, not assumed here — mirror of
    /// [`HollyControl::set_eq`]'s stance on unobserved bytes.
    pub fn set_gain(&mut self, stop: u8) -> Result<u8, Error> {
        // Fresh GET inside every SET (crate rule). The gain payload has no
        // preserved byte (§3 Gain row); the snapshot's stop is returned to
        // the caller as the pre-write state.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_VOICE_LEVEL, vec![stop]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(state.voice_level),
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

    /// Write the controller reverb state — cmd 5 SET_REVERB, two-byte
    /// payload `[isOpen, gear]` with the SWITCH FIRST [code:
    /// `UsbMessengerV2.setReverb(int value, boolean isOpen)` builds
    /// `{isOpen ? 1 : 0, value}` — `UsbMessengerV2.java:192-195`; the
    /// order is the one byte the APK does NOT move around in this frame,
    /// unlike cmd 19's fresh-get preserve pattern]. Gear space = the
    /// enum `DeviceConnectVariable.Reverb` GEAR_ONE/GEAR_TWO/GEAR_THREE =
    /// 0/1/2 [code `HLDevice6302Activity.java:629-632`], identity-mapped to
    /// the heart's reverb byte (§5 offset 14 — session-4 read-map check:
    /// the gear flip 00→02 landed in the heart while the switch byte
    /// stayed on, docs/protocol.md §5.2). Callers mirror the APK's own
    /// two writes [code `HLDevice6302Activity.java:759/:769/:779/:792-793`]:
    /// the gear buttons send the switch ON with their gear, the toggle-off
    /// path sends the switch OFF with its remembered current gear
    /// (`this.reverbType` — heart values keep feeding it, :1280/:1883-1891),
    /// i.e. a toggle never invents a gear. The byte layout stays [code]
    /// until the guarded PC-side round trip captures it on the wire
    /// (route (b) — that run settles the §3 Reverb row).
    ///
    /// Unlike the SET_NOISE family there is NO preserved byte in this
    /// payload: both bytes are caller-supplied exactly as the APK supplies
    /// them, and the pair a prepare/restore path needs comes from the
    /// return instead — this op returns the PAIR its fresh
    /// `get_heart_info()` reported: the state in force on the device
    /// immediately **before** the write. The fresh GET is the crate's
    /// uniform SET discipline (one fresh GET inside every SET call; no
    /// observable device state is ever cached), it gates the SET on a
    /// live, answering device, and its `(reverbSwitch, reverbLevel)`[
    /// wire captures/2026-11-05-hearinfo-first-wire.txt: heart payload[13]
    /// = 00 (off), payload[14] = 02 — and hearts kept reporting gear 02
    /// while the switch was off across sessions 1-4, §5.2] go back to the
    /// caller instead of being dropped: a guarded round trip's restore leg
    /// targets THAT pair — if another client moved reverb between the
    /// caller's own GET and this SET, the returned pair is the freshest
    /// memory the device has.
    ///
    /// Ack gating is identical to [`HollyControl::set_gain`] (checklist §3:
    /// the `data[0] == 0` status rule is uniform across all `ProSet*`
    /// classes [code]): `[00]` = success; anything else surfaces as
    /// [`Error::Rejected`]/[`Error::BadAck`] — no retry. Reply-tail bytes
    /// stay uninterpreted (decode discards everything past `7+len`,
    /// docs/protocol.md §2). Values outside the certified 0..=2 gear space
    /// are the caller's responsibility, not assumed here — family
    /// convention (mirror of [`HollyControl::set_gain`]).
    pub fn set_reverb(&mut self, switch: bool, gear: u8) -> Result<(bool, u8), Error> {
        // Fresh GET inside every SET (crate rule); its pair is the restore
        // datum returned to the caller.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_REVERB, vec![u8::from(switch), gear]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok((state.reverb_switch, state.reverb_level)),
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
    /// `packaging/70-hollyland.rules` (MODE 0666, `ae709d6`) so this works
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
            // detach_and_claim (not plain claim): a FRESH enumeration lets the
            // kernel probe IF0 with usbhid (observed 2026-11-05 after replug,
            // `lsusb -t`: If0 Driver=usbhid → errno 16 BUSY on claim). nusb's
            // detach path uses USBDEVFS_DISCONNECT_CLAIM with
            // EXCEPT_DRIVER("usbfs") — it detaches the kernel driver *but*
            // still fails with the same errno-16 BUSY when another usbfs
            // userspace holder keeps IF0, which is exactly the parallel-
            // claim signal holly-app classifies (`classify_claim`).
            // Drop reattaches the kernel driver (see type-level doc).
            let iface = device
                .detach_and_claim_interface(CONTROL_IFACE)
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
