#!/bin/sh
# Link a minimal program against the QNX relibc build.
#
# This proves the toolchain end to end without a QNX SDP: cargo compiles a
# no_std program for the QNX target and rust-lld links it against crt0, crti,
# libc.a and crtn to produce an AArch64 ELF executable. It does not run
# anything -- that needs a QNX target.
#
# QNX 7.1 by default; QNX 8.0 with
#   TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/smoke-link.sh
set -eu

# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."

TARGET_SPEC=$(cd "$(dirname "${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}")" && pwd)/$(basename "${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}")
TARGET=$(basename "$TARGET_SPEC" .json)
PROFILE=${PROFILE:-release}
CARGO=${CARGO:-cargo}
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(pwd)/target}
export CARGO_TARGET_DIR
OUT="$CARGO_TARGET_DIR/$TARGET/$PROFILE"

for obj in libc.a crt0.o crti.o crtn.o; do
    [ -f "$OUT/$obj" ] || { echo "missing $OUT/$obj; run qnx/build-qnx.sh first" >&2; exit 1; }
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/hello/src"
cat > "$WORK/hello/Cargo.toml" <<'EOF'
[package]
name = "hello"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["staticlib"]
path = "src/lib.rs"

[profile.release]
panic = "abort"
EOF

cat > "$WORK/hello/src/lib.rs" <<'EOF'
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

unsafe extern "C" {
    fn write(fd: i32, buf: *const u8, n: usize) -> isize;
}

#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let msg = b"hello from relibc on QNX\n";
    unsafe { write(1, msg.as_ptr(), msg.len()) };
    0
}
EOF

export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options"
PROFILE_FLAG=""
[ "$PROFILE" = release ] && PROFILE_FLAG="--release"

# shellcheck disable=SC2086
$CARGO rustc $PROFILE_FLAG \
    -Z build-std=core,alloc,compiler_builtins -Z json-target-spec \
    --manifest-path "$WORK/hello/Cargo.toml" --target="$TARGET_SPEC" \
    -- --emit link="$WORK/libhello.a"

HOST=$(rustc -vV | sed -n 's/host: //p')
RUST_LLD="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/rust-lld"

"$RUST_LLD" -flavor gnu --eh-frame-hdr -o "$WORK/hello.elf" \
    "$OUT/crt0.o" "$OUT/crti.o" \
    --start-group "$WORK/libhello.a" "$OUT/libc.a" --end-group \
    "$OUT/crtn.o"

cp "$WORK/hello.elf" "$OUT/hello-qnx.elf"
echo "linked $OUT/hello-qnx.elf"
for readelf in aarch64-none-elf-readelf readelf; do
    if command -v "$readelf" >/dev/null 2>&1; then
        "$readelf" -lW "$OUT/hello-qnx.elf" | grep -E 'LOAD|TLS|GNU_EH_FRAME'
        break
    fi
done
