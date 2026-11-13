//! OS-audio level tap — the REAL per-mic level source behind
//! [`StatusSource::tx_levels`] (the wire has no level field; the APK
//! measures its meter from local PCM [code AudioRecordUtils.java]).
//!
//! `pw-record` (PipeWire) captures the dongle's UAC input — the desktop
//! twin of the APK's own AudioRecord path — as raw S16LE 48 kHz stereo.
//! The reader thread cuts ~50 ms windows (the APK's own meter tick
//! [code AudioRecordUtils.java mLock.wait(50L)]), computes each channel's
//! RMS dBFS with the APK's exact formula, and publishes the latest pair.
//! Discovery finds the dongle's capture node from
//! `pactl list sources short` by the device's own USB strings
//! (inputs/lsusb-v.txt:16 iProduct "Wireless Microphone", :12
//! iManufacturer "Shenzhen Hollyland Technology Co.,Ltd");
//! `HOLLY_MIC_TARGET` overrides the matched node.
//!
//! Dormancy and honesty rules: with no dongle node there is NO child
//! process and levels stay `None` (the meter renders the honest
//! "nothing to display" zero). A child that dies mid-session (PipeWire
//! restart, dongle unplug) drops levels back to `None` and the cadence
//! to the poll — no respawn, no fabricated stillness.
//!
//! UNVERIFIED until a [wire] hardware session: the channel→TX-slot
//! mapping (ch0→TX1, ch1→TX2 is an assumption), the negotiated channel
//! count of the dongle's UAC node (forced to 2 here so the parser is
//! deterministic), and level fidelity against the Lark app's own meter.
//!
//! Layering: std-only (`std::process`, threads, no new dependencies),
//! wraps any [`StatusSource`] — the wrapped source's device round trips
//! delegate untouched to holly-usb.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{Error, HeartInfo, StatusSource};

/// Meter cadence: 50 ms — the APK's own [code AudioRecordUtils.java
/// mLock.wait(50L)].
pub const TAP_INTERVAL: Duration = Duration::from_millis(50);

/// Capture format forced through PipeWire's converter regardless of the
/// node's native format: S16LE stereo at 48 kHz. 50 ms of that is
/// 9600 bytes (2400 frames × 2 ch × 2 bytes).
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const WINDOW_BYTES: usize = SAMPLE_RATE as usize / 20 * CHANNELS * 2;

/// The tap's published state, shared between the reader thread and
/// [`TappedSource`]. Levels start `None` (no measurement yet) and return
/// there when the child dies — the meter's honest zero, never a frozen
/// last frame.
struct TapState {
    levels: Mutex<[Option<i16>; 2]>,
    alive: AtomicBool,
}

impl TapState {
    fn new() -> Self {
        Self {
            levels: Mutex::new([None, None]),
            alive: AtomicBool::new(true),
        }
    }

    fn levels(&self) -> [Option<i16>; 2] {
        *self.levels.lock().unwrap()
    }

    /// Level cadence while the reader is alive; `None` once the child is
    /// gone (levels are cleared too — the actor's window reverts to the
    /// poll without any reset plumbing).
    fn level_interval(&self) -> Option<Duration> {
        self.alive.load(Ordering::Relaxed).then_some(TAP_INTERVAL)
    }

    /// Reader-thread exit: clear the levels and stop advertising a
    /// cadence.
    fn set_dead(&self) {
        self.alive.store(false, Ordering::Relaxed);
        *self.levels.lock().unwrap() = [None, None];
    }
}

/// Owns the `pw-record` child; dropping it kills the child (the reader
/// thread then EOFs and clears the state via [`TapState::set_dead`]).
/// `child` is `None` only in unit tests, which drive [`TapState`]
/// directly.
struct TapHandle {
    state: Arc<TapState>,
    child: Option<Child>,
}

