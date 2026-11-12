# AGENTS.md

Standing rules for any agent working in this repo (Hollyland Lark A1 / 6108
reverse engineering, dongle VID:PID 3547:0407).

## Evidence discipline

- Every constant, byte and rule is tagged `[code]` (traced in the decompiled
  APK) or `[wire]` (confirmed in a file under `captures/`). Cite the doc
  section or capture file for every claim. Never guess a constant, field
  meaning, or byte mapping — an ungoverned value must not exist in the tree.
- Nothing from `re/` (decompiled APK sources) is ever committed.
- PC-side substitute evidence (Phase 1B) is tagged
  `[wire] PC-originated round trip` in `docs/protocol.md` rows.

## Layering (hard rule)

`holly-proto` (framing, no I/O) -> `holly-usb` (nusb transport) ->
`holly-app` (actor/ViewModel, std only) -> `holly-cli` (exactly 3 commands:
status, nc, watch) and `frontends/`. Frontends never touch nusb and never
hardcode device rules or strings.

## Device discipline

- Forbidden interfaces: IF1/IF2/IF3 (UAC audio + vendor bulk). Only IF0 is
  claimed.
- Forbidden commands: the OTA/reboot/pairing/reset family; speaker (37).
  Shutdown (39) is lifted for timer values only (owner, 2026-11-12): only
  the two documented timer values 0 = 15min, 1 = Never [code
  `HLDevice6302Activity.java:1668/:1673`], never an immediate power-off.
  TX mute (22) is lifted for per-transmitter mute on/off, TX1 first (owner,
  2026-11-12); TX2 stays gated until "22 lifted for TX2" — TX2 may not be
  powered during the run. Identify (35) is lifted for mic recognition
  on/off only (owner, 2026-11-12): only the two documented values
  OFF = 0, ON = 1 [code `HLDevice6302Activity.java:819-824`,
  `DeviceConnectVariable.java:652-654`], and any device run for it
  requires BOTH transmitters connected (heart offsets 0/1) before the
  first write [code `HLDevice6302Activity.java:805-812`].
- No cached device state: every SET derives from a fresh GET issued inside
  the same call.
- Captures are observation only (usbmon, airgap): we never inject frames
  into a capture session.
- Hardware runs need the owner at the machine: no device run without an
  explicit "go", one hardware session at a time, and no `holly`/GUI process
  may hold IF0 while the VM rig or a hardware test has the dongle.

## GUI changes

- GUI changes for a feature may add behaviour only: click and keyboard
  wiring, the pending state, and the ViewModel fields they read. They must
  NOT change style, layout, colour, spacing, sizes, typography, icons, or
  panel structure.
- Reuse the existing 0.5 dim for in-flight writes; never add a new look.
- If a requirement seems to need a visual change, stop and ask the owner.

## Repo map

Workspace: root = holly-proto, holly-usb, holly-cli, holly-app (`frontends/`
is its OWN workspace, excluded so root `cargo test` never compiles gpui).

- Build/test (root): `cargo fmt --check` · `cargo clippy --all-targets --
  -D warnings` · `cargo test` (150 pass, 12 ignored hardware).
- Build/test (gpui): in `frontends/gpui` — `cargo build` · `cargo clippy
  --all-targets -- -D warnings` · `cargo test` (13 unit) ·
  `cargo run -- --demo [connected|pending|busy]`.
- CLI: `cargo run -p holly-cli -- status|nc|watch` — exactly 3 commands, by rule.

Crates (layering: proto -> usb -> app -> cli/frontends):
- `holly-proto/` — HID framing only, zero deps: Frame, xor_checksum
  (tests/frames.rs, 20).
- `holly-usb/` — nusb transport + certified SET/GET ops: device.rs
  (HollyControl), transport.rs (NusbTransport + MockTransport), heart.rs
  (HeartInfo). tests/mock.rs (56, the guarded-round-trip replays) +
  tests/hardware.rs (12, ignored without a dongle).
- `holly-app/` — std-only actor + ViewModel: actor.rs (Core/DeviceHandle/Hub,
  one coalescing write bucket), vm.rs (ViewModel, WRITABLE table, *_wire
  translations + labels), demo.rs (DemoDevice, --demo scenarios).
  tests/actor.rs (19).
- `holly-cli/` — clap shell over holly-usb (status / nc / watch).
- `frontends/gpui/` — gpui 0.2.2 window over holly-app only (no nusb):
  src/main.rs (entry, AppModel, all panels), theme.rs (tokens),
  fonts.rs (bundled Hanken Grotesk); assets/fonts (sfnt); patches/
  (vendored nusb + xattr, sha256-pinned version-conflict fixes); README.md
  (token map + unfinished reproductions).

Evidence:
- docs/protocol.md — the byte table, every row [code]/[wire]-tagged;
  set-gate-checklist.md — per-control SET gates; capture.md — capture
  workflow + route-(a) rig end state (FROZEN); device.md, gui-roadmap.md,
  set-capture-plan.md; design/ — index.html px mock + tokens + screens.
- captures/: `2026-11-05-*.txt` raw text exports; `set_<control>_roundtrip
  .jsonl` exported guarded round trips (tracked, cited by docs AND tests —
  paths must not move); `captures/raw/*.pcap` local originals (ignored).
- inputs/ (lsusb -v, HID descriptor), packaging/70-hollyland.rules (udev).

Local-only, NEVER commit: `re/` (decompiled APK), `*.xapk`,
`android-x86-9.0-r2.iso`, `tools/*.qcow2`, `captures/raw/`, `target/`,
`frontends/gpui/target/`, `.build/`. Untracked owner files (not ignored
yet): `features/`, `holly-design-ref.zip`, `holly-feature-prompts.zip`,
`Holly theme.html`, `tools/capture_read_map.py`,
`captures/set_shutdown_time_roundtrip.jsonl`.

## Git

- Do not commit anything under `docs/` without owner approval (this file is
  excepted). Doc changes go to the working tree; show diffs instead; never
  stage or revert pre-existing owner edits.
- Small, one-concern commits in the `git log` style: lowercase
  `type(scope): detail — extras`.
- Surgical edits only; no drive-by refactors; no new dependencies without
  asking.
