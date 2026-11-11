//! The device actor: ONE thread owning the claim, plus the synchronous
//! [`Core`] state machine it drives.
//!
//! Threading is deliberate: std-only (`std::sync::mpsc` + `std::thread`), so
//! any frontend — gpui, Tauri, or a test binary — adapts without sharing an
//! async runtime. Frontends must NEVER call nusb; they talk to
//! [`DeviceHandle`] and read [`Snapshot`]s.
//!
//! Commands: one SET in flight at a time; queued requests coalesce to the
//! latest (`Core::pump` collapses `SetNoise(true)…SetNoise(false)` to a
//! single false, and `SetEq(2)…SetEq(3)` — or any SET command at all — to a
//! single newest intent, see `Command::SetEq`). Every SET uses holly-usb's
//! existing path (fresh GET inside `set_noise`/`set_eq`) and is followed by
//! a fresh poll so subscribers see the DEVICE's verdict, not the request.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use holly_usb::HollyControl;

use serde::Serialize;

use crate::{
    ConnStatus, HeartInfo, OpenFn, POLL_INTERVAL, Snapshot, StatusSource, classify_claim,
    device_gone,
};

/// Wall-clock stamp for [`Snapshot::received_at`] — integer UNIX seconds.
/// App-side origin information; never device data and never invented.
fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The in-flight noise write — which NC byte is being changed. This is the
/// Noise variant's payload type of [`PendingWrite`] (serde for the JSON
/// snapshot test): RENAMES are for serialization only; the variants carry
/// exactly what the corresponding holly-usb operation sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseWrite {
    /// `set_noise(on)`: the flips to this value; the level byte is whatever
    /// the fresh GET inside holly-usb reports.
    Switch(bool),
    /// `set_noise_level(l)`: the level byte goes to `l`; the switch stays
    /// as the fresh GET reports (holly-usb preserves, replacing the APK's
    /// always-on form — its level buttons send `isOpen=true`);
    /// `HLDevice6302Activity.java:1593/:1610/:1626-1631`.
    Level(u8),
}

/// WHAT is in flight when a SET is between "accepted" and "device verdict"
/// — the payload type of [`Snapshot::pending_set`]. One slot (one SET runs
/// at a time), so the variants are exclusive by construction. UI shows
/// "applying" ONLY from a snapshot's pending — never optimistically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingWrite {
    /// A NC write (toggle flip or level pick) is in flight.
    Noise(NoiseWrite),
    /// An EQ pad write is in flight: the byte is the WIRE pad being written
    /// (`0x02` = Bright on the wire — the display-order mapping lives in
    /// `vm::eq_selection`/`vm::eq_pad_wire`, never here).
    Eq(u8),
    /// A gain (Volume adjustment) write is in flight: the byte is the WIRE
    /// stop being written (0..=5 = the display stops "1".."6"; the identity
    /// lives in `vm::volume_selection`/`vm::volume_pad_wire`, never here).
    Gain(u8),
    /// A reverb write is in flight: the bytes are the WIRE pair being
    /// written, `(switch, gear)` — the frame builds the pair as
    /// `[isOpen, gear]`, switch FIRST (checklist §3). The identity mapping
    /// lives in `vm::reverb_toggle_wire`/`vm::reverb_gear_wire`, never
    /// here.
    Reverb(bool, u8),
    /// A channel-mode write is in flight: the byte is the WIRE value being
    /// written (0 = mono, 1 = stereo — checklist §3). The identity mapping
    /// lives in `vm::channel_mode_selection`/`vm::channel_mode_pad_wire`,
    /// never here.
    ChannelMode(u8),
    /// A TX-mute write is in flight: the bytes are the WIRE pair being
    /// written, `(tx, muted)` — the 0-based slot (TX1=0 … TX4=3) then the
    /// mute byte, one TX per frame (checklist §3 TX-mute row). The
    /// identity mapping lives in `vm::tx_mute_wire`, never here.
    TxMute { tx: u8, muted: bool },
    /// A shutdown-timer write is in flight: the byte is the WIRE value
    /// being written (0 = 15min, 1 = Never — checklist §3
    /// Shutdown-power-off row, identity on both sides). The identity
    /// mapping lives in `vm::shutdown_pad_wire`, never here.
    Shutdown(u8),
}

