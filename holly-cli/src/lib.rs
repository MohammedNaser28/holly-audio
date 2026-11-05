//! holly-cli library half: rendering, exit codes, and the runners.
//!
//! Layering (project rule — logic never duplicates across layers):
//! - framing: `holly-proto` (none here)
//! - transport/protocol gates: `holly-usb` (only called, never bent)
//! - this crate: output formatting, exit-code mapping, watch loop skeleton
//!
//! No OTA, reboot, pairing, or reset functionality exists anywhere in the
//! dependency chain's public API used here (`holly status` / `holly nc` /
//! `holly watch` are the only commands; enforced by the main.rs clap schema).

use clap::{Parser, Subcommand};
use holly_usb::{Error, HeartInfo, HollyControl, Transport};
use serde::Serialize;

pub const EXIT_OTHER: i32 = 1;
/// No dongle 3547:0407 on the bus.
pub const EXIT_NOT_FOUND: i32 = 2;
/// `/dev/bus/usb` write access denied → install packaging/70-hollyland.rules.
pub const EXIT_PERMISSION: i32 = 3;
/// Frame decode/gate failure, or the device rejected/acked wrong.
pub const EXIT_PROTOCOL: i32 = 4;

/// Minimum watch poll interval (task spec: 250 ms).
pub const MIN_INTERVAL_MS: u64 = 250;
/// Default watch interval (task spec: 1000 ms).
pub const DEFAULT_INTERVAL_MS: u64 = 1000;

/// `on`/`off` argument parser for `holly nc` (case-insensitive). Returns a
/// typed value rather than a bare bool — clap cannot attach a bool
/// value-parser to a positional argument.
pub fn parse_onoff(s: &str) -> Result<NcState, String> {
    match s.to_ascii_lowercase().as_str() {
        "on" => Ok(NcState::On),
        "off" => Ok(NcState::Off),
        _ => Err(format!("invalid NC state {s:?}: expected 'on' or 'off'")),
    }
}

/// The two NC states `holly nc` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NcState {
    On,
    Off,
}

/// The full CLI schema. Lives in the lib half so argument parsing is unit-
/// testable; main.rs touches it only for parsing and dispatch.
#[derive(Parser, Debug)]
#[command(
    name = "holly",
    version,
    about = "Hollyland 6108 wireless-mic dongle control (VID 3547 PID 0407)"
)]
pub struct Cli {
    /// Extra detail on protocol failures: raw reply frames in hex.
    #[arg(long)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print the dongle status snapshot.
    Status {
        /// Machine output: one compact JSON object.
        #[arg(long)]
        json: bool,
    },
    /// Turn noise cancellation on or off, verify with a fresh status GET.
    Nc {
        /// Desired NC state: on | off (case-insensitive).
        #[arg(value_parser = parse_onoff, value_name = "on|off")]
        state: NcState,
    },
    /// Poll and print only state changes; Ctrl-C exits cleanly.
    Watch {
        /// Poll interval in milliseconds (minimum 250).
        #[arg(long, default_value_t = DEFAULT_INTERVAL_MS, value_parser = clap::value_parser!(u64).range(MIN_INTERVAL_MS..))]
        interval_ms: u64,
    },
}

/// Map a transport-level failure to the documented exit codes:
/// 2 not-found, 3 permission, 4 protocol/ack, 1 other.
pub fn exit_code(err: &Error) -> i32 {
    match err {
        Error::NotFound => EXIT_NOT_FOUND,
        Error::Usb(io) if io.kind() == std::io::ErrorKind::PermissionDenied => EXIT_PERMISSION,
        Error::Decode(_)
        | Error::Encode(_)
        | Error::CmdMismatch { .. }
        | Error::UnknownDirection { .. }
        | Error::Rejected { .. }
        | Error::BadAck { .. }
        | Error::HeartInfoShort { .. } => EXIT_PROTOCOL,
        Error::Usb(_) => EXIT_OTHER,
    }
}

/// Bytes as two-digit hex columns (lowercase), for --verbose raw frames.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A CLI-level failure: exit code + user-facing line + optional raw detail.
pub struct Failure {
    pub code: i32,
    pub line: String,
    pub detail: Option<String>,
}

