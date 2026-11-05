#!/usr/bin/env bash
# Task-4c crash-path check for the Hollyland dongle (3547:0407).
#
# Runs holly-usb/examples/claim_and_sleep.rs, kills it with SIGKILL (no Drop
# handlers run, no usbfs RELEASE ioctl), then prints the resulting kernel
# state. This script only READS sysfs and prints command outputs; it never
# claims anything itself, never sends SET_NOISE, and never unbinds drivers.
#
# Usage:  tools/crash_test.sh          (dongle plugged in; no VM passthrough)
#
# Run WITHOUT sudo: the probe binary needs only the udev-rule grant
# (packaging/99-hollyland.rules) and cargo must run as the invoking user —
# rustup has no toolchain configured for root. If accidentally started as
# root, the script re-execs itself as the original user instead of failing
# late at `cargo build` (observed 2026-11-05: rustup "no default toolchain").
set -u

# Re-exec as the invoking user when started via sudo.
if [ "$(id -u)" = "0" ] && [ -n "${SUDO_USER:-}" ]; then
    echo "(started as root — re-executing as '$SUDO_USER': cargo must run as the user)"
    exec sudo -u "$SUDO_USER" "$0" "$@"
fi

cd "$(cd "$(dirname "$0")/.." && pwd)"

# ---- locate the dongle (same walk as the capture tools) ---------------------
dev=""
for d in /sys/bus/usb/devices/*; do
    [ -f "$d/idVendor" ] || continue
    [ "$(cat "$d/idVendor" 2>/dev/null)" = "3547" ] || continue
    [ "$(cat "$d/idProduct" 2>/dev/null)" = "0407" ] || continue
    dev="$d"
    break
done
if [ -z "$dev" ]; then
    echo "no dongle 3547:0407 on the bus — plug it in and rerun"
    exit 1
fi
echo "dongle sysfs node: $dev (parent $(basename "$(dirname "$dev")"))"

# ---- state recorder: which driver is bound to which interface ---------------
print_state() {
    local label="$1"
    echo "--- $label ---"
    for i in 0 1 2 3; do
        if="$dev:1.$i"
        if [ ! -e "$if" ]; then continue; fi
        printf '%-22s driver=%s\n' "$(basename "$if")" \
            "$( [ -e "$if/driver" ] && basename "$(readlink "$if/driver")" || echo NONE )"
    done
    echo -n "bus node: "
    # sysfs busnum/devnum are plain decimals; the node path is zero-padded
    # (/dev/bus/usb/006/001). (Previous dd-over-descriptor walk printed NONE
    # when the reader lacked descriptor access — sysfs files always readable.)
    busnum="$(cat "$dev/busnum" 2>/dev/null || true)"
    devnum="$(cat "$dev/devnum" 2>/dev/null || true)"
    if [ -n "$busnum" ] && [ -n "$devnum" ]; then
        printf '/dev/bus/usb/%03d/%03d\n' "$busnum" "$devnum"
    else
        echo "NONE (no busnum/devnum in sysfs)"
    fi
    echo "ALSA capture devices (arecord -l):"
    arecord -l 2>&1 | sed 's/^/    /'
    echo "PipeWire sources matching holly/Hollyland:"
    # NOTE: `grep | sed || echo` never reached the fallback (grep finds
    # nothing -> sed still exits 0). Collect first, fall back on emptiness.
    pw="$(pactl list sources 2>/dev/null | grep -iE 'holly|wireless microphone' | sed 's/^/    /')"
    if [ -n "$pw" ]; then
        echo "$pw"
    else
        echo "(no PipeWire match / not running)"
    fi
    echo
}

# VID 0x3547 = 13639 decimal, PID 0x0407 = 1031 decimal  (inputs/lsusb-v.txt)
print_state "BEFORE (idle: kernel drivers as the OS left them)"

# ---- build + run + SIGKILL --------------------------------------------------
cargo build -p holly-usb --example claim_and_sleep >/dev/null || exit 1

./target/debug/examples/claim_and_sleep 2>/tmp/opencode/claim.log &
probe_pid=$!

# wait for READY (claim actually established) — up to 15 s
ready=""
for _ in $(seq 1 150); do
    if grep -q READY /tmp/opencode/claim.log 2>/dev/null; then ready=yes; break; fi
    if ! kill -0 "$probe_pid" 2>/dev/null; then break; fi
    sleep 0.1
done
if [ -z "$ready" ]; then
    echo "PROBE DID NOT REPORT READY — log follows (already dead?):"
    cat /tmp/opencode/claim.log
    if ! kill -0 "$probe_pid" 2>/dev/null; then exit 1; fi
fi
echo "probe running as pid $probe_pid; claim log:"
sed 's/^/    /' /tmp/opencode/claim.log

echo "--- DURING claim (should show IF0 WITHOUT usbhid) ---"
for i in 0 1 2 3; do
    if="$dev:1.$i"
    [ -e "$if" ] || continue
    printf '%-22s driver=%s\n' "$(basename "$if")" \
        "$( [ -e "$if/driver" ] && basename "$(readlink "$if/driver")" || echo NONE )"
done

kill -9 "$probe_pid"
wait "$probe_pid" 2>/dev/null
echo "killed probe with SIGKILL (no Drop handler ran)"

# give the kernel a moment to release/close anything it will reattach
sleep 1

print_state "AFTER SIGKILL (task: does usbhid come back to IF0? audio still bound?)"

echo "== done =="
echo "Report this whole stdout + whether audio capture from the dongle still"
echo "works (arecord -D <source> ... if you want to test actually capturing)."
