# Capture workflow — sniffing HollyAudio's traffic to the dongle

Goal: record the exact USB transfers the Android app "HollyAudio" issues for one
action per capture file, so every byte in `docs/protocol.md` traces to a real frame.

Verified device on this laptop (source: `inputs/lsusb-v.txt`):

```
3547:0407  Shenzhen Hollyland Technology Co.,Ltd Wireless Microphone
Bus 006, Device 006   (addresses change on replug — re-check with lsusb)
```

Control-channel candidates (see `docs/device.md`): IF0 HID (EP 0x81 IN / 0x06 OUT int)
and IF3 vendor bulk (EP 0x82 IN / 0x05 OUT bulk). We capture **both** and let the
diffs tell us which one the app actually uses. Do not pre-filter interfaces.

## One-time setup (Arch)

```sh
sudo pacman -S --needed qemu-desktop
```

## Android environment: QEMU + Android-x86 (primary)

Waydroid cannot hand a general USB host device (our dongle) to the container —
the guest has no USB host stack for it. Use a QEMU VM instead: the dongle is
passed through with `-device usb-host,vendorid=0x3547,productid=0x0407`, and
since the guest's USB traffic flows through the **host** kernel (QEMU emits it
via usbfs), host-side `usbmon` still captures everything.

ABI warning: HollyAudio 3.1.0 ships **armeabi-v7a only** — use an image with
libhoudini ARM translation (Android-x86 / Prime OS, not modern Bliss).

Setup (one-time):

```sh
sudo pacman -S --needed qemu-desktop
# download an Android-x86 ISO with houdini, e.g. from android-x86.org/download
tools/qemu-android.sh install -i ~/Downloads/android-x86.iso   # boots the ISO installer
tools/qemu-android.sh run                                      # next boots, from disk
adb connect 127.0.0.1:5555                                     # sideload the APK from host:
adb install /tmp/opencode/xapk/com.hollyland.larkc1.apk         # path created during RE; keep a copy of the xapk in the project dir as input
```

## Per-capture procedure

The bus/device numbers change on replug — always re-check first:

```sh
lsusb | grep -i hollyland      # → Bus 006 Device 006: ID 3547:0407 ...
```

1. Boot the VM (dongle plugged in):

   ```sh
   tools/qemu-android.sh run       # or .../install -i <iso> on first boot
   adb connect 127.0.0.1:5555      # optional; lets you control the app via scrcpy/adb
   ```

2. Start the capture (root needed for usbmon):

   ```sh
   sudo tools/capture.sh -b 6 -d 6 -t 25 -o captures/raw/<action>.pcap
   ```

   Repeat **one action per file**, with ~3 s of idle before/after the action so
   periodic frames can be separated from the action's frames.

3. While it runs, in the **QEMU VM session** drive **exactly one** HollyAudio action.

4. Export raw payload rows and eyeball the first frames:

   ```sh
   python3 tools/pcap_to_frames.py captures/raw/<action>.pcap --bus 6 --device 6 \
       -o captures/<action>.jsonl
   ```

## Action list (one file each)

| name | action | expected side |
|---|---|---|
| `idle` | nothing, 30 s | baseline polling |
| `pair` | pair/begin-pairing flow in app | ~10 s window |
| `unpair` | unpair a mic | |
| `gain-up` | one gain + step on ch1 | |
| `gain-down` | one gain − step, same channel | |
| `nc-on` | enable noise cancellation | |
| `nc-off` | disable noise cancellation | |
| `monitor-on` | enable ear-monitor | |
| `monitor-off` | disable ear-monitor | |
| `mute-mic` | mute a mic in app | |
| `unmute-mic` | unmute mic | |
| `status-open` | open the app's main status screen after fresh replug | initial status dump |

File naming: exactly these names in `captures/` — the JSONL export is what gets
referenced in `docs/protocol.md`, raw `.pcap` stays in `captures/raw/`.

## Notes

- usbmon shows host-side view of both control-channel candidates, so one capture
  per action is enough — we diff interfaces afterwards.
- If a needed app action can't be triggered in QEMU/Android-x86 (houdini
  translation failing on some lib, permission crash), the alternative is a
  real rooted phone + OTG — but then usbmon capture on the host won't see
  anything; you'd capture on the phone instead. Prefer fixing the VM first.
- Do NOT use `usbpcap` on the Windows host for the Android-side traffic — we need
  Linux captures the Python expander can parse.
- Root-only airgap discipline: capture hosts' `usbmon` reads are read-only; the app
  writes frames its own way, we never inject frames.

## Rig status (2026-11-06): the installed image cannot run HollyAudio 3.1.0

Verified live, evidence-only (VM run, adb session, ISO inventory):

- VM boots fine installed (`tools/android-x86.qcow2`, Android-x86 9.0-r2 from
  `android-x86-9.0-r2.iso` in the repo root; KVM, dongle passthrough and adb
  `127.0.0.1:5555` all work).
