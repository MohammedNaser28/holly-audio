# holly-gpui — desktop frontend

`gpui` shell over [`holly-app`](../../holly-app). This tree contains **no device
knowledge**: it consumes `holly_app::vm::view_model(Snapshot) -> ViewModel` and
tells `DeviceHandle` what the user asked for. It never imports `holly-usb`,
never enumerates/claims a USB interface, and never streams audio — IF1-IF3 are
untouched by construction (UI talks only to the actor over channels).

## Build & run

```sh
cargo run                          # real hardware (dongle 3547:0407 on IF0)
cargo run -- --demo                # connected demo (same as --demo connected)
cargo run -- --demo pending        # every SET stalls 20 s: photograph in-flight UI
cargo run -- --demo busy           # OS busy error: the busy banner, no claim
cargo test                         # 13 unit tests, no GPUI runtime/hardware needed
```

`--demo [connected|pending|busy]` parses via `DemoScenario::from_word`
(`holly-app/src/demo.rs`); a bare flag = `connected`. An unknown word or any
unrecognized argument is a usage error (`exit(2)`), never a silent fallback to
a session the user did not pick.

This is its own workspace (`[workspace]` here, excluded from the root via
`exclude = ["frontends"]`), so the root crate set never compiles the gpui
toolchain.

Build/run of a real claim requires the udev grant
(`packaging/70-hollyland.rules`) and a free IF0 — if anything else
(`holly-cli status|nc|watch`, a second Holly instance) holds the claim, the UI
shows the Busy banner instead of the dashboard.

## Dependency pinning (why 0.2.2, and the patches)

- `gpui = "0.2.2"` (crates.io, feature-complete stable release, Linux
  wayland+x11 backends, blade-graphics/Vulkan render path). Pinned to the
  existing `Cargo.lock`; no unpinned git deps (project rule).
- The resolution conflict with gpui 0.2.2 — `nusb 0.2.9` (macOS target
  `core-foundation = "0.10.1"` vs gpui's `=0.10.0`; fails graph-wide before
  any macOS code compiles) and `xattr 0.2.3` (`libc::ENOATTR` on
  linux/android where modern libc doesn't define that errno — reached via
  `gpui → gpui_util → zed-async-tar`) — is resolved by **vendored patches**
  in [`patches/`](patches/) (sha256s recorded next to each; each is the
  crate verbatim plus one minimal change). See [`Cargo.toml`](Cargo.toml)
  for the fetched-at date and both sources' version lines.
- gpui API discipline: **every** gpui call in `src/main.rs` was verified
  against the vendored 0.2.2 sources (`~/.cargo/registry/src/.../gpui-0.2.2/`,
  citations with file:line inline in the module docs and comments) — nothing
  from memory.

## Architecture

```text
holly-app actor thread ──Snapshot──▶ hub subscriber (pump thread)
        ▲                                   │ forwards into
        │ commands (set_noise /set_noise_level /set_eq /set_gain /refresh) ▼
   DeviceHandle                      gpui executor (cx.spawn → Entity::update)
```

- `AppModel` holds a `ViewModel` + the `DeviceHandle`; the pump thread bridges
  the blocking `Receiver<Snapshot>` into gpui's main-thread executor
  (`AsyncApp` is `!Send`, documented in `src/main.rs`).
- Rendering is snapshot-driven: any state change the actor emits (1 s poll,
  change-only) replaces the VM and repaints. UI state is *only ever* snapshot
  state — never optimistic: a click sends a command and the actor-reported
  **pending** field becomes the UI's only truth until a fresh snapshot
  confirms the device's verdict (or the last confirmed value stays on
  failure).
- CSD: `TitlebarOptions { title, appears_transparent }` + `WindowDecorations::
  Client` (the xdg title survives for taskbars, the chrome is painted);
  drag = `Window::start_window_move()` on the titlebar strip, close =
  `Window::remove_window()` (gpui 0.2.2 has no `window_control_area`).

## Controls and where their truth comes from

