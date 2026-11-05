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
//! single false). Every SET uses holly-usb's existing path (fresh GET
//! inside `set_noise`) and is followed by a fresh poll so subscribers see
//! the DEVICE's verdict, not the request.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

use holly_usb::HollyControl;

use crate::{
    ConnStatus, HeartInfo, OpenFn, POLL_INTERVAL, Snapshot, StatusSource, classify_claim,
    device_gone,
};

/// Commands the UI/frontend may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Request an NC toggle. Coalesces to the latest.
    SetNoise(bool),
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
    pending: Option<bool>,
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

    /// Process ONE batch of queued commands with coalescing (latest-wins
    /// for SetNoise).
    pub fn pump<I: IntoIterator<Item = Command>>(&mut self, cmds: I) {
        let mut refresh = false;
        let mut nc: Option<bool> = None;
        for cmd in cmds {
            match cmd {
                Command::Refresh => refresh = true,
                Command::SetNoise(on) => nc = Some(on),
            }
        }
        if self.device.is_none() {
            // Requests while disconnected are dropped (UI disables its
            // toggle when no heart exists); refresh doubles as an
            // immediate reconnect attempt.
            if refresh || nc.is_some() {
                self.open_attempt();
            }
            return;
        }
        if let Some(on) = nc {
            // Pending visible in the snapshot stream BEFORE the blocking
            // SET runs — humans see it if the 3 s control budget stalls.
            self.pending = Some(on);
            self.emit();
            self.execute_set(on);
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
                self.status = ConnStatus::Connected;
                self.heart = Some(heart);
                self.error_clear();
            }
            Err(e) if device_gone(&e) => {
                // Drop the claim first (Drop restores interface state),
                // then re-enter the connect loop from NotFound.
                self.device = None;
                self.pending = None;
                self.heart = None;
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
    /// so subscribers always see the DEVICE's verdict.
    fn execute_set(&mut self, on: bool) {
        let Some(source) = self.device.as_mut() else {
            return;
        };
        match source.set_noise(on) {
            Ok(()) => {
                self.pending = None;
                self.error_clear();
                self.poll_and_emit(); // DEVICE state, not the request
            }
            Err(e) if device_gone(&e) => {
                self.device = None;
                self.pending = None;
                self.heart = None;
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
                // Keep status Error + detail; heart = the fresh read.
                self.heart = Some(heart);
            }
            Err(e) if device_gone(&e) => {
                self.device = None;
                self.heart = None;
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