- HollyAudio (`com.hollyland.larkc1`) is installed but crashes on every
  launch, killed in `BaseApplication.onCreate`:
  `java.lang.UnsatisfiedLinkError: dlopen failed: ".../lib/arm/
  libc++_shared.so" has unexpected e_machine: 40 (EM_ARM)` — the ARM ELF is
  handed to the x86 linker; no translation layer ever engages.
- Root cause is in the shipped image itself: `ro.product.cpu.abilist`
  *claims* `x86,armeabi-v7a,armeabi` and `ro.dalvik.vm.native.bridge=libnb.so`,
  but inside the ISO's `system.sfs/system.img`: `/system/lib/libhoudini.so`
  is a **0-byte placeholder**, `/system/lib/arm/` is an **empty directory**,
  `libnb.so` is 10,660 bytes, and `/etc/init.sh` has zero houdini/ARM hooks
  (debugfs listings 2026-11-06). The r2 ISO ships native-bridge plumbing
  without the translation runtime — exactly the failure the ABI warning
  above predicts, so the "use an image with houdini" condition is NOT met
  and must be for this rig to satisfy §1 step 2.
- Unsharp fix (awaiting owner decision): install a houdini-carrying
  Android-x86/Prime-OS/Bliss variant to a fresh disk (rewrite of the `-i`
  flow: already supported by `qemu-android.sh install -i <iso>`), or
  retrofit the houdini blobs (`/system/lib/arm/*`, real `libhoudini.so`,
  `.native.bridge=libhoudini.so`) into the current image. Until then the
  app cannot reach the dongle in the guest and no app-side SET capture is
  possible. Guest network is fine (10.0.2.2 pings), so an in-guest blob
  fetch would work once a toggle/source exists.

## Rig fix source (2026-11-06): Bliss 11.13 official ISO on a fresh disk

- Source: archive.org item `ds-store_20220731`, file
  `BlissOS/BlissISO/BlissV11.3.iso` (1,706,033,152 B). Uploader
  `bluecrow1387@gmail.com` = the Bliss OS maintainer.
- Identity cross-check: the file's archive.org metadata sha1
  `df0d4ca5a6391c81056ab41f8f72a23146097186` equals the sha1 of the ISO in
  the dedicated official item
  `bliss-v-11.13-official-20201113-1525-x-86-64-k-k-4.19.122-ax-86-ga-rmi-m-20.1.0-`
  — same bytes, so this is the official Bliss v11.13 2020-11-13 build
  (Android 9 / API 28, `pie` tagged).
- Local verification after download: sha1 match as above; **sha256
  `8be22c015664f9b4d5209dcf81ae3e7735e727fb14599ccc31b08b7d61f83064`**
  (computed in-pass by the downloader, no extra CPU pass).
- Fallback on file: official item
  `bliss-v11.14-official-20210508-1013_x86_64_k-k4.19.122-ax86-ga-rmi_m-20.1.10_pie`
  (1,546 MB, metadata sha1 `ded04da42d64…`, Android 9 "pie" too).
- Retrofit-into-r2 attempt (same day, superseded): pushed Bliss's real
  houdini 8.0.0_y.49374 blobs into `/system/lib{,/arm}` of the r2 guest
  (197 files, perms+restorecon+`persist.sys.nativebridge=1`); bridge
  engaged (`houdini: Initialize library ... successfully`) but the app
  process died with a houdini-internal `Fatal error (ID:0x019001d0)` →
  SIGSEGV (libsigchain log evidence) before app init. Interim conclusion:
  blob-only retrofit is version-fragile; owner ordered the clean install
  path (option 2). Old disk preserved as `tools/android-x86-r2.qcow2`.

## Phase 1 end state (2026-11-09): Bliss 11.14 frozen after the houdini swap SIGBUS — Phase 1B elected

Per-session snapshots: the console helpers + net recipe now live in `tools/`
(`qmon.py`, `qsnap.py`, `qmon2.py`, `guest-net.sh`). Boot-recipe persistence:
**no** userinit hook exists in this ROM (grep of `/system/etc` empty), so each
boot needs exactly one console line: `sh /data/local/guest-net.sh` (the script
is already at `/data/local/guest-net.sh` on the disk; prints `200 0 success`
×2 + `NET-OK` and starts adbd). Boot chain of record tonight, all live-only:

1. Net recipe applied, adb auth read from disk `adb_keys`, transport `device`.
   App present with all three splits (`pm path`: base + `armeabi_v7a` + `mdpi`;
   the streamed session install survived every restart).
2. Launch (`monkey`) → **FATAL in `BaseApplication.onCreate`**
   (`LogUtil.initXLog` → `System.loadLibrary("c++_shared")` →
   `UnsatisfiedLinkError ... e_machine: 40 (EM_ARM)`), and immediately above
   it: `houdini: Initialize library(version: 9.0.5c_y.51331 RELEASE)...
   failed(0x00010002)`. Tree/props verified good (real 5.9 MB libhoudini.so,
   207 arm + 204 arm64 files, `zygote64_32`, `isa.arm=x86`,
   `isa.arm64=x86_64`) — the 9.0.5c bridge refuses to initialize on this
   build for reasons that stayed undiagnosed inside the time box.
