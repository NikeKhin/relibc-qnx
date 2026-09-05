#!/bin/sh
# Build the bring-up self-test against the QNX relibc build.
#
# Produces qnx-selftest next to libc.a, a statically linked AArch64 QNX
# executable. Copy it to the target and run it: it prints one line per step,
# so a failure says which layer broke and with what errno.
#
# QNX 7.1 by default; QNX 8.0 with
#   TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/build-selftest.sh
set -eu

# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/ensure-nightly.sh

TARGET_SPEC=${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}
TARGET=$(basename "$TARGET_SPEC" .json)
PROFILE=${PROFILE:-release}
CARGO=${CARGO:-cargo}
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(pwd)/target}
export CARGO_TARGET_DIR
OUT="$CARGO_TARGET_DIR/$TARGET/$PROFILE"

for obj in libc.a crt1.o crti.o crtn.o; do
    [ -f "$OUT/$obj" ] || { echo "missing $OUT/$obj; run qnx/build-qnx.sh first" >&2; exit 1; }
done

export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options"
PROFILE_FLAG=""
[ "$PROFILE" = release ] && PROFILE_FLAG="--release"

# shellcheck disable=SC2086
$CARGO rustc $PROFILE_FLAG \
    -Z build-std=core,alloc,compiler_builtins -Z json-target-spec \
    --manifest-path qnx/selftest/Cargo.toml --target="$TARGET_SPEC" \
    -- --emit link="$OUT/libqnx_selftest.a"

HOST=$(rustc -vV | sed -n 's/host: //p')
RUST_LLD="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/rust-lld"

"$RUST_LLD" -flavor gnu --eh-frame-hdr -o "$OUT/qnx-selftest" \
    "$OUT/crt1.o" "$OUT/crti.o" \
    --start-group "$OUT/libqnx_selftest.a" "$OUT/libc.a" --end-group \
    "$OUT/crtn.o"

echo "built $OUT/qnx-selftest"
echo "copy THIS file onto the board (overwrite the old binary)"
echo "the first line must be:  relibc QNX backend self-test  HARNESS=6"