| Control | Displayed by | Click/key | Write gate |
|---|---|---|---|
| NC rocker (On/Off pads + Space/Enter) | verified `nc.state` only | click on a pad, or Space/Enter while the well is focused (`key_context "nc_rocker"`) | `nc_write_accepted`: nothing in flight, state verified; a pad requests THAT pad's state and dedups on the verified state |
| NC level (Weak/Medium/Strong = `nc_level_label`) | verified `nc.state == some(true) && nc.level` — no pending target ever shown as selected | click a row (`named_usize("nc-level", L)`), or keys 1/2/3 | `level_pick`: blocked while ANY SET is in flight, blocked while the switch is not verified on, noop on the device's own level |
| EQ selector (Equalization/Low/Bright = display order) | verified `eq.selection` only (wire bytes 3/1/2 via `vm::eq_selection`; no pending target ever shown as selected) | click a pad (`named_usize("eq-pad", N)`), or Left/Right while the well is focused (`key_context "eq_selector"`) | `eq_pick` + `vm::eq_pad_wire`: blocked while an EQ write is in flight or `WRITABLE.eq` is false, noop on the device's own pad; the wire byte comes from the VM translation — never a literal here |
| Volume selector (stops "1".."6" = display order) | verified `volume.selected` only (the §5.2 read space 0..=5 via `vm::volume_selection`; no pending target ever shown as selected) | click a stop (`named_usize("volume-stop", N)`), or Left/Right while the well is focused (`key_context "volume_selector"`) | `volume_pick` + `vm::volume_pad_wire`: blocked while a gain write is in flight or `WRITABLE.volume` is false, noop on the device's own stop; the wire byte comes from the VM translation — never a literal here |
| Mic mute chips, channel/schedule selectors, reverb size + toggle, mic-recognition + indicator toggles | read-only snaps of the VM's known fields; unknown = no selection, full opacity | none | nothing is sent — every one of these is `WRITABLE.*: false` until a §1-gated capture exists (`docs/gui-roadmap.md`) |
| Refresh ("Try again") | — | banner button | `DeviceHandle::refresh()` (actor re-open attempt) |

Pending semantics (single rule, applied by `nc_write_accepted`/`level_pick`/
`eq_write_accepted`/`volume_write_accepted`): ANY SET in flight blocks its OWN
control family only (`PendingWrite::Noise` dims the NC controls,
`PendingWrite::Eq` dims the EQ selector, `PendingWrite::Gain` dims the Volume
selector — never two at once): the affected controls ignore picks, the
family's status word reads "Applying", the lit pad renders the tungsten
gradient at 40% element opacity with no glow, and the family's selector dims
to 0.5. Nothing greys-out for "unknown": a state the wire never reported
displays as-is.

## Theme: graphite and tungsten

Tokens live in `src/theme.rs` (0xRRGGBB, no gpui dependency there); fonts
(Hanken Grotesk 300/400/500/600, converted to plain sfnt) register in
`src/fonts.rs` before the first window. Reference contract:
`docs/design/index.html` (px-exact mock) + `docs/design/screens/*.png`.

Reference hex → token mapping (the token set in theme.rs is closed; reference
values without their own token map to the NEAREST token, documented here so
"no guessed mappings" stays auditable):

| Reference value | Token | Where the reference uses it |
|---|---|---|
| `#0F1114` | `CHASSIS` | window/`body` ground |
| `#15181C` | `CHASSIS_TOP` | window gradient top |
| `#0A0B0D` | `WELL` | recessed wells (rocker, selector, mic badge) |
| `#08090A` | — (reuse `WELL`) | page bg in the mock; inside the window this is WELL's shadow edge |
| `#1C1F24`→`#15171B` | `PANEL_TOP`/`PANEL_BOTTOM` | faceplate gradient |
| `#262A30` | `RAISED` / `LINE_PANEL` | buttons, unlit segments; panel border |
| `#2B2F36`→`#22252B` | `RAISED_TOP`/`RAISED_BOTTOM` | matte pad/button gradient |
| `#3A3F47` / `#343941` | `LINE` | hard button borders (nearest token) |
| `#22262C` | `LINE_SOFT` | titlebar/footer rules |
| `#ECEEF1` | `TEXT` | primary text, lit battery segments, mic glyph (live) |
| `#A3AAB4` | `TEXT_2` | secondary text, level captions, unselected options |
| `#7F8793` | `TEXT_3` | dim mic glyph, footer micro-copy |
| `#4A5059` | `TEXT_DISABLED` | dead digits (`--`), decorative ring |
| `#000` (`0 1px 0`/`0 2px 0` under-edges) | `WELL` | hard matte under-edges |
| `#FFB547` | `TUNGSTEN` | engagement/attention (LED, indicator bars, DEMO chip) |
| `#FFC66E`→`#F5A524` | `TUNGSTEN_TOP`/`TUNGSTEN_BOTTOM` | lit pad gradient |
| `#1A1205` | `INK` | text on tungsten |
| `#5FD38D` | `LIVE` | green LED (live connection) |
| `#FF6B6B` | `ALERT` | error (banner accent + error LED) |
| `#221C12`/`#3A2E18`/`#C9B38A` | `BANNER_BG`/`BANNER_LINE`/`BANNER_TEXT` | banner surface set |
| `rgba(0,0,0,.5)` | `DROP_DARK` | panel bottom edge |

