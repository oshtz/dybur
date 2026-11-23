#!/usr/bin/env bash
# Smoke-test the macOS update helper against a built DMG.
#
# Copies the app out of the DMG, then runs that app's own binary as the update
# helper so it replaces its own bundle from the same DMG, exactly as an installed
# app does when it updates. This catches the helper being killed at launch (for
# example by code signing, as in 1.3.0-1.3.7) and a broken bundle swap.
#
# Usage: scripts/smoke-macos-updater.sh <path/to/dybur-macos-arm64.dmg> [--require-signature]
set -euo pipefail

usage() {
  echo "Usage: $0 <dmg> [--require-signature]" >&2
  exit 2
}

[ $# -ge 1 ] || usage
DMG=$1
REQUIRE_SIGNATURE=0
if [ "${2:-}" = "--require-signature" ]; then
  REQUIRE_SIGNATURE=1
elif [ $# -gt 1 ]; then
  usage
fi
[ -f "$DMG" ] || { echo "DMG not found: $DMG" >&2; exit 2; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/dybur-updater-smoke.XXXXXX")
MOUNT="$WORK/mount"
mkdir -p "$MOUNT" "$WORK/home" "$WORK/install"
cleanup() {
  hdiutil detach "$MOUNT" -quiet -force >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  if [ -f "$WORK/updater.log" ]; then
    echo "--- updater.log ---" >&2
    cat "$WORK/updater.log" >&2
  fi
  exit 1
}

# Install the app from the DMG into a scratch "Applications" folder.
hdiutil attach "$DMG" -mountpoint "$MOUNT" -nobrowse -readonly -quiet
APP_NAME=$(cd "$MOUNT" && ls -d ./*.app | head -1)
[ -n "$APP_NAME" ] || fail "no .app bundle in $DMG"
ditto "$MOUNT/$APP_NAME" "$WORK/install/$APP_NAME"
hdiutil detach "$MOUNT" -quiet
APP="$WORK/install/$(basename "$APP_NAME")"
EXE=$(find "$APP/Contents/MacOS" -maxdepth 1 -type f -perm -u+x | head -1)
[ -n "$EXE" ] || fail "no executable in $APP/Contents/MacOS"
cp "$DMG" "$WORK/update.dmg"

# Stand-in for the running app that the helper waits on.
sleep 2 &
APP_PID=$!

set +e
HOME="$WORK/home" DYBUR_UPDATE_HELPER_SKIP_RELAUNCH=1 "$EXE" \
  --dybur-update-helper \
  --platform macos-dmg \
  --version smoke-test \
  --pid "$APP_PID" \
  --artifact "$WORK/update.dmg" \
  --target-exe "$EXE" \
  --bundle "$APP" \
  --relaunch "$APP" \
  --log "$WORK/updater.log"
STATUS=$?
set -e

if [ "$STATUS" -gt 128 ]; then
  fail "update helper was killed by signal $((STATUS - 128)) (code signing rejected it?)"
fi
[ "$STATUS" -eq 0 ] || fail "update helper exited with status $STATUS"
grep -q "install complete" "$WORK/updater.log" || fail "updater.log has no 'install complete'"
[ -x "$EXE" ] || fail "replaced bundle has no executable at $EXE"
[ ! -e "$APP.bak" ] || fail "backup bundle $APP.bak was left behind"
[ ! -e "$WORK/home/.dybur/updates/failed-update-version" ] || fail "helper recorded a failed update"

if [ "$REQUIRE_SIGNATURE" -eq 1 ]; then
  codesign --verify --deep --strict "$APP" || fail "replaced bundle fails codesign verification"
fi

echo "OK: update helper ran from inside the bundle and replaced it"
cat "$WORK/updater.log"