/// Commands the UI/frontend may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Request an NC switch flip. Coalesces to the latest.
    SetNoise(bool),
    /// Request an NC level write. Coalesces to the latest (inter-command:
    /// a later Level replaces an earlier Switch in the same batch and vice
    /// versa — one SET goes out per batch, always the newest intent).
    SetNoiseLevel(u8),
    /// Request an EQ pad write, `pad` in the WIRE value space 1..=3
    /// (1=Low/OVERCAST, 2=Bright, 3="Equalization"/BALANCE — checklist §3
    /// EQ row, §5.2); the DISPLAY-order translation lives in the VM
    /// (`eq_pad_wire`), never in a frontend. Coalesces to the latest like
    /// every other SET: one SET goes out per batch, always the newest
    /// intent (an `Eq` replacing a `SetNoise*` or the reverse is the same
    /// latest-wins rule — the UI never queues a second write in flight, so
    /// the replacement only spans one batch window).
    SetEq(u8),
    /// Request a Volume-stop write, `stop` in the WIRE value space 0..=5
    /// (the display stops "1".."6" — checklist §3 Gain row: write space ==
    /// read space, identity). The DISPLAY-order translation lives in the
    /// VM (`volume_pad_wire`), never in a frontend. Coalesces to the
    /// latest like every other SET (one SET per batch, newest intent).
    SetGain(u8),
    /// Request a reverb write, `(switch, gear)` in the WIRE value spaces
    /// (switch free, gear 0..=2 — checklist §3 Reverb row: write space ==
    /// read space, identity). The DISPLAY-order translations live in the
    /// VM (`reverb_toggle_wire`/`reverb_gear_wire`), never in a frontend.
    /// Coalesces to the latest like every other SET (one SET per batch,
    /// newest intent).
    SetReverb(bool, u8),
    /// Request a Channel-mode write, `value` in the WIRE value space
    /// 0..=1 (0=mono, 1=stereo — checklist §3 Channel-mode row: write
    /// space == read space, identity). The DISPLAY-order translation
    /// lives in the VM (`channel_mode_pad_wire`), never in a frontend.
    /// Coalesces to the latest like every other SET (one SET per batch,
    /// newest intent).
    SetChannelMode(u8),
    /// Request a TX-mute write, `tx` the WIRE 0-based slot (TX1=0 … TX4=3)
    /// and `muted` the mute byte 0/1 — one TX per frame (checklist §3
    /// TX-mute row: write space == read space, §5 offsets 4/5). The
    /// DISPLAY-order translation lives in the VM (`tx_mute_wire`), never
    /// in a frontend. Coalesces to the latest like every other SET (one
    /// SET per batch, newest intent).
    SetTxMute { tx: u8, muted: bool },
    /// Request a Schedule-power-off write, `value` in the WIRE value space
    /// {0, 1} (0 = 15min, 1 = Never — checklist §3 Shutdown-power-off row:
    /// write space == read space, identity). The identity translation
    /// lives in the VM (`shutdown_pad_wire`), never in a frontend.
    /// Coalesces to the latest like every other SET (one SET per batch,
    /// newest intent).
    SetSchedulePowerOff(u8),
    /// Force one fresh poll/reconnect attempt now.
    Refresh,
}

#[derive(Default)]
struct HubInner {
    /// One bounded channel per subscriber; lagged or dead subscribers are
    /// dropped silently on the next emit (change-only, never a backlog).
    subs: Vec<SyncSender<Snapshot>>,
    /// The last emitted snapshot, handed to NEW subscribers immediately so
    /// a late-joining UI does not wait a full poll cycle for state.
    last: Option<Snapshot>,
}

/// Fan-out point for snapshots. Shared between handle(s) and actor.
#[derive(Default)]
pub struct Hub(Mutex<HubInner>);

