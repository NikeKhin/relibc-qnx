#!/bin/sh
# Typecheck redox-rt with the pinned nightly (a stable cargo cannot pass -Z).
#
# Not --offline: QNX builds never download redox-rt's crates.io packages
# (goblin), so the first run fetches them. Set CARGO_OFFLINE=1 only if they
# are already cached.
set -eu
# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/ensure-nightly.sh
TARGET=${1:-x86_64-unknown-redox}
OFFLINE=
[ "${CARGO_OFFLINE:-0}" = 1 ] && OFFLINE=--offline
# Cargo does not rebuild a directory source whose files change under the same
# version, so a refreshed copy of redox_syscall would otherwise go unnoticed.
# shellcheck disable=SC2086
cargo clean -q -p redox_syscall $OFFLINE --config qnx/tools/cargo-redox-stubs.toml --target "$TARGET" || true
echo "==> cargo check -p redox-rt --locked $OFFLINE --target $TARGET (Redox git dependencies: local stand-ins)"
# shellcheck disable=SC2086
exec cargo check -p redox-rt --locked $OFFLINE --config qnx/tools/cargo-redox-stubs.toml --target "$TARGET" \
    -Z build-std=core,alloc,compiler_builtins
