#!/usr/bin/env bash
set -euo pipefail

PREFIX="/usr/local"
SKIP_DEPS=0

usage() {
    cat <<'EOF'
Usage: ./install.sh [--skip-deps] [--prefix DIR]

Installs the Holly Audio CLI and GPUI frontend, the IF0-only udev rule, and
the runtime packages needed by the Linux desktop frontend.
EOF
}

while (($# > 0)); do
    case "$1" in
        --skip-deps)
            SKIP_DEPS=1
            shift
            ;;
        --prefix)
            if (($# < 2)); then
                echo "error: --prefix needs a directory" >&2
                exit 2
            fi
            PREFIX=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

if [[ ! -f "$SCRIPT_DIR/holly" || ! -f "$SCRIPT_DIR/holly-gpui" ]]; then
    echo "error: run this script from an extracted release directory" >&2
    echo "       expected holly and holly-gpui beside install.sh" >&2
    exit 1
fi

if [[ $EUID -eq 0 ]]; then
    SUDO=()
else
    if ! command -v sudo >/dev/null 2>&1; then
        echo "error: sudo is required when not running as root" >&2
        exit 1
    fi
    SUDO=(sudo)
fi

install_dependencies() {
    local distro_like=${ID_LIKE:-}
    case "${ID:-}:$distro_like" in
        debian:*|ubuntu:*|pop:*)
            "${SUDO[@]}" apt-get update
            "${SUDO[@]}" apt-get install --yes \
                libfontconfig1 \
                libgl1 \
                libudev1 \
                libvulkan1 \
                libwayland-client0 \
                libx11-xcb1 \
                libxcb1 \
                libxcursor1 \
                libxfixes3 \
                libxkbcommon-x11-0 \
                libxkbcommon0 \
                libxrandr2
            ;;
        fedora:*|rhel:*|centos:*)
            "${SUDO[@]}" dnf install --assumeyes \
                fontconfig \
                libX11-xcb \
                libXcursor \
                libXfixes \
                libXi \
                libXkbcommon \
                libXkbcommon-x11 \
                libXrandr \
                libxcb \
                mesa-libEGL \
                mesa-libGL \
                systemd-libs \
                vulkan-loader \
                wayland
            ;;
        arch:*|manjaro:*|endeavouros:*)
            "${SUDO[@]}" pacman --sync --needed --noconfirm \
                fontconfig \
                libx11 \
                libxcb \
                libxcursor \
                libxfixes \
                libxi \
                libxkbcommon \
                libxkbcommon-x11 \
                libxrandr \
                mesa \
                systemd \
                vulkan-icd-loader \
                wayland
            ;;
        *)
            echo "error: unsupported Linux distribution: ${ID:-unknown}" >&2
            echo "       use --skip-deps and install the GPUI runtime libraries manually" >&2
            exit 1
            ;;
    esac
}

if ((SKIP_DEPS == 0)); then
    if [[ ! -r /etc/os-release ]]; then
        echo "error: cannot identify the Linux distribution (/etc/os-release missing)" >&2
        exit 1
    fi
    # shellcheck disable=SC1091
    source /etc/os-release
    install_dependencies
fi

"${SUDO[@]}" install --directory --mode 0755 "$PREFIX/bin"
"${SUDO[@]}" install --mode 0755 "$SCRIPT_DIR/holly" "$PREFIX/bin/holly"
"${SUDO[@]}" install --mode 0755 "$SCRIPT_DIR/holly-gpui" "$PREFIX/bin/holly-gpui"
"${SUDO[@]}" install --directory --mode 0755 /etc/udev/rules.d
"${SUDO[@]}" install --mode 0644 \
    "$SCRIPT_DIR/70-hollyland.rules" \
    /etc/udev/rules.d/70-hollyland.rules

if command -v udevadm >/dev/null 2>&1; then
    "${SUDO[@]}" udevadm control --reload-rules
    "${SUDO[@]}" udevadm trigger --subsystem-match=usb
fi

cat <<EOF
Installed holly and holly-gpui into ${PREFIX}/bin.
Installed the IF0-only udev rule into /etc/udev/rules.d/70-hollyland.rules.
Run: holly status
EOF
