#!/bin/bash
# Static package smoke only: never launches a GUI, including on local macOS.
# Usage: bash tests/package-mac.sh [zip [expected-version ["arm64 x86_64"]]]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZIP="${1:-$ROOT/build/release/NovaCut-macOS.zip}"
VERSION="${2:-$(awk '/^\[package\]$/ { package=1; next } /^\[/ { package=0 } package && /^version *=/ { gsub(/"/, "", $3); print $3; exit }' "$ROOT/Cargo.toml")}"
EXPECTED_ARCHS="${3:-arm64 x86_64}"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
[[ "$(uname -s)" == Darwin ]] || fail 'Requires macOS'
[[ -f "$ZIP" ]] || fail "Missing ZIP: $ZIP"
[[ -n "$VERSION" && -n "$EXPECTED_ARCHS" ]] || fail 'Expected version and architectures must not be empty'

SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/novacut-package.XXXXXXXX")"
cleanup() { rm -rf -- "$SANDBOX"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

ditto -x -k "$ZIP" "$SANDBOX"
APP="$SANDBOX/NovaCut.app"
PLIST="$APP/Contents/Info.plist"
BIN="$APP/Contents/MacOS/Editorcito"
[[ -x "$BIN" ]] || fail 'Missing executable: Contents/MacOS/Editorcito'
plutil -lint "$PLIST"

assert_plist() {
    local actual
    actual="$(/usr/libexec/PlistBuddy -c "Print :$1" "$PLIST")"
    [[ "$actual" == "$2" ]] || fail "Plist $1: expected '$2', got '$actual'"
}
assert_plist CFBundleName NovaCut
assert_plist CFBundleDisplayName NovaCut
assert_plist CFBundleExecutable Editorcito
assert_plist CFBundleIdentifier studio.editorcito.mac
assert_plist CFBundlePackageType APPL
assert_plist CFBundleShortVersionString "$VERSION"
assert_plist CFBundleVersion "$VERSION"
assert_plist LSMinimumSystemVersion 14.0

codesign --verify --deep --strict "$APP"
ACTUAL_ARCHS="$(lipo -archs "$BIN")"
# Compare sets, not lipo's architecture ordering.
normalize_archs() { printf '%s\n' "$1" | tr ' ' '\n' | LC_ALL=C sort -u | tr '\n' ' '; }
[[ "$(normalize_archs "$ACTUAL_ARCHS")" == "$(normalize_archs "$EXPECTED_ARCHS")" ]] ||
    fail "Expected architectures '$EXPECTED_ARCHS', got '$ACTUAL_ARCHS'"
for notice in LICENSE THIRD_PARTY_NOTICES.md; do
    packaged="$APP/Contents/Resources/$notice"
    [[ -s "$packaged" ]] || fail "Missing or empty $notice"
    cmp -s "$ROOT/$notice" "$packaged" || fail "Stale $notice"
done
printf 'PASS: macOS ZIP, signature, plist version %s, architectures %s, license and notices (no GUI launch)\n' "$VERSION" "$ACTUAL_ARCHS"
