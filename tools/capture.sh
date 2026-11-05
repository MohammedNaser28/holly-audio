#!/usr/bin/env bash
# tools/capture.sh — capture one USB session of the Hollyland dongle to a pcap.
# Usage: sudo tools/capture.sh -b 6 -d 6 -o captures/gain-up.pcap [-t 20]
# You act in HollyAudio (inside Waydroid) while this runs.
set -euo pipefail

BUS="" DEV="" OUT="" DUR=20
while getopts "b:d:o:t:h" opt; do
  case $opt in
    b) BUS=$OPTARG ;;
    d) DEV=$OPTARG ;;
    o) OUT=$OPTARG ;;
    t) DUR=$OPTARG ;;
    *) echo "usage: $0 -b <bus> -d <devaddr> -o <out.pcap> [-t seconds]"; exit 2 ;;
  esac
done
[[ -n $BUS && -n $DEV && -n $OUT ]] || { echo "usage: $0 -b <bus> -d <devaddr> -o <out.pcap> [-t seconds]"; exit 2; }

command -v tshark >/dev/null || { echo "tshark missing: sudo pacman -S wireshark-cli"; exit 1; }

modprobe usbmon 2>/dev/null || true
if [[ ! -e /sys/kernel/debug/usb/usbmon ]]; then
  echo "usbmon not available — is it modprobed as root?" >&2; exit 1
fi
# Warn if the capture must run as root (usbmon is root-only by default)
if [[ $EUID -ne 0 ]]; then
  echo "usbmon needs root by default; re-run with sudo" >&2; exit 1
fi
# lsusb numbers:  usbmon<BUS>  ;
# snaplen 512: enough for full 64 B bulk/interrupt usbmon records.
# No -f (libpcap) filter: usbmon BPF syntax varies across kernel/libpcap versions.
# Keep everything on this bus; pcap_to_frames.py filters by bus/device afterwards.
tshark -i "usbmon${BUS}" \
  -a duration:"${DUR}" \
  -s 512 \
  -Y "usb.bus_id == ${BUS} && usb.device_address == ${DEV}" \
  -w "${OUT}"
echo "wrote ${OUT}"
