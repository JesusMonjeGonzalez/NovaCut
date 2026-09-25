#!/usr/bin/env bash
set -euo pipefail

# Run with bash on macOS/Linux or Git Bash on Windows. No installation or fetch.
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ABOUT="${CARGO_ABOUT:-cargo-about}"
PYTHON="${PYTHON:-python3}"
MODE="${1:-generate}"
if [[ "$MODE" != generate && "$MODE" != --check ]] || [[ $# -gt 1 ]]; then
    printf 'Usage: bash tools/license-report.sh [--check]\n' >&2
    exit 2
fi
if [[ "$("$ABOUT" --version)" != 'cargo-about 0.9.2' ]]; then
    printf 'Required: cargo-about 0.9.2 (set CARGO_ABOUT to its binary path).\n' >&2
    exit 1
fi
"$PYTHON" -c 'import sys; assert sys.version_info >= (3, 11), "Python >= 3.11 required"'
TMP="$(mktemp -d "${TMPDIR:-/tmp}/novacut-licenses.XXXXXX")"
trap 'rm -rf -- "$TMP"' EXIT
"$ABOUT" generate --locked --offline --fail \
    --manifest-path "$ROOT/Cargo.toml" \
    --features windows-host --target x86_64-pc-windows-msvc \
    --config "$ROOT/docs/licenses/about.toml" \
    --format json --output-file "$TMP/about.json"
"$PYTHON" "$ROOT/docs/licenses/render.py" "$TMP/about.json" "$TMP"
for FILE in windows-inventory.json THIRD_PARTY_LICENSES-Windows.html; do
    if [[ "$MODE" == --check ]]; then
        cmp "$TMP/$FILE" "$ROOT/docs/licenses/$FILE"
    else
        mv -- "$TMP/$FILE" "$ROOT/docs/licenses/$FILE"
    fi
done
printf 'Windows license report: %s\n' "$MODE successful"