Window geometry: 1080×760 (also the minimum), radius 12; titlebar 44, footer
32; NC/pending flows live entirely in `nc_panel`/`nc_selector`.

## Unfinished reproductions (what cannot be painted today)

All deferred with a reason; none invented:

- **Inset shadows.** The reference's `inset 0 2px .. rgba(0,0,0,.8)` well
  shading and `inset 0 1px 0 rgba(255,255,255,..)` top highlights have no
  gpui counterpart (outset `BoxShadow` only). Wells render as flat fills;
  theme.rs keeps the `HIGHLIGHT_*` numbers as designed data with a
  `dead_code` allow carrying that reason.
- **Meter segments (source).** The meter now RENDERS the mock's lit recipe
  (TEXT_2 segments, TUNGSTEN from 0-based index 17 — the APK's own
  18..=20 distortion-report zone [code BleSingleVolumeView.java:setVolume]
  matching index.html:230) from the VM's per-mic dBFS level. The dB→segments
  curve is the APK's exact `getVolumeProgress2` [code], never a frontend
  constant (vm.rs `meter_lit`/`meter_lit_for`; offline/muted/no-source
  cards show zero). What stays unfinished is the LEVEL SOURCE: the wire
  has no level field, so real hardware reads `None` and the meter stays
  unlit; `--demo` scripts a −16..0 dBFS sweep (DEMO marker rule). The real
  source is the OS-audio tap — PipeWire/ALSA capture of the dongle's UAC
  input, the desktop twin of the APK's own path
  [code HLDevice6302Activity.java:476 → AudioRecordUtils] — parked until
  the owner authorizes it (`pw-record` subprocess design agreed
  2026-11-12).
- **Battery segments.** The mock lights ALL five battery segments whenever
  the link is live, regardless of the percent shown next to it
  (index.html:229). The desktop bar lights `battery_segment_count(pct)`
  segments (~20 % each, ceil) instead, so the bar agrees with the digits —
  a documented deviation, deliberate (vm.rs marks the thresholds
  presentation-only; 93 % lights 5, 80 % lights 4, 20 % lights 1, a dead
  link stays fully unlit).
- **Busy-mode global 0.6 dim.** The mock dims the WHOLE page while busy. The
  design brief says unknown/recovering states must not look disabled —
  banners already say what is wrong, so the dim is NOT applied (this is a
  documented deviation, deliberate).
- **Logo.** The reference's 18×18 SVG has a stroke ring and a filled dot in
  DIFFERENT colors while `svg()` paints a single color through one alpha
  mask; the logo is composed from div primitives (16px ring, 1.2px stroke,
  6px dot) proportional to the reference's viewBox math.
- **Ring LED.** Reference draws `1.5px solid #5A616B`; gpui helpers accept
  integer-ish pixel widths, we draw 1px `TEXT_DISABLED` (#5A616B →
  `TEXT_DISABLED`, nearest token).
- **LED glow alpha on ALERT.** The alert LED keeps the LED-glow strength of
  the designed glows (0.70) on the ALERT color — a composition, documented
  here and inline.

## Reused assets

- **Fonts**: the reference's `hanken-grotesk-latin-{300,400,500,600}-normal.woff2`
  files, converted once to plain sfnt (ttf) with system fontTools — gpui's
  Linux font registry feeds bytes into fontdb/cosmic-text, which parse sfnt
  only (same family/weight axes, verified digit advances, `fonts.rs`).
- **Icons**: the mock's inline SVG paths (mic, charger, close) embedded as
  consts in `src/main.rs`, loaded through `AssetSource` keyed by the source
  itself (no filesystem, no new dependency; `svg()` paints one color).
- **Nothing else** is fetched: all colors/geometry are tokens + code; the
  design tooling artifacts (`docs/design/*`) are the reference, not runtime
  assets.

## Demo mode (`--demo`)

The SAME actor (`holly-app`) runs over `DemoDevice` — a `StatusSource`
standing in for the dongle, seeded from the certified session-3 capture (NC
on, level 1, tx2 93 %). No USB is opened. Every demo variant stays visibly
marked: window title `HOLLY — DEMO`, a titlebar chip, footer `· demo`. Demo
data is never passable as hardware status.

- `connected` — SETs apply instantly; the full dashboard.
- `pending` — every SET stalls 20 s: in-flight pads/selector, "Applying".
- `busy` — open attempts fail with the real OS busy error shape
  (`ResourceBusy, errno 16`), classified exactly like a held dongle: the busy
  banner, "Try again" does not succeed either.

Demo writes change ONLY the scripted in-memory heart — `holly-cli status` (or
any real-mode instance) reads the dongle and will never agree with a demo
window; compare demo windows with nothing, compare `status` only with real
sessions, sequentially (the claim is single-holder by design).

