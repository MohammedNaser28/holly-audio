#!/usr/bin/env bash
# tools/qemu-android.sh — run an Android-x86 VM with the Hollyland dongle
# passed through on USB, so HollyAudio talks to the dongle from the guest while
# all USB traffic still flows through THIS host's kernel (visible to usbmon).
#
# Usage:
#   tools/qemu-android.sh install -i <android-x86.iso>   # first boot: live ISO installer
#   tools/qemu-android.sh run                            # normal boot from installed disk
#
# ABI note: HollyAudio 3.1.0 ships only armeabi-v7a libraries. Use an
# Android-x86/Prime OS image that bundles libhoudini ARM translation
# (Bliss OS builds without houdini will not run the app).
# Docs: docs/capture.md
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
DISK="${DISK:-${DIR}/android-x86.qcow2}"
MODE="${1:-run}"
shift || true
ISO=""

case $MODE in
  install)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        -i|--iso) ISO="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    if [[ -z $ISO || ! -f $ISO ]]; then
      echo "install mode requires: $0 install -i <android-x86.iso>"
      exit 2
    fi
    ;;
  run) ;;
  *) echo "usage: $0 [install -i iso | run]"; exit 2 ;;
esac

command -v qemu-system-x86_64 >/dev/null || { echo "qemu missing: sudo pacman -S qemu-desktop"; exit 1; }

# (dongle check moved below, with the QEMU_ALLOW_NO_DONGLE=1 bypass)
if [[ $MODE == run ]]; then :; fi

if [[ ! -f $DISK ]]; then
  echo "no disk yet, creating ${DISK} (16 GiB, sparse)"
  qemu-img create -f qcow2 "${DISK}" 16G >/dev/null
fi

CKVM=()
if [[ -e /dev/kvm ]]; then
  CKVM=(-enable-kvm)
else
  echo "!! /dev/kvm missing — booting without KVM will be slow" >&2
fi

# usb-host by vid/pid: replug address changes don't break the command line.
# -boot d only in install mode; otherwise boot the installed disk.
HAVE_DONGLE=1
lsusb | grep -iq "3547:0407" || {
  if [[ ${QEMU_ALLOW_NO_DONGLE:-0} == 1 ]]; then
    HAVE_DONGLE=0
    echo "warn: dongle 3547:0407 not on the USB bus — starting without USB passthrough"
  else
    echo "dongle 3547:0407 not found in lsusb — is it plugged in?"; exit 1
  fi
}

USB=()
if [[ $HAVE_DONGLE == 1 ]]; then
  USB=(-device usb-host,vendorid=0x3547,productid=0x0407)
fi

EXTRA=()
if [[ $MODE == install ]]; then
  EXTRA=(-boot d -cdrom "${ISO}")
fi

exec qemu-system-x86_64 \
  "${CKVM[@]}" \
  -m 4096 -smp 4 \
  -device qemu-xhci,id=xhci \
  "${USB[@]}" \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:5555-:5555 \
  -device "${QEMU_NIC:-virtio-net-pci}",netdev=n0 \
  -audiodev pa,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 \
  -display "${QEMU_DISPLAY:-sdl,gl=off}" \
  -monitor unix:/tmp/opencode/qmon.sock,server,nowait \
  -serial file:/tmp/opencode/qserial.log \
  -hda "${DISK}" \
  "${EXTRA[@]}"