impl Drop for TapHandle {
    fn drop(&mut self) {
        self.state.set_dead();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The dongle's UAC capture node name, or `None` when it is not on the
/// bus (no child is spawned then — the tap stays fully dormant).
/// Matches the device's OWN USB strings from inputs/lsusb-v.txt; the
/// `HOLLY_MIC_TARGET` env override handles a node name the string match
/// does not cover.
fn discover_target() -> Option<String> {
    // Explicit override first: a node name the string match does not
    // cover (or a deliberate test target).
    let override_target = std::env::var("HOLLY_MIC_TARGET")
        .ok()
        .filter(|target| !target.is_empty());
    if let Some(target) = override_target {
        return Some(target);
    }
    let out = Command::new("pactl")
        .arg("list")
        .arg("sources")
        .arg("short")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(|line| line.split('\t').next().unwrap_or(""))
        .filter(|name| name.starts_with("alsa_input.usb-"))
        .find(|name| {
            let lower = name.to_ascii_lowercase();
            lower.contains("hollyland") || lower.contains("wireless_microphone")
        })
        .map(str::to_string)
}

/// Spawn `pw-record` on the target node and the reader thread. `None` =
/// pw-record missing or spawn failed → dormant tap (levels stay `None`).
fn spawn_tap(target: &str) -> Option<TapHandle> {
    let mut child = Command::new("pw-record")
        .args([
            "--raw",
            "--format",
            "s16",
            "--rate",
            &SAMPLE_RATE.to_string(),
            "--channels",
            &CHANNELS.to_string(),
            "--target",
            target,
            "-", // stdout
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let state = Arc::new(TapState::new());
    let reader_state = Arc::clone(&state);
    std::thread::Builder::new()
        .name("holly-mic-tap".to_string())
        .spawn(move || reader_loop(&mut stdout, &reader_state))
        .ok()?;
    Some(TapHandle {
        state,
        child: Some(child),
    })
}

/// Read the S16LE stream, cut [`WINDOW_BYTES`] windows, publish each
/// channel's RMS dBFS. EOF/error (child exit — dongle gone, PipeWire
/// restart): levels back to `None`, cadence reverts to the poll.
fn reader_loop(stdout: &mut impl Read, state: &TapState) {
    let mut window: Vec<u8> = Vec::with_capacity(WINDOW_BYTES);
    let mut chunk = [0u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                window.extend_from_slice(&chunk[..n]);
                while window.len() >= WINDOW_BYTES {
                    let levels = window_dbfs(&window[..WINDOW_BYTES]);
                    window.drain(..WINDOW_BYTES);
                    *state.levels.lock().unwrap() = levels;
                }
            }
            Err(_) => break,
        }
    }
    state.set_dead();
}

/// The APK's exact RMS→dBFS [code AudioRecordUtils.java:305-320]:
/// `sqrt(Σs²/len)`; RMS == 0 → the −99.0 silence sentinel; else
/// `20·log10(rms / 32767.0)` — divisor 32767.0, NOT 32768. Published as
/// the integer dB the meter curve consumes: the APK casts the double
/// with `(int)` (truncation toward zero) [code BleSingleVolumeView.java
/// getVolumeProgress2((int) volume)], so the cast — not a round — is the
/// faithful step; clamped to the documented −99..=0 snapshot domain.
fn dbfs_i16(samples: &[i16]) -> i16 {
    let sum: f64 = samples.iter().map(|s| *s as f64 * *s as f64).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    let db = if rms == 0.0 {
        -99.0
    } else {
        20.0 * (rms / 32767.0).log10()
    };
    (db.max(-99.0) as i16).min(0)
}

/// Per-channel dBFS of one interleaved S16LE stereo window: first pair
/// = ch0, second = ch1. The channel→TX-slot mapping (ch0→TX1, ch1→TX2)
/// is UNVERIFIED until the [wire] hardware session.
fn window_dbfs(bytes: &[u8]) -> [Option<i16>; 2] {
    let mut ch0 = Vec::with_capacity(bytes.len() / 4);
    let mut ch1 = Vec::with_capacity(bytes.len() / 4);
    for frame in bytes.as_chunks::<4>().0 {
        ch0.push(i16::from_le_bytes([frame[0], frame[1]]));
        ch1.push(i16::from_le_bytes([frame[2], frame[3]]));
    }
    if ch0.is_empty() {
        return [None, None];
    }
    [Some(dbfs_i16(&ch0)), Some(dbfs_i16(&ch1))]
}

/// Any [`StatusSource`] with the tap attached: the device round trips
/// delegate untouched to the inner source (holly-usb's certified ops);
/// the level reads come from the tap when it is live, falling back to
/// the inner source's own levels when it is not — which are the trait
/// defaults (`None` levels, no cadence) for `HollyControl`, so a dormant
/// tap is exactly the pre-tap behavior.
pub struct TappedSource<S: StatusSource> {
    inner: S,
    tap: Option<TapHandle>,
}

impl<S: StatusSource> TappedSource<S> {
    /// Attach the tap to `inner`: discovery first, a child process only
    /// when the dongle's UAC node exists (otherwise fully dormant).
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            tap: discover_target().and_then(|target| spawn_tap(&target)),
        }
    }
}