impl From<&Error> for Failure {
    /// Never swallows protocol-level errors: [`Error::Rejected`] and
    /// [`Error::BadAck`] keep their Display text and gain a full raw frame
    /// (64-byte reply as received) in `detail` for --verbose printing.
    fn from(err: &Error) -> Self {
        let code = exit_code(err);
        let detail = match err {
            Error::Rejected { status, raw } => {
                Some(format!("{status:#04x} ack, raw reply frame: {}", hex(raw)))
            }
            Error::BadAck { raw, .. } => Some(format!("raw reply frame: {}", hex(raw))),
            _ => None,
        };
        let mut line = err.to_string();
        if let Error::Usb(io) = err
            && io.kind() == std::io::ErrorKind::PermissionDenied
        {
            // actionable, points at the repo's own rule file
            line = "permission denied writing /dev/bus/usb — install \
                    packaging/70-hollyland.rules (sudo cp packaging/70-hollyland.rules \
                    /etc/udev/rules.d/ && sudo udevadm control --reload, then replug \
                    or 'sudo udevadm trigger --action=add /sys/bus/usb/devices/<node>')"
                .to_string();
        }
        Failure { code, line, detail }
    }
}

/// Compact single-line state summary (used by `nc` and by `watch` changes).
pub fn status_line(info: &HeartInfo) -> String {
    format!(
        "nc={} nclevel={} tx1={} tx1bat={} tx2={} tx2bat={} voice={} voiceMode={} eq={} reverb={} reverbLvl={} signal={}",
        onoff(info.noise_switch),
        info.noise_level,
        onoff(info.tx1_status),
        info.tx1_battery,
        onoff(info.tx2_status),
        info.tx2_battery,
        info.voice_level,
        info.voice_mode,
        info.eq_level,
        onoff(info.reverb_switch),
        info.reverb_level,
        info.signal_mode,
    )
}

