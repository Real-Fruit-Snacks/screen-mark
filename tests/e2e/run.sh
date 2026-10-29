#!/usr/bin/env bash
# Run the end-to-end usage tests on a virtual X display.
#
#   tests/e2e/run.sh [path/to/screen-mark] [test-name-substring]
#
# Needs: Xvfb openbox stalonetray xdotool xclip imagemagick x11-xserver-utils
#        python3-pil python3-xlib python3-dbus python3-gi dbus-x11
#        libayatana-appindicator3-1 (Ubuntu package names)
set -euo pipefail

BIN="${1:-target/debug/screen-mark}"
ONLY="${2:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"

if [ ! -x "$BIN" ]; then
    echo "no binary at $BIN; run cargo build first" >&2
    exit 2
fi

DISP=":${E2E_DISPLAY:-87}"
Xvfb "$DISP" -screen 0 1920x1080x24 -nolisten tcp >/dev/null 2>&1 &
XVFB=$!
cleanup() {
    kill "$WM_PID" "$TRAY_PID" 2>/dev/null || true
    kill "$XVFB" 2>/dev/null || true
}
trap cleanup EXIT

export DISPLAY="$DISP"
for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done

openbox >/dev/null 2>&1 &
WM_PID=$!
# An XEmbed tray host, so the tray icon has somewhere to live.
stalonetray --geometry 4x1+1700+0 --icon-size 24 --kludges force_icons_size >/dev/null 2>&1 &
TRAY_PID=$!
sleep 1

# The distro's python has the apt-installed PIL and Xlib; a python3 earlier
# on PATH (pyenv, a CI tool cache) may not.
PY=python3
if ! python3 -c 'import PIL, Xlib' 2>/dev/null && [ -x /usr/bin/python3 ]; then
    PY=/usr/bin/python3
fi

# A session bus for the system-notification test.
status=0
dbus-run-session -- "$PY" "$HERE/e2e.py" "$BIN" "$ONLY" || status=$?
exit "$status"
