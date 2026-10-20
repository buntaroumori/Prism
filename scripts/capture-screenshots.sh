#!/usr/bin/env bash
# Capture real screenshots of the running Prism shell on a headless Linux box.
#
# Requirements: xvfb, imagemagick, dbus-x11, plus the standard Tauri build deps.
# Usage: ./scripts/capture-screenshots.sh [output-dir]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/docs/media}"
DISPLAY_NUM="${PRISM_CAPTURE_DISPLAY:-:99}"
WIDTH="${PRISM_CAPTURE_WIDTH:-1440}"
HEIGHT="${PRISM_CAPTURE_HEIGHT:-900}"
SETTLE_SECONDS="${PRISM_CAPTURE_SETTLE:-12}"

mkdir -p "$OUT"

cleanup() {
  if [[ -n "${APP_PID:-}" ]]; then
    kill "$APP_PID" 2>/dev/null || true
  fi
  if [[ -n "${XVFB_PID:-}" ]]; then
    kill "$XVFB_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

Xvfb "$DISPLAY_NUM" -screen 0 "${WIDTH}x${HEIGHT}x24" -nolisten tcp >/tmp/prism-xvfb.log 2>&1 &
XVFB_PID=$!
sleep 1

export DISPLAY="$DISPLAY_NUM"
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export PRISM_LOG="prism=debug,warn"

cd "$ROOT"
dbus-run-session -- pnpm tauri dev >/tmp/prism-capture.log 2>&1 &
APP_PID=$!

echo "waiting ${SETTLE_SECONDS}s for the WebView to paint…"
sleep "$SETTLE_SECONDS"

import -display "$DISPLAY_NUM" -window root "$OUT/overview.png"
echo "wrote $OUT/overview.png"