/// `true` → "on", `false` → "off" (the two-value vocabulary of `nc`).
pub fn onoff(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

/// Multi-line human-readable status (the `holly status` default).
pub fn status_text(info: &HeartInfo) -> String {
    let conn = |s: bool| if s { "connected" } else { "disconnected" };
    let mut out = String::from("Hollyland 6108 (3547:0407) status\n");
    out.push_str(&format!(
        "  TX1: {:<11} battery {:3}%  mute {}\n",
        conn(info.tx1_status),
        info.tx1_battery,
        onoff(info.tx1_mute),
    ));
    out.push_str(&format!(
        "  TX2: {:<11} battery {:3}%  mute {}\n",
        conn(info.tx2_status),
        info.tx2_battery,
        onoff(info.tx2_mute),
    ));
    out.push_str(&format!(
        "  Noise cancel: {} (level {})\n",
        onoff(info.noise_switch),
        info.noise_level,
    ));
    out.push_str(&format!(
        "  Voice: level {}, mode {};  EQ level {}\n",
        info.voice_level, info.voice_mode, info.eq_level,
    ));
    out.push_str(&format!(
        "  Reverb: {} (level {});  shutdown timer value {}\n",
        onoff(info.reverb_switch),
        info.reverb_level,
        info.shutdown_time,
    ));
    out.push_str(&format!(
        "  Light: {};  identify beep: {};  signal mode {}\n",
        onoff(info.light_on),
        onoff(info.tx_identify),
        info.signal_mode,
    ));
    if info.tx3_status.is_some() || info.tx4_status.is_some() {
        out.push_str(&format!(
            "  TX3 status {:?} battery {:?} mute {:?}\n",
            info.tx3_status, info.tx3_battery, info.tx3_mute,
        ));
        out.push_str(&format!(
            "  TX4 status {:?} battery {:?} mute {:?}\n",
            info.tx4_status, info.tx4_battery, info.tx4_mute,
        ));
    }
    out
}

/// Machine output for `holly status --json`. Field names mirror the §5
/// payload semantics one-to-one; absent multi-TX fields are omitted.
#[derive(Debug, Clone, PartialEq, Serialize, serde :: Deserialize)]
pub struct StatusJson {
    pub tx1_status: bool,
    pub tx1_battery: u8,
    pub tx1_mute: bool,
    pub tx2_status: bool,
    pub tx2_battery: u8,
    pub tx2_mute: bool,
    pub noise_switch: bool,
    pub noise_level: u8,
    pub voice_level: u8,
    pub voice_mode: u8,
    pub shutdown_time: u8,
    pub light_on: bool,
    pub eq_level: u8,
    pub reverb_switch: bool,
    pub reverb_level: u8,
    pub tx_identify: bool,
    pub signal_mode: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx3_status: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx4_status: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx3_battery: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx4_battery: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx3_mute: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx4_mute: Option<bool>,
}

impl From<&HeartInfo> for StatusJson {
    fn from(i: &HeartInfo) -> Self {
        Self {
            tx1_status: i.tx1_status,
            tx1_battery: i.tx1_battery,
            tx1_mute: i.tx1_mute,
            tx2_status: i.tx2_status,
            tx2_battery: i.tx2_battery,
            tx2_mute: i.tx2_mute,
            noise_switch: i.noise_switch,
            noise_level: i.noise_level,
            voice_level: i.voice_level,
            voice_mode: i.voice_mode,
            shutdown_time: i.shutdown_time,
            light_on: i.light_on,
            eq_level: i.eq_level,
            reverb_switch: i.reverb_switch,
            reverb_level: i.reverb_level,
            tx_identify: i.tx_identify,
            signal_mode: i.signal_mode,
            tx3_status: i.tx3_status,
            tx4_status: i.tx4_status,
            tx3_battery: i.tx3_battery,
            tx4_battery: i.tx4_battery,
            tx3_mute: i.tx3_mute,
            tx4_mute: i.tx4_mute,
        }
    }
}

/// `holly status --json`: one compact JSON object.
pub fn status_json(info: &HeartInfo) -> String {
    serde_json::to_string(&StatusJson::from(info)).expect("StatusJson always serializes")
}

/// Run one status poll and render it.
pub fn run_status<T: Transport>(ctl: &mut HollyControl<T>, json: bool) -> Result<String, Error> {
    let info = ctl.get_heart_info()?;
    Ok(if json {
        status_json(&info)
    } else {
        status_text(&info)
    })
}

/// Failure of `holly nc <on|off>`.
#[derive(Debug)]
pub enum NcError {
    /// SET or the follow-up GET failed at transport/protocol level.
    Usb(Error),
    /// SET acked success, but the fresh GET reports a different switch state
    /// than requested — the device did not apply the request.
    Mismatch(HeartInfo),
}

/// `holly nc <on|off>`: set, then verify with a fresh GET (the same
/// guarded pattern as the capture tool; no cached state anywhere).
pub fn run_nc<T: Transport>(ctl: &mut HollyControl<T>, on: bool) -> Result<HeartInfo, NcError> {
    ctl.set_noise(on).map_err(NcError::Usb)?;
    let info = ctl.get_heart_info().map_err(NcError::Usb)?;
    if info.noise_switch != on {
        return Err(NcError::Mismatch(info));
    }
    Ok(info)
}

/// `holly watch` polling core — one step per call, deliberately behind a
/// function boundary: an EP 0x81 interrupt reader (docs/device.md IF0 IN
/// endpoint) replaces the body of [`Watcher::poll`] later without any
/// caller change.
pub struct Watcher<T: Transport> {
    ctl: HollyControl<T>,
    /// Last payload seen (full raw §5 bytes: ANY state change counts,
    /// including battery drift while NC toggles nothing).
    last: Option<Vec<u8>>,
}

impl<T: Transport> Watcher<T> {
    pub fn new(ctl: HollyControl<T>) -> Self {
        Self { ctl, last: None }
    }

    /// Poll once. Returns `Some(line)` when the state differs from the
    /// previously seen state (the very first poll always yields the
    /// baseline line so the operator knows watching is live).
    pub fn poll(&mut self) -> Result<Option<String>, Error> {
        let info = self.ctl.get_heart_info()?;
        let changed = self.last.as_ref() != Some(&info.raw);
        self.last = Some(info.raw.clone());
        Ok(changed.then(|| status_line(&info)))
    }
}