impl<S: StatusSource> StatusSource for TappedSource<S> {
    fn poll(&mut self) -> Result<HeartInfo, Error> {
        self.inner.poll()
    }

    fn set_noise(&mut self, on: bool) -> Result<(), Error> {
        self.inner.set_noise(on)
    }

    fn set_noise_level(&mut self, level: u8) -> Result<(), Error> {
        self.inner.set_noise_level(level)
    }

    fn set_eq(&mut self, pad: u8) -> Result<u8, Error> {
        self.inner.set_eq(pad)
    }

    fn set_gain(&mut self, stop: u8) -> Result<u8, Error> {
        self.inner.set_gain(stop)
    }

    fn set_reverb(&mut self, switch: bool, gear: u8) -> Result<(bool, u8), Error> {
        self.inner.set_reverb(switch, gear)
    }

    fn set_channel_mode(&mut self, value: u8) -> Result<u8, Error> {
        self.inner.set_channel_mode(value)
    }

    fn set_tx_mute(&mut self, tx: u8, muted: bool) -> Result<Option<bool>, Error> {
        self.inner.set_tx_mute(tx, muted)
    }

    fn set_shutdown_time(&mut self, value: u8) -> Result<u8, Error> {
        self.inner.set_shutdown_time(value)
    }

    fn set_indicator_light(&mut self, value: u8) -> Result<u8, Error> {
        self.inner.set_indicator_light(value)
    }

    fn set_mic_recognition(&mut self, value: u8) -> Result<u8, Error> {
        self.inner.set_mic_recognition(value)
    }

    fn tx_levels(&mut self) -> [Option<i16>; 2] {
        match &self.tap {
            Some(tap) => tap.state.levels(),
            None => self.inner.tx_levels(),
        }
    }