impl Hub {
    /// Subscribe to change-only snapshots. The current last snapshot is
    /// delivered first when one already exists. Public also for
    /// synchronous-`Core` drivers (tests, executor-bridged UIs) that hold a
    /// [`Hub`] directly instead of a [`DeviceHandle`].
    pub fn subscribe(&self) -> Receiver<Snapshot> {
        let (tx, rx) = sync_channel::<Snapshot>(16);
        {
            let mut inner = self.0.lock().unwrap_or_else(|p| p.into_inner());
            // Seed BEFORE registration so a change racing the subscription
            // cannot skip this subscriber.
            if let Some(last) = inner.last.clone() {
                let _ = tx.send(last);
            }
            inner.subs.push(tx);
        }
        rx
    }

    fn push(&self, snap: Snapshot) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.last = Some(snap.clone());
        inner.subs.retain(|sub| sub.try_send(snap.clone()).is_ok());
    }

    fn current(&self) -> Option<Snapshot> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last
            .clone()
    }
}

/// Cloneable uplink to the running actor. Dropping every clone stops the
/// actor thread (channel disconnect) and, through it, releases the claim.
#[derive(Clone)]
pub struct DeviceHandle {
    tx: Sender<Command>,
    hub: Arc<Hub>,
}

impl DeviceHandle {
    pub fn set_noise(&self, on: bool) {
        let _ = self.tx.send(Command::SetNoise(on));
    }

    pub fn set_noise_level(&self, level: u8) {
        let _ = self.tx.send(Command::SetNoiseLevel(level));
    }

    /// Request an EQ pad write (WIRE pad 1..=3; display-order translation
    /// is the VM's `eq_pad_wire`). Fire-and-forget like every other send:
    /// the snapshot stream reports "pending" then the device verdict.
    pub fn set_eq(&self, pad: u8) {
        let _ = self.tx.send(Command::SetEq(pad));
    }

    /// Request a Volume-stop write (WIRE stop 0..=5; display-order
    /// translation is the VM's `volume_pad_wire`). Fire-and-forget like
    /// every other send: the snapshot stream reports "pending" then the
    /// device verdict.
    pub fn set_gain(&self, stop: u8) {
        let _ = self.tx.send(Command::SetGain(stop));
    }

    /// Request a reverb write (WIRE pair `(switch, gear)`; display-order
    /// translation is the VM's `reverb_toggle_wire`/`reverb_gear_wire`).
    /// Fire-and-forget like every other send: the snapshot stream reports
    /// "pending" then the device verdict.
    pub fn set_reverb(&self, switch: bool, gear: u8) {
        let _ = self.tx.send(Command::SetReverb(switch, gear));
    }

    /// Request a Channel-mode write (WIRE value 0..=1; display-order
    /// translation is the VM's `channel_mode_pad_wire`). Fire-and-forget
    /// like every other send: the snapshot stream reports "pending" then
    /// the device verdict.
    pub fn set_channel_mode(&self, value: u8) {
        let _ = self.tx.send(Command::SetChannelMode(value));
    }

    /// Request a TX-mute write (WIRE pair `(tx, muted)`; display-order
    /// translation is the VM's `tx_mute_wire`). Fire-and-forget like
    /// every other send: the snapshot stream reports "pending" then the
    /// device verdict.
    pub fn set_tx_mute(&self, tx: u8, muted: bool) {
        let _ = self.tx.send(Command::SetTxMute { tx, muted });
    }

    /// Request a Schedule-power-off write (WIRE value {0, 1}; identity
    /// translation is the VM's `shutdown_pad_wire`). Fire-and-forget like
    /// every other send: the snapshot stream reports "pending" then the
    /// device verdict.
    pub fn set_schedule_power_off(&self, value: u8) {
        let _ = self.tx.send(Command::SetSchedulePowerOff(value));
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Command::Refresh);
    }

    /// Subscribe to change-only snapshots (delegates to [`Hub::subscribe`]).
    pub fn subscribe(&self) -> Receiver<Snapshot> {
        self.hub.subscribe()
    }

    /// Last emitted snapshot without subscribing (one-shot renderers).
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.hub.current()
    }
}

