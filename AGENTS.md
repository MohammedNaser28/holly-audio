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
- Forbidden commands: the OTA/reboot/pairing/reset family; identify (35),
  speaker (37). Shutdown (39) is lifted for timer values only (owner,
  2026-11-12): only the two documented timer values 0 = 15min, 1 = Never
  [code `HLDevice6302Activity.java:1668/:1673`], never an immediate
  power-off. TX mute (22) is lifted for per-transmitter mute on/off, TX1
  first (owner, 2026-11-12); TX2 stays gated until "22 lifted for TX2" —
  TX2 may not be powered during the run.
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

## Git

- Do not commit anything under `docs/` without owner approval (this file is
  excepted). Doc changes go to the working tree; show diffs instead; never
  stage or revert pre-existing owner edits.
- Small, one-concern commits in the `git log` style: lowercase
  `type(scope): detail — extras`.
- Surgical edits only; no drive-by refactors; no new dependencies without
  asking.
