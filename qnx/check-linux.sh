#!/bin/sh
# Typecheck the Linux Pal. The build host need not be Linux, so the target
# is given explicitly; ensure-nightly.sh puts the pinned nightly first.
#
# Not --offline: QNX builds never download linux-only crates.io packages
# (sc), so the first run fetches them. Set CARGO_OFFLINE=1 only if they are
# already cached.
set -eu
# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/ensure-nightly.sh
TARGET=${1:-x86_64-unknown-linux-gnu}
OFFLINE=
[ "${CARGO_OFFLINE:-0}" = 1 ] && OFFLINE=--offline
export RELIBC_SKIP_C=1
echo "==> cargo check --locked $OFFLINE --target $TARGET (Redox git dependencies: local stand-ins)"
# shellcheck disable=SC2086
exec cargo check --locked $OFFLINE --config qnx/tools/cargo-redox-stubs.toml --target "$TARGET" \
    -Z build-std=core,alloc,compiler_builtins