/// Coalescing window: one pump processes at most this many queued commands
/// — generous for a human-driven UI, bounded so a runaway producer cannot
/// starve the device thread.
const PUMP_MAX: usize = 32;

/// Synchronous device state machine. Thread-less on purpose: tests drive
/// `pump`/`tick` deterministically; only [`start`] wraps it in a thread.
/// Public by design — a frontend may also drive the machine itself on its
/// own executor instead of the ready-made thread.
pub struct Core {
    factory: OpenFn,
    hub: Arc<Hub>,
    /// The claimed dongle. `None` = no claim held (holly-usb's Drop path
    /// on the dropped box restores interface/driver state).
    device: Option<Box<dyn StatusSource + Send>>,
    /// Canonical state element set — the emitter composes from these.
    status: ConnStatus,
    detail: Option<String>,
    heart: Option<HeartInfo>,
    /// A SET that was accepted but not yet executed (latest-wins).
    pending: Option<PendingWrite>,
    /// UNIX-epoch seconds of the last successful poll (wall clock, set by
    /// the actor — app-side metadata, never device data). `None` until the
    /// first successful GET; cleared when the claim is lost. Lives on the
    /// Core (not inside `snapshot()`) so the emit-on-change dedup stays
    /// byte-stable between polls.
    updated: Option<u64>,
    /// Last composed snapshot (emit-on-change dedup base).
    last: Option<Snapshot>,
}

impl Core {
    pub fn new(factory: OpenFn, hub: Arc<Hub>) -> Self {
        let mut core = Self {
            factory,
            hub,
            device: None,
            status: ConnStatus::NotFound,
            detail: None,
            heart: None,
            pending: None,
            updated: None,
            last: None,
        };
        // Baseline BEFORE anything ran: a subscriber joining right after
        // `start_with_factory` sees a true NotFound first.
        core.emit();
        core
    }