## Keybinds

| Key | Action | Where it lives |
|---|---|---|
| Space / Enter | flip the NC switch (target computed from the verified state — equivalent to clicking the OPPOSITE pad) | only while the rocker well is focused (`nc_rocker` context) |
| `1` / `2` / `3` | NC level Weak / Medium / Strong | global |
| Left / Right | step the EQ selection one pad (from the device's verified pad; no verified pad = noop) | only while the EQ well is focused (`eq_selector` context) |
| Left / Right | step the Volume selection one stop (from the device's verified stop; no verified stop = noop) | only while the Volume well is focused (`volume_selector` context — disjoint from the EQ well's identical keys) |
| ctrl-r | refresh (same as "Try again") | global |
| ctrl-q | quit | global, app-level action |
| Mouse drag (titlebar strip) | move window | CSD: `start_window_move` |
| Close button (titlebar) | close window | CSD: `remove_window` (last window quits the app) |

Global keys reach the root's `on_action` listeners when nothing holds focus
(gpui dispatch falls back to the root node); while a SET is pending the
flip/level decisions ignore the request — the keys carry the same gates as
the mouse.

## Known issue: in niri the window only works once it is made WIDE

**Observed 2026-11-10** (niri scrollable tiling, gpui 0.2.2 wayland CSD
window, both `--demo` and real-device runs):

- Launched as a normal-sized tile, the window shows its FIRST frame (usually
  the `No dongle (3547:0407) on the bus` banner) and then **stops reacting
  visually**: clicks and keys do nothing on screen. The clicks still reach
  the actor — the terminal log keeps printing the SETs/pendings and the
  DEVICE state actually changes (e.g. NC really went off) — only the
  pixels freeze, so it both *looks* broken and *looks* like the dongle is
  not being detected (the stale first banner is the "no dongle" text, while
  the actor behind it is Connected).
- Making the window WIDE — maximize the column, or drag-resize it — makes
  everything work immediately: the connected dashboard paints, and from
  then on clicks/keys/EQ writes repaint normally. The observed session
  contract: **wide screen in niri = working app; tile-sized = not
  responding.**
- NOT a device issue: dongle detection/connection is actor-side and
  independent of window size. No dongle should ever be "detected more"
  because the window is wider — the frozen banner only misleads.
- Working theory (not yet verified): the stall is in the gpui 0.2.2
  Linux/Wayland redraw path for CSD client-decorations windows — the app
  fires `cx.notify()` every second regardless (the VM's `updated` timestamp
  changes on every poll), yet niri keeps showing the old frame until IT
  forces one via resize. That means the paint decisions in our code are
  fine and the missing piece sits below our layer: frame callbacks /
  `request_frame`-on-wayland, possibly niri-specific (scrollable tiling +
  `WindowDecorations::Client`). An X11/Wayland A/B test and a gpui-upstream
  minimal case would settle it; our `patches/` vendored copy is where any
  fix would land.
- Workaround for the owner: maximize the holly column (or drag it wide)
  once after each launch. If a window ever looks frozen again, any
  resize/focus toggle restores painting without losing state — actions
  taken while frozen DID take effect on the device; quitting (ctrl-q) and
  relaunching repaints the true state.

## Current limitations

- NC (switch + level, cmd 19), EQ (cmd 7), and Volume (cmd 3) are writable —
  wire-exercised (captures/2026-11-05-noise-level-medium.txt;
  captures/set_eq_roundtrip.jsonl + captures/set_gain_roundtrip.jsonl;
  docs/protocol.md). Channel, schedule, reverb, mute, mic-recognition, and
  the indicator light are rendered but read-only until their per-control
  capture procedures ([`../../docs/gui-roadmap.md`](../../docs/gui-roadmap.md))
  pass the §1 gate.
- **niri repaint stall** (see the known issue above): until traced, keep
  the window widened after launch.
- Linux only (wayland/x11 via gpui 0.2.2). No settings persistence, no tray.
- The meter is always-unlit and inset shadows are absent — see "Unfinished
  reproductions".

## Extension points

- New read-only fields: extend `HeartInfo` + `ViewModel`, then render — no UI
  device logic (same pattern as the current panels).
- Per-control writes after a gated capture: flip the menu's `WRITABLE.*`
  bit, attach a `cx.listener`, extend the actor command surface — the view
  code already isolates every write behind a pixel-pure decision fn.
- EP 0x81 unsolicited-push reader: drops in behind `holly-app`'s
  `StatusSource` trait; the actor/UI contract is unchanged.
- Moving this frontend out of the repo (`git` dependency): already prepared by
  keeping it isolated in its own workspace with a path-only `holly-app` dep.
