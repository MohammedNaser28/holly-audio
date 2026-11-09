#!/system/bin/sh
# guest-net.sh — Bliss 11.14 (Android 9, kernel 4.19.122-ax86) slirp/adb net recipe.
# Proven 2026-11-06; every step verified against netd pie source (netd is local
# netlink only — no network needed to run this, so it is safe before adbd).
#
# Why: 11.14's kernel has no virtio_net; e1000 appears as wifi_eth; Android 9
# policy routing ignores the kernel main table; and the system brings up a
# virtual wlan0 clone (same MAC/IP) that steals the ARP path.
#
# Run ONCE per boot, after boot completes. Console:  sh /data/local/guest-net.sh
# Persistence candidate: /data/local/userinit.sh (hook pending verification).

IF=wifi_eth
IP=10.0.2.15
GW=10.0.2.2

ifconfig "$IF" "$IP" up

# netId 99 is LOCAL_NET_ID — a built-in LocalNetwork; routes added to it land
# in the local_network table covered by rule 17000 (fwmark 0x0/0x10000).
ndc network interface add 99 "$IF"
ndc network route add 99 "$IF" 10.0.2.0/24

# the wlan0 clone (same MAC/IP, brought up by the system) steals ARP
ifconfig wlan0 down 2>/dev/null

# acceptance gate
if ping -c 2 "$GW" > /dev/null 2>&1; then
  echo "NET-OK"
else
  echo "NET-FAIL"
fi

# transport (silence = success; verify: getprop init.svc.adbd -> running)
start adbd