    fn level_interval(&self) -> Option<Duration> {
        match &self.tap {
            Some(tap) => tap
                .state
                .level_interval()
                .or_else(|| self.inner.level_interval()),
            None => self.inner.level_interval(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal inner source for the pass-through tests: the trait's 11
    /// round trips all fail loudly (never called here); only the default
    /// `tx_levels`/`level_interval` matter.
    #[derive(Debug)]
    struct InnerMock;

    impl StatusSource for InnerMock {
        fn poll(&mut self) -> Result<HeartInfo, Error> {
            Err(Error::NotFound)
        }
        fn set_noise(&mut self, _: bool) -> Result<(), Error> {
            Err(Error::NotFound)
        }
        fn set_noise_level(&mut self, _: u8) -> Result<(), Error> {
            Err(Error::NotFound)
        }
        fn set_eq(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
        fn set_gain(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
        fn set_reverb(&mut self, _: bool, _: u8) -> Result<(bool, u8), Error> {
            Err(Error::NotFound)
        }
        fn set_channel_mode(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
        fn set_tx_mute(&mut self, _: u8, _: bool) -> Result<Option<bool>, Error> {
            Err(Error::NotFound)
        }
        fn set_shutdown_time(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
        fn set_indicator_light(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
        fn set_mic_recognition(&mut self, _: u8) -> Result<u8, Error> {
            Err(Error::NotFound)
        }
    }

    /// The APK's own formula, vector-checked [code
    /// AudioRecordUtils.java:305-320]: silence sentinel, the 32767.0
    /// divisor at full scale, the half-scale square, and the −99 clamp
    /// for an inaudible-but-nonzero window.
    #[test]
    fn dbfs_is_the_apk_formula() {
        assert_eq!(dbfs_i16(&[0; 100]), -99);
        // Full-scale square wave: rms = 32767 → 20·log10(1) = 0.
        assert_eq!(dbfs_i16(&[32767; 100]), 0);
        // Half-scale square: 20·log10(0.5) ≈ −6.02 → (int) −6.
        assert_eq!(dbfs_i16(&[16383; 100]), -6);
        // One LSB in a 50 ms window: below −99 → clamped to the domain.
        let mut quiet = vec![0i16; 2400];
        quiet[0] = 1;
        assert_eq!(dbfs_i16(&quiet), -99);
    }

    /// S16LE interleaved parse: ch0 loud, ch1 silent in the same window.
    #[test]
    fn window_dbfs_splits_channels() {
        let mut bytes = Vec::with_capacity(WINDOW_BYTES);
        for _ in 0..2400 {
            bytes.extend_from_slice(&16383i16.to_le_bytes());
            bytes.extend_from_slice(&0i16.to_le_bytes());
        }
        let [l0, l1] = window_dbfs(&bytes);
        assert_eq!(l0, Some(-6));
        assert_eq!(l1, Some(-99));
    }

    /// The state machine behind the honesty rules: starts with no
    /// measurement, publishes windows, and a dead reader clears the
    /// levels AND the cadence — no fabricated stillness, no fake ticks.
    #[test]
    fn tap_state_lifecycle() {
        let state = TapState::new();
        assert_eq!(state.levels(), [None, None]);
        assert_eq!(state.level_interval(), Some(TAP_INTERVAL));
        *state.levels.lock().unwrap() = [Some(-12), Some(-16)];
        assert_eq!(state.levels(), [Some(-12), Some(-16)]);
        state.set_dead();
        assert_eq!(state.levels(), [None, None]);
        assert_eq!(state.level_interval(), None);
    }

    /// A dormant tap (no dongle node → no child) is a pure pass-through:
    /// levels and cadence both come from the inner source — exactly a
    /// bare `HollyControl`.
    #[test]
    fn dormant_tap_defers_to_inner() {
        let mut tapped = TappedSource::new(InnerMock);
        assert_eq!(tapped.tx_levels(), [None, None]);
        assert_eq!(tapped.level_interval(), None);
        assert!(matches!(tapped.poll(), Err(Error::NotFound)));
    }

    /// A live tap serves the reader's state and the APK cadence; a dead
    /// one reverts both to the inner source.
    #[test]
    fn live_tap_serves_tap_state() {
        let mut tapped = TappedSource {
            inner: InnerMock,
            tap: Some(TapHandle {
                state: Arc::new(TapState::new()),
                child: None,
            }),
        };
        assert_eq!(tapped.level_interval(), Some(TAP_INTERVAL));
        *tapped.tap.as_ref().unwrap().state.levels.lock().unwrap() = [Some(-12), Some(-16)];
        assert_eq!(tapped.tx_levels(), [Some(-12), Some(-16)]);
        tapped.tap.as_ref().unwrap().state.set_dead();
        assert_eq!(tapped.tx_levels(), [None, None]);
        assert_eq!(tapped.level_interval(), None);
    }
}
