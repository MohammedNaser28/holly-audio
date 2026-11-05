# holly-gpui — desktop frontend

`gpui` shell over [`holly-app`](../../holly-app). This tree contains **no device
knowledge**: it consumes `holly_app::vm::view_model(Snapshot) -> ViewModel` and
tells `DeviceHandle` what the user asked for. It never imports `holly-usb`,
never enumerates/claims a USB interface, and never streams audio — IF1-IF3 are
untouched by construction (UI talks only to the actor over channels).

## Build & run

```sh
cargo run                  # real hardware (dongle 3547:0407 on IF0)
cargo run -- --demo        # scripted fake device, zero USB, clearly marked
cargo test                 # 5 unit tests, no GPUI runtime/hardware needed
```

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
  examples cited in the module docs) — nothing from memory.

## Architecture

```text
holly-app actor thread ──Snapshot──▶ hub subscriber (pump thread)
        ▲                                   │ forwards into
        │ commands (set_noise /refresh)      ▼ futures unbounded channel
   DeviceHandle                      gpui executor (cx.spawn → Entity::update)
```

- `AppModel` holds a `ViewModel` + the `DeviceHandle`; the pump thread bridges
  the blocking `Receiver<Snapshot>` into gpui's main-thread executor
  (`AsyncApp` is `!Send`, documented in `src/main.rs`).
- Rendering is snapshot-driven: any state change the actor emits (1 s poll,
  change-only) replaces the VM and repaints. UI state is *only ever* snapshot
  state — never optimistic: a click sends a command and the row immediately
  shows the actor-reported **pending** class until a fresh snapshot confirms
  the device's verdict (or keeps the last confirmed value on failure).
- Interactive controls: NC switch toggle (space / click) and NC level buttons
  (1/2/3 / click). Their decision logic is pure and unit-tested
  (`level_button`, `toggle_target`).

## Theme: Signal

Dark, compact, muted-blue accent; tokens centralized in `src/main.rs`
(components carry no hex values).

| Token | Hex | Used for |
|---|---|---|
| background | `#111318` | window |
| surface | `#191D25` | recessed rows/buttons, banner body |
| elevated | `#222833` | cards (NC toggle, TX) |
| border | `#303744` | borders, inactive banner line |
| text | `#E8EBF0` | primary |
| muted | `#929BAA` | hints, metadata, no-device badge |
| accent | `#83A9E8` | interactive emphasis, active level button, "connecting" |
| success | `#65B891` | connected badge, healthy battery |
| warning | `#E5B765` | pending SET, low battery, busy/permission banner, DEMO chip |
| error | `#DF7777` | error banner, critical battery |

Window: 760×620. Header: `HOLLY` + "Wireless Audio Controller" + connection
badge. Footer: `state received HH:MM:SS (UTC)` + crate version. The stamp is
actor-side metadata (`Snapshot.received_at`), change-gated: it advances when
the device confirmed something new, not once per identical poll — the footer
never invents freshness (and shows `—` before the first successful GET).

## Demo mode (`--demo`)

`holly_app::demo::start_demo()` runs the REAL actor state machine over
`DemoDevice` (a `StatusSource` standing in for the dongle, seeded from the
certified session-3 capture: NC on, medium, tx2 93 %, gain 3/eq 3/reverb off).
No USB is opened. The window title (`HOLLY — DEMO`), a header chip, and the
footer prefix all mark it; demo data is never passable as hardware status.

## Current limitations

- Only NC (switch + level, cmd 19) is writable — wire-exercised in
  `captures/2026-11-05-noise-level-medium.txt` + `docs/protocol.md` §4. Voice
  gain / EQ / reverb are READ ONLY rows until their per-control capture
  procedures ([`../../docs/gui-roadmap.md`](../../docs/gui-roadmap.md) §2) are
  executed on the wire; no control exists for them by design.
- The footer time reflects the last *confirmed* state change; a fully stable
  device keeps one stamp (honest: the displayed verdict was received then).
- Linux only (wayland/x11 via gpui 0.2.2). No settings persistence, no tray.

## Extension points

- New read-only fields: extend `HeartInfo` + `ViewModel`, then render — no UI
  device logic (same pattern as the current rows).
- EP 0x81 unsolicited-push reader: drops in behind `holly-app`'s
  `StatusSource` trait; the actor/UI contract is unchanged.
- Moving this frontend out of the repo (`git` dependency): already prepared by
  keeping it isolated in its own workspace with a path-only `holly-app` dep.
