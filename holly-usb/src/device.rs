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
//!   - `set_channel_mode(u8)` — cmd 1, single-byte payload `[{value}]`
//!     (voiceMode, Mono=0/Stereo=1); same set_* shape (fresh GET inside,
//!     returns the pre-write mode); [code] until the guarded round trip
//!     runs. THE re-enumeration-risk control: the mode change re-presents
//!     the audio path, so callers must expect the handle to die briefly
//!     after the write.
//!   - `set_tx_mute(u8, bool)` — cmd 22, payload `[{txIndex}, {muted}]`
//!     (0-based TX slot TX1=0 … TX4=3, then the mute byte); same set_*
//!     shape (fresh GET inside, returns the pre-write mute of the
//!     ADDRESSED slot — `None` where the fresh heart's length does not
//!     carry that slot's field); [code] until the guarded round trip
//!     runs.
//!   - `set_shutdown_time(u8)` — cmd 39, single-byte payload `[{value}]`
//!     (shutdownTime, 0 = 15min / 1 = Never); same set_* shape (fresh GET
//!     inside, returns the pre-write value); [code] until the guarded
//!     round trip runs. AGENTS.md lift: timer values ONLY, never an
//!     immediate power-off (owner, 2026-11-12).
//!   - `set_light(u8)` — cmd 33, single-byte payload `[{value}]`
//!     (lightStatus, 0 = on / 1 = off — INVERTED relative to the UI toggle
//!     on BOTH APK sides); same set_* shape (fresh GET inside, returns the
//!     pre-write wire byte); [code] until the guarded round trip runs. The
//!     UI-bool ↔ wire-byte inversion lives in exactly one place upstream
//!     (holly-app's vm.rs), never here.

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

    /// Write the receiver channel mode — the "Channel mode" panel's
    /// Mono/Stereo selector — cmd 1 SET_VOICE_MODE, single-byte payload
    /// `[{value}]` [code docs/set-gate-checklist.md §3 Channel-mode row:
    /// `UsbMessengerV2.setVoiceMode(int value)` builds
    /// `saveReserved(new byte[]{(byte) value})` — `UsbMessengerV2.java:185-188`;
    /// cmd re-derived from `ProSetVoiceMode.getCmd()` = `(byte) 1`
    /// (`ProSetVoiceMode.java:15-17`)]. Identity write semantics, from the
    /// APK's own selector caller: the `mono_mode` press sends 0, the
    /// `stereo_mode` press sends 1 (`HLDevice6302Activity.java:1651-1652`,
    /// `:1661-1662`). The write space EQUALS the read space (no inversion):
    /// heart `voiceMode` (§5 offset 9, `ProGetHearInfo.java:69-71`) is the
    /// same value the read path renders — `monoMode` selected at 0,
    /// `stereoMode` at 1 (`HLDevice6302Activity.java:1803-1805`, whose field
    /// is literally named `channelMode`); §5.2 Channel-mode row. The APK's
    /// GET companion (cmd 2) has NO producer in `UsbMessengerV2` — readback
    /// rides the heart poll, like light/identify/speaker/shutdown (checklist
    /// §3 footnote). The frame stays [code] until the guarded PC-side round
    /// trip captures it on the wire (route (b) — that run settles the §3
    /// Channel-mode row).
    ///
    /// Returns the voiceMode THIS call's fresh `get_heart_info()` reported —
    /// the value in force on the device immediately **before** the write.
    /// The fresh GET is the crate's uniform SET discipline (one fresh GET
    /// inside every SET call; no observable device state is ever cached),
    /// it gates the SET on a live, answering device, and its mode goes back
    /// to the caller instead of being dropped: a guarded round trip's
    /// restore leg targets THAT value, not a possibly older pre-run poll —
    /// if another client moved the mode between the caller's own GET and
    /// this SET, the returned mode is the freshest memory the device has.
    /// (The SET frame itself still carries only `[{value}]` — this payload
    /// has no preserved byte, §3 Channel-mode row.)
    ///
    /// ⚠ THIS is the re-enumeration-risk control (feature prompt §Risks):
    /// changing the channel mode re-presents the audio path to the host —
    /// the phone app itself stops its recorder, waits 200 ms and restarts it
    /// after every mode change [code `HLDevice6302Activity.java:1837-1861`],
    /// and the dongle may briefly drop its interfaces or change its USB
    /// address. The control handle can therefore go dead right after a
    /// successful ack; callers (the guarded test) must reopen by VID:PID
    /// within a bounded window instead of assuming the op failed.
    ///
    /// Ack gating is identical to [`HollyControl::set_gain`] (checklist §3:
    /// the `data[0] == 0` status rule is uniform across all `ProSet*`
    /// classes — `ProSetVoiceMode.receiveData` reads `toInt(data[0]) == 0`
    /// [code `ProSetVoiceMode.java:25-28`]): `[00]` = success; anything else
    /// surfaces as [`Error::Rejected`]/[`Error::BadAck`] — no retry.
    /// Reply-tail bytes stay uninterpreted: heart replies carry
    /// XOR(payload[0..16)) ⊕ 0x68 [wire — LAST payload byte excluded],
    /// ACK tails follow XOR(whole 7+len prefix) ⊕ 0x07 = cmd ⊕ 0x66 [wire,
    /// 4 command shapes] (docs/protocol.md §2 — decode discards everything
    /// past `7+len`, so no decode behaviour changes here either way).
    /// Values outside the documented 0/1 space are the caller's
    /// responsibility, not assumed here — family convention (mirror of
    /// [`HollyControl::set_gain`]).
    pub fn set_channel_mode(&mut self, value: u8) -> Result<u8, Error> {
        // Fresh GET inside every SET (crate rule). The channel-mode payload
        // has no preserved byte (§3 Channel-mode row); the snapshot's mode
        // is returned to the caller as the pre-write state.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_VOICE_MODE, vec![value]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(state.voice_mode),
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

    /// Write ONE transmitter's mute state — the per-mic "Mute"/"Muted"
    /// chip — cmd 22 SET_TX_MUTE, two-byte payload `[{txIndex}, {muted}]`
    /// [code docs/set-gate-checklist.md §3 TX-mute row:
    /// `UsbMessengerV2.setMicMute(boolean on, int deviceType)` builds
    /// `saveReserved(new byte[]{(byte) (deviceType.ordinal() - 1), (byte)
    /// (on ? 1 : 0)})` clamped into 0..=3 (`UsbMessengerV2.java:206-210`,
    /// `Protocol.java:49-57`); cmd re-derived from `ProSetTxMute.getCmd()`
    /// = `(byte) 22` = Ascii.SYN (`ProSetTxMute.java:16-18`)]. One TX per
    /// frame — the APK's mute click writes exactly one ordinal, there is
    /// no broadcast variant (`HLDevice6302Activity.java:677/:691/:705/:719`).
    /// `tx_index` is the WIRE 0-based slot (TX1=0 … TX4=3); `muted` is the
    /// write byte 0/1, and the write space EQUALS the read space (no
    /// inversion): heart `tx1_mute`/`tx2_mute` (§5 offsets 4/5,
    /// `ProGetHearInfo.java:57-58`) are the same values the app renders —
    /// §5.2 TX-mute row. The frame stays [code] until the guarded PC-side
    /// round trip captures it on the wire (route (b) — that run settles
    /// the §3 TX-mute row).
    ///
    /// Returns the ADDRESSED slot's pre-write mute from THIS call's fresh
    /// `get_heart_info()` — the value in force on the device immediately
    /// before the write (the crate's uniform SET discipline — one fresh
    /// GET inside every SET call, no observable device state cached — it
    /// gates the SET on a live, answering device, and the value is the
    /// guarded test's restore datum, like set_gain's returned stop). The
    /// result is `Option` because the fresh heart may not CARRY the slot:
    /// `tx3_mute`/`tx4_mute` exist only at heart lengths ≥ 22/23 [code
    /// `ProGetHearInfo.java:100-123`], so a write targeting TX3/TX4 from a
    /// certified 17-byte heart has no restore datum — the return stays
    /// honest about what the GET actually reported instead of inventing a
    /// false.
    ///
    /// Ack gating is identical to [`HollyControl::set_gain`] (checklist §3:
    /// the `data[0] == 0` status rule is uniform across all `ProSet*`
    /// classes — `ProSetTxMute.receiveData` reads `toInt(data[0]) == 0`
    /// [code `ProSetTxMute.java:26-30`]): `[00]` = success; anything else
    /// surfaces as [`Error::Rejected`]/[`Error::BadAck`] — no retry.
    /// Reply-tail bytes stay uninterpreted: heart replies carry
    /// XOR(payload[0..16)) ⊕ 0x68 [wire — LAST payload byte excluded],
    /// ACK tails follow XOR(whole 7+len prefix) ⊕ 0x07 = cmd ⊕ 0x66 [wire,
    /// 5 command shapes] (docs/protocol.md §2 — decode discards everything
    /// past `7+len`, so no decode behaviour changes here either way).
    /// Values outside the documented tx_index space 0..=3 are the caller's
    /// responsibility, not assumed here — family convention (mirror of
    /// [`HollyControl::set_gain`]).
    pub fn set_tx_mute(&mut self, tx_index: u8, muted: bool) -> Result<Option<bool>, Error> {
        // Fresh GET inside every SET (crate rule); the addressed slot's
        // mute is the restore datum returned to the caller.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_TX_MUTE, vec![tx_index, u8::from(muted)]);
        let (reply, raw) = self.exchange(request)?;
        // The fresh GET's word for the slot we are about to write. Slots
        // the heart's length gate does not carry return None (see doc).
        let previous = match tx_index {
            0 => Some(state.tx1_mute),
            1 => Some(state.tx2_mute),
            2 => state.tx3_mute,
            3 => state.tx4_mute,
            _ => None,
        };
        match reply.payload.as_slice() {
            [0x00] => Ok(previous),
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

    /// Write the auto-power-off timer — the "Schedule power off" selector —
    /// cmd 39 SET_SHUTDOWN_TIME, single-byte payload `[{value}]` [code
    /// docs/set-gate-checklist.md §3 Shutdown-power-off row:
    /// `UsbMessengerV2.setShutdownTime(int value)` builds
    /// `saveReserved(new byte[]{(byte) value})`
    /// (`UsbMessengerV2.java:157-161`); cmd re-derived from
    /// `ProSetShutdownTime.getCmd()` = `PostProtocol.Command.GET_MUTE`
    /// (`ProSetShutdownTime.java:16-18`), the constant folding to 39
    /// (`PostProtocol.java:27` — checklist §5)]. The value space is the
    /// APK's own two timer values, an IDENTITY mapping on both sides —
    /// quarter("15min") → 0, never("Never") → 1 (`HLDevice6302Activity
    /// .java:1667-1671/:1672-1676`), read renderer `quarterTime
    /// .setSelected(value == 0)` / `neverTime.setSelected(value == 1)`
    /// (`:1864-1867`) — write space == read space, no inversion; values
    /// outside {0, 1} have no certified meaning and are the caller's
    /// responsibility, not assumed here (family convention). AGENTS.md
    /// lift: cmd 39 is for timer values ONLY, never an immediate
    /// power-off (owner, 2026-11-12) — the APK's immediate power-off /
    /// reboot family is the separate cmd 12, excluded.
    ///
    /// Returns the pre-write `shutdownTime` from THIS call's fresh
    /// `get_heart_info()` (§5 offset 10, `ProGetHearInfo.java:73`) — the
    /// value in force on the device immediately before the write (the
    /// crate's uniform SET discipline — one fresh GET inside every SET
    /// call, no observable device state cached; the value is the guarded
    /// test's restore datum, like `set_channel_mode`'s returned mode).
    ///
    /// Ack gating is identical to [`HollyControl::set_channel_mode`]
    /// (checklist §3: the `data[0] == 0` status rule is uniform across all
    /// `ProSet*` classes — `ProSetShutdownTime.receiveData` reads
    /// `toInt(data[0]) == 0` [code `ProSetShutdownTime.java:26-30`]):
    /// `[00]` = success; anything else surfaces as
    /// [`Error::Rejected`]/[`Error::BadAck`] — no retry. Reply-tail bytes
    /// stay uninterpreted: heart replies carry XOR(payload[0..16)) ⊕ 0x68
    /// [wire — LAST payload byte excluded], ACK tails follow
    /// XOR(whole 7+len prefix) ⊕ 0x07 = cmd ⊕ 0x66 [wire, 6 command
    /// shapes] (docs/protocol.md §2 — decode discards everything past
    /// `7+len`, so no decode behaviour changes here either way; for cmd
    /// 0x27 the rule predicts 0x41, settling as the 7th point only when
    /// the run byte-matches).
    pub fn set_shutdown_time(&mut self, value: u8) -> Result<u8, Error> {
        // Fresh GET inside every SET (crate rule). The shutdown-time
        // payload has no preserved byte (§3 Shutdown-power-off row); the
        // snapshot's timer value is returned to the caller as the
        // pre-write state.
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_SHUTDOWN_TIME, vec![value]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(state.shutdown_time),
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

    /// Write the receiver indicator light — the "Indicator light" toggle —
    /// cmd 33 SET_LIGHT, single-byte payload `[{value}]` [code
    /// docs/set-gate-checklist.md §3 Indicator-light row:
    /// `UsbMessengerV2.setLight(int value)` builds
    /// `saveReserved(new byte[]{(byte) value})`
    /// (`UsbMessengerV2.java:214-218`); cmd re-derived from
    /// `ProSetLight.getCmd()` = `PostProtocol.Command.GET_TX_GAIN`
    /// (`ProSetLight.java:15-18`), the constant folding to 33
    /// (`PostProtocol.java:34`; corroborated by the independent constant
    /// `ProtocolV2.HID_SET_LIGHT = 33`, `ProtocolV2.java:45`)]. THE
    /// inverted control: the wire byte space is 0 = light **on** / 1 =
    /// light **off** — the APK's own toggle handler writes the OPPOSITE of
    /// the switch state (`setLight(!z ? 1 : 0)`, switch on → 0,
    /// `HLDevice6302Activity.java:662`) and the heart reads the same
    /// inversion back (`lightStatus` = `toInt(data[11]) == 0` → on,
    /// `ProGetHearInfo.java:75-76` — §5 offset 11; docs/protocol.md §5.2
    /// "0 = light on"). BOTH sides carry the SAME inversion (checklist §3:
    /// "light and speaker are inverted on BOTH sides"), so there is no
    /// inverted-write mismatch to undo and this op is IDENTITY on the
    /// documented space {0, 1}: the caller speaks WIRE bytes (0 = on), and
    /// the UI-bool ↔ wire-byte inversion lives in exactly one place
    /// upstream (holly-app's `vm.rs`, with its one unit test) — never in a
    /// view, never here.
    ///
    /// Returns the pre-write `lightStatus` byte from THIS call's fresh
    /// `get_heart_info()` (§5 offset 11) — the value in force on the
    /// device immediately before the write (the crate's uniform SET
    /// discipline — one fresh GET inside every SET call, no observable
    /// device state cached; the value is the guarded test's restore
    /// datum, like `set_shutdown_time`'s returned timer). The heart's
    /// typed `light_on` already applied the documented parse inversion
    /// (`== 0` → on [code :75-76]); the returned byte is that same field
    /// verbatim in its own wire space (`!light_on`), i.e. payload[11] of
    /// the fresh heart — no second rule is created here.
    ///
    /// Ack gating is identical to [`HollyControl::set_shutdown_time`]
    /// (checklist §3: the `data[0] == 0` status rule is uniform across all
    /// `ProSet*` classes — `ProSetLight.receiveData` reads
    /// `toInt(data[0]) == 0` [code `ProSetLight.java:26-29`]):
    /// `[00]` = success; anything else surfaces as
    /// [`Error::Rejected`]/[`Error::BadAck`] — no retry. Reply-tail bytes
    /// stay uninterpreted: heart replies carry XOR(payload[0..16)) ⊕ 0x68
    /// [wire — LAST payload byte excluded], ACK tails follow
    /// XOR(whole 7+len prefix) ⊕ 0x07 = cmd ⊕ 0x66 [wire, 6 command
    /// shapes] (docs/protocol.md §2 — decode discards everything past
    /// `7+len`, so no decode behaviour changes here either way; for cmd
    /// 0x21 the rule predicts 0x47, settling as the 7th point only when
    /// the run byte-matches). Values outside the documented space {0, 1}
    /// have no certified meaning and are the caller's responsibility,
    /// not assumed here (family convention).
    pub fn set_light(&mut self, value: u8) -> Result<u8, Error> {
        // Fresh GET inside every SET (crate rule). The light payload has
        // no preserved byte (§3 Indicator-light row); the snapshot's
        // lightStatus byte is returned to the caller as the pre-write
        // state (payload[11] verbatim — see the return doc above).
        let state = self.get_heart_info()?;
        let request = Frame::new(cmd::SET_LIGHT, vec![value]);
        let (reply, raw) = self.exchange(request)?;
        match reply.payload.as_slice() {
            [0x00] => Ok(u8::from(!state.light_on)),
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