    /// Compose the snapshot from the canonical state; emit only on change.
    fn emit(&mut self) {
        let snap = self.snapshot();
        if self.last.as_ref() != Some(&snap) {
            self.hub.push(snap.clone());
            self.last = Some(snap);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        // Compose WITHOUT emitting — tests read expectations from here.
        Snapshot {
            status: self.status.clone(),
            detail: self.detail.clone(),
            heart: self.heart.clone(),
            pending_set: self.pending,
            received_at: self.updated,
        }
    }

    /// One tick with no queued commands. Connected: poll heart. Disconnected:
    /// one claim attempt (the caller's 1 s recv window is the backoff).
    pub fn tick(&mut self) {
        if self.device.is_some() {
            self.poll_and_emit();
        } else {
            self.open_attempt();
        }
    }

    /// Process ONE batch of queued commands with coalescing (latest-wins:
    /// within the batch, any SET command — NC switch, NC level, EQ pad,
    /// Volume stop, or reverb write — replaces its predecessor; only the
    /// newest SET goes out).
    pub fn pump<I: IntoIterator<Item = Command>>(&mut self, cmds: I) {
        let mut refresh = false;
        let mut write: Option<PendingWrite> = None;
        for cmd in cmds {
            match cmd {
                Command::Refresh => refresh = true,
                Command::SetNoise(on) => write = Some(PendingWrite::Noise(NoiseWrite::Switch(on))),
                Command::SetNoiseLevel(l) => {
                    write = Some(PendingWrite::Noise(NoiseWrite::Level(l)))
                }
                Command::SetEq(pad) => write = Some(PendingWrite::Eq(pad)),
                Command::SetGain(stop) => write = Some(PendingWrite::Gain(stop)),
                Command::SetReverb(switch, gear) => {
                    write = Some(PendingWrite::Reverb(switch, gear))
                }
                Command::SetChannelMode(value) => write = Some(PendingWrite::ChannelMode(value)),
                Command::SetTxMute { tx, muted } => {
                    write = Some(PendingWrite::TxMute { tx, muted })
                }
                Command::SetSchedulePowerOff(value) => write = Some(PendingWrite::Shutdown(value)),
            }
        }
        if self.device.is_none() {
            // Requests while disconnected are dropped (UI disables its
            // controls when no heart exists); refresh doubles as an
            // immediate reconnect attempt.
            if refresh || write.is_some() {
                self.open_attempt();
            }
            return;
        }
        if let Some(write) = write {
            // Pending visible in the snapshot stream BEFORE the blocking
            // SET runs — humans see it if the 3 s control budget stalls.
            self.pending = Some(write);
            self.emit();
            self.execute_set(write);
        } else if refresh {
            self.poll_and_emit();
        }
    }

    /// ONE claim attempt; reports every failure kind through [`ConnStatus`].
    fn open_attempt(&mut self) {
        // Entering the attempt phase resets any prior terminal-condition
        // decoration; the machine is NotFound -> Connecting -> ...
        self.status = ConnStatus::Connecting;
        self.detail = None;
        self.heart = None;
        self.pending = None;
        self.updated = None;
        self.device = None; // never double-holding
        self.emit();
        match (self.factory)() {
            Ok(device) => {
                self.device = Some(device);
                self.status = ConnStatus::Connected;
                self.error_clear();
                self.poll_and_emit();
            }
            Err(e) => {
                self.status = classify_claim(&e);
                self.detail = Some(e.to_string());
                self.emit();
            }
        }
    }

    fn error_set(&mut self, msg: String) {
        self.detail = Some(msg);
    }

    fn error_clear(&mut self) {
        self.detail = None;
    }

    /// One GET round trip (read-only path).
    fn poll_and_emit(&mut self) {
        let Some(source) = self.device.as_mut() else {
            return;
        };
        match source.poll() {
            Ok(heart) => {
                // Stamp wall-clock only when device-visible content moved,
                // or on the session's first success: `emit` dedups on full
                // snapshot equality, so re-stamping every poll would push a
                // fresh 1 Hz snapshot even with an identical heart. A stable
                // device keeps the stamp that received its current verdict
                // (footer semantics: "state received at T").
                let changed = self.heart.as_ref() != Some(&heart)
                    || self.status != ConnStatus::Connected
                    || self.detail.is_some();
                self.status = ConnStatus::Connected;
                self.heart = Some(heart);
                self.error_clear();
                if changed || self.updated.is_none() {
                    self.updated = Some(unix_secs());
                }
            }
            Err(e) if device_gone(&e) => {
                // Drop the claim first (Drop restores interface state),
                // then re-enter the connect loop from NotFound.
                self.device = None;
                self.pending = None;
                self.heart = None;
                self.updated = None;
                self.status = ConnStatus::NotFound;
                self.error_set(e.to_string());
            }
            Err(e) => {
                // Protocol-level: abort-and-say (no retry; docs/protocol.md
                // §2). Device stays claimed; next poll arbitrates recovery.
                self.status = ConnStatus::Error;
                self.error_set(e.to_string());
            }
        }
        self.emit();
    }

    /// One SET round trip — the single in-flight SET — then a fresh poll
    /// so subscribers always see the DEVICE's verdict. For EQ/gain/reverb,
    /// holly-usb's `set_eq`/`set_gain`/`set_reverb` additionally return the
    /// fresh GET's pre-write value; nothing here consumes it for STATE (the
    /// post-SET poll is the verdict the snapshot carries — never
    /// optimistic); the value exists for the guarded restore flows
    /// (holly-usb tests).
    fn execute_set(&mut self, write: PendingWrite) {
        let Some(source) = self.device.as_mut() else {
            return;
        };
        let result = match write {
            PendingWrite::Noise(NoiseWrite::Switch(on)) => source.set_noise(on),
            PendingWrite::Noise(NoiseWrite::Level(level)) => source.set_noise_level(level),
            PendingWrite::Eq(pad) => source.set_eq(pad).map(|_previous| ()),
            PendingWrite::Gain(stop) => source.set_gain(stop).map(|_previous| ()),
            PendingWrite::Reverb(switch, gear) => {
                source.set_reverb(switch, gear).map(|_previous| ())
            }
            PendingWrite::ChannelMode(value) => source.set_channel_mode(value).map(|_previous| ()),
            PendingWrite::TxMute { tx, muted } => source.set_tx_mute(tx, muted).map(|_previous| ()),
            PendingWrite::Shutdown(value) => source.set_shutdown_time(value).map(|_previous| ()),
        };
        match result {
            Ok(()) => {
                self.pending = None;
                self.error_clear();
                self.poll_and_emit(); // DEVICE state, not the request
            }
            Err(e) if device_gone(&e) => {
                self.device = None;
                self.pending = None;
                self.heart = None;
                self.updated = None;
                self.status = ConnStatus::NotFound;
                self.error_set(e.to_string());
                self.emit();
            }
            Err(e) => {
                // Rejected/BadAck/decode: NEVER swallowed. Fresh
                // verification GET so the snapshot carries the device's
                // real state next to the error text.
                self.pending = None;
                self.error_set(e.to_string());
                self.status = ConnStatus::Error;
                self.emit();
                self.poll_after_set_error();
            }
        }
    }

    /// After a protocol-level SET failure: one more GET to embed the real
    /// device state (`heart`) in the Error snapshot.
    fn poll_after_set_error(&mut self) {
        let Some(source) = self.device.as_mut() else {
            // Vanished mid-SET: re-enter connect loop with the error kept.
            self.status = ConnStatus::NotFound;
            self.emit();
            return;
        };
        match source.poll() {
            Ok(heart) => {
                // Keep status Error + detail; heart = the fresh read. Stamp
                // only on change — same cadence rule as poll_and_emit.
                let changed = self.heart.as_ref() != Some(&heart);
                self.heart = Some(heart);
                if changed || self.updated.is_none() {
                    self.updated = Some(unix_secs());
                }
            }
            Err(e) if device_gone(&e) => {
                self.device = None;
                self.heart = None;
                self.updated = None;
                self.status = ConnStatus::NotFound;
                self.error_set(e.to_string());
            }
            Err(_) => {
                // Still unparseable; nothing else to say. Next tick retries.
            }
        }
        self.emit();
    }
}

/// Production wiring: real factory (`HollyControl::open`), 1 s poll, 1 s
/// reconnect backoff (the actor's idle receive window while disconnected;
/// see [`run_loop`]). Stopping: drop every handle clone.
pub fn start() -> DeviceHandle {
    let factory: OpenFn =
        Box::new(|| HollyControl::open().map(|ctl| Box::new(ctl) as Box<dyn StatusSource + Send>));
    start_with_factory(factory)
}

/// Same runtime loop, explicit factory — used for UI binaries built on
/// scripted factories and by tests driving the real thread.
pub fn start_with_factory(factory: OpenFn) -> DeviceHandle {
    let (tx, rx) = std::sync::mpsc::channel::<Command>();
    let hub = Arc::new(Hub::default());
    let handle = DeviceHandle {
        tx,
        hub: Arc::clone(&hub),
    };

    std::thread::Builder::new()
        .name("holly-app-device".to_string())
        .spawn(move || run_loop(factory, hub, rx))
        .expect("actor thread: spawn");

    handle
}

fn run_loop(factory: OpenFn, hub: Arc<Hub>, rx: Receiver<Command>) {
    let mut core = Core::new(factory, hub);
    loop {
        // Batch window: everything that queued while the previous device
        // operation ran arrives together and coalesces here.
        let mut batch: Vec<Command> = Vec::with_capacity(4);
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(cmd) => {
                batch.push(cmd);
                while batch.len() < PUMP_MAX {
                    match rx.try_recv() {
                        Ok(cmd) => batch.push(cmd),
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if batch.is_empty() {
            core.tick();
        } else {
            core.pump(batch);
        }
        // While disconnected this 1 s idle window IS the reconnect
        // backoff (RECONNECT_BACKOFF == POLL_INTERVAL by value, documented
        // in lib.rs so both knobs stay findable).
    }
    // Actor exits here: Core (and its claimed dongle) drops, restoring
    // interface state via holly-usb's Drop path.
}