3. Resilience attempt #1 (the only one): swap the whole blob set to the
   proven 8.0.0_y.49374 from the 11.13 image (hot-attached
   `tools/android-x86.qcow2` → its `bliss-x86-11.13/system.img` loop-mounted
   ro; `/system` is already rw here). Backups first:
   `/data/local/tmp/houdini905c-32.tgz` / `houdini905c-64.tgz`; replaced
   `lib/libhoudini.so` + `lib/arm` (158 files) + `lib64/libhoudini.so` +
   `lib64/arm64` + `libvixl-arm.so`. **APK untouched.**
4. Launch after swap → **SIGBUS (BUS_ADRERR) inside
   `android::InitializeNativeBridge(+361)` on the app_process32 fork path**.
   `tombstone_03`: fault addr inside a mapping of `/system/lib/libhoudini.so`
   at file offset `0x554000` — an offset that only exists in the PRE-swap
   9.0.5c ELF (the 8.0.0 file ends at 0x4920F0). Cause: `cp -f` rewrote the
   same inode the live zygote had mmap'ed (old ELF headers), so the forked
   child mapped past end-of-file → BUS_ADRERR. Lesson: an in-place
   same-inode library overwrite is not a swap; library swaps on a live
   system need a boot boundary (or rename-based inode replacement).
5. Guest rebooted cleanly (~23:13) to re-map under fresh processes, and the
   net recipe was re-applied by the one-liner. Re-launch was fired, but the
   result was never observed: an `adb root` restart wedged the transport
   (`device offline`), and the subsequent recovery keystrokes were not
   verified on the console (VT had moved). **No further attempts** — owner
   stopped the rig work here. Open question parked with the VM: whether
   8.0.0 + clean boot initializes ("successfully") like 11.13 did.
6. Freeze actions: powered off from the console (`reboot -p`); QEMU exited;
   dongle rebound to `usbhid` (6-1:1.0); host `holly status` equals the
   pre-flight read (EQ 3, voice 3, reverb off gear 0, light on — nothing
   drifted through the rig work). Disk keeps: installed app + splits, the
   8.0.0 blob set, the 9.0.5c tgz backups + `.905c.bak` dirs,
   `/data/local/guest-net.sh`.
7. Per PROMPT-holly-continue.md decision: Phase 1 frozen, **Phase 1B**
   (guarded PC-side round trips) is the substitute-evidence lane; see
   the gui-roadmap §1 amendment.

## Rig fix attempt 2 (2026-11-06, fresh Bliss 11.13): blocked by the ROM linker, pivoting to Bliss 11.14

Sequence on the fresh install (`tools/android-x86.qcow2`, API 28, houdini
8.0.0_y.49374, adb via root-injected `/data/misc/adb/adb_keys` + adbd TCP):

- App installed from the ORIGINAL minSdk-28 slice (base 184,487,863 B +
  `armeabi_v7a` + `mdpi` splits) read from the preserved r2 disk through a
  hot-attached virtio drive (`drive_add` + `device_add virtio-blk-pci`,
  mounted ro at `/r2d`). **Recipe trap:** plain `pm install a b c` attaches
  only the FIRST file (`pm path` proof); multi-APK requires the session API
  (`pm install-create` / `install-write` / `install-commit`, in-guest script
  `/data/local/tmp/inst.sh`). Host `adb install-multiple` fails for the
  minSdk-29 xapk slice, not for this one.
- Process then ran under houdini (`houdini: Initialize library ... 
  successfully`, libs load "via Native Bridge": `libc++_shared.so`,
  `libmarsxlog.so`, `libdatastore_shared_counter.so`, `libcrashlytics.so`).
- Crash loop #1: Crashlytics NDK spawns `CrashpadMain` as a **32-bit**
  `app_process32` child whose host-side `dlopen` rejects the ARM handler
  (`unexpected e_machine: 40 (EM_ARM)`), killing the parent each time
  (584+ deaths). Worked around with a 32-bit x86 stub
  (`-m32 -shared -fPIC -nostdlib`, exporting
  `Java_com_google_firebase_crashlytics_ndk_CrashpadMain_crashpadMain`)
  written over `lib/arm/libcrashlytics-handler.so`.
- Crash loop #2 exposed the real blocker: Bliss's patched linker aborts in
  `increment_dso_handle_reference_counter` (SIGABRT, tombstone ABI 'x86',
  backtrace `__cxa_thread_atexit_impl` ← `libhoudini.so`) whenever
  translated code registers a thread-local destructor. ROM-level bug in
  this build; second gate attempt (disabled `libcrashlytics.so` so the NDK
  path never initializes — `JniNativeApi` catches it) still hits the same
  linker abort. Not stubbable.
- Decision per plan: pivot to the pre-approved fallback, Bliss 11.14
  official (`bliss-v11.14-official-20210508-1013_x86_64_k-k4.19.122-ax86-
  ga-rmi_m-20.1.10_pie`, 1,621,098,496 B, metadata sha1 `ded04da42d64…`,
  sha256 from the item's own sidecar
  `795299b00a776a2a09fed6d2d55d01964b56bf85344faf162a3d1b05817b72d2`).
  Download in progress at time of writing.
