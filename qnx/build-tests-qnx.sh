#!/usr/bin/env bash
# Compile relibc's C tests/ against the QNX static libc.
#
# Upstream `make test` is Linux/Redox only (gcc + sysroot/$(TARGET) + a
# host runner). This script is the QNX equivalent: the headers build-qnx.sh
# installs, a C compiler for AArch64 ELF, and rust-lld with the same
# crt+libc.a recipe as qnx-selftest.
#
# The compiler is $CC if set, else aarch64-none-elf-gcc (on PATH or under
# GCC_PREFIX), else zig cc or clang. relibc's headers are written for GCC:
# <stdatomic.h> is GCC's own, and <math.h> redeclares GCC builtins, both of
# which clang rejects, so with clang the tests that include them fail.
#
# Usage, after a successful qnx/build-qnx.sh with cbindgen on PATH:
#
#   qnx/build-tests-qnx.sh          # every standalone .c
#   ONLY='string/ malloc/' qnx/build-tests-qnx.sh
#   SKIP='sys_epoll ptrace mkfifo' qnx/build-tests-qnx.sh
#   TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/build-tests-qnx.sh
#
# Copy tests/build_qnx/ onto the board and run:
#
#   cd tests/build_qnx && ./run-on-qnx.sh
set -eu

# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/ensure-nightly.sh

TARGET_SPEC=${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}
TARGET=$(basename "$TARGET_SPEC" .json)
PROFILE=${PROFILE:-release}
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(pwd)/target}
OUT="$CARGO_TARGET_DIR/$TARGET/$PROFILE"
DEST=${DEST:-$(pwd)/tests/build_qnx}
HDR="$DEST/include"

for obj in libc.a crt1.o crti.o crtn.o; do
    [ -f "$OUT/$obj" ] || { echo "missing $OUT/$obj; run qnx/build-qnx.sh first" >&2; exit 1; }
done

SYSROOT_INC="$OUT/sysroot/usr/include"
[ -f "$SYSROOT_INC/stdio.h" ] || {
    echo "missing the generated headers in $SYSROOT_INC;" >&2
    echo "run qnx/build-qnx.sh with cbindgen on PATH (cargo install cbindgen)" >&2
    exit 1
}

HOST=$(rustc -vV | sed -n 's/host: //p')
RUST_LLD="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/rust-lld"
[ -x "$RUST_LLD" ] || { echo "rust-lld not found at $RUST_LLD" >&2; exit 1; }

# shellcheck disable=SC1091
. qnx/gcc-binutils.sh

is_elf_aarch64() {
    python3 - "$1" <<'PY'
import sys
p = sys.argv[1]
with open(p, "rb") as f:
    hdr = f.read(20)
sys.exit(0 if hdr[:4] == b"\x7fELF" and hdr[18:20] in (b"\xb7\x00", b"\x00\xb7") else 1)
PY
}

CC_BIN=${CC:-}
if [ -z "$CC_BIN" ]; then
    for cand in aarch64-none-elf-gcc zig clang-20 clang-19 clang-18 clang; do
        if command -v "$cand" >/dev/null; then
            CC_BIN=$cand
            break
        fi
    done
fi
[ -n "$CC_BIN" ] || {
    echo "No AArch64 ELF C compiler found. Put one of these on PATH:" >&2
    echo "  aarch64-none-elf-gcc   (an aarch64-none-elf GCC toolchain; or set GCC_PREFIX)" >&2
    echo "  zig                    (used as: zig cc -target aarch64-freestanding-none)" >&2
    echo "  clang                  (one that can emit ELF)" >&2
    echo "Or set CC=/path/to/compiler." >&2
    exit 1
}

# What the compiler needs to produce AArch64 ELF, and for GCC its own
# headers after relibc's, as qnx-link-check.py compiles.
CC_TARGET_FLAG=()
CC_INC=()
case "$(basename "$CC_BIN")" in
    zig) CC_TARGET_FLAG=(cc -target aarch64-freestanding-none) ;;
    *clang*) CC_TARGET_FLAG=(--target=aarch64-unknown-none-elf) ;;
    *)
        if "$CC_BIN" --version 2>/dev/null | head -1 | grep -q clang; then
            CC_TARGET_FLAG=(--target=aarch64-unknown-none-elf)
        else
            CC_INC=(-isystem "$("$CC_BIN" -print-file-name=include)")
        fi
        ;;
esac
CC_DESC="$CC_BIN${CC_TARGET_FLAG[*]:+ ${CC_TARGET_FLAG[*]}}"

probe_dir=$(mktemp -d "${TMPDIR:-/tmp}/relibc-ccprobe.XXXXXX")
echo 'int x;' >"$probe_dir/probe.c"
if ! "$CC_BIN" ${CC_TARGET_FLAG[@]+"${CC_TARGET_FLAG[@]}"} -c -ffreestanding \
        -o "$probe_dir/probe.o" "$probe_dir/probe.c" 2>"$probe_dir/probe.err"; then
    echo "compiler probe failed ($CC_DESC):" >&2
    cat "$probe_dir/probe.err" >&2
    rm -rf "$probe_dir"
    exit 1
fi
if ! is_elf_aarch64 "$probe_dir/probe.o"; then
    echo "$CC_DESC produced an object that is not AArch64 ELF." >&2
    echo "Use an aarch64-none-elf GCC, zig, or a clang that can emit ELF." >&2
    rm -rf "$probe_dir"
    exit 1
fi
rm -rf "$probe_dir"

echo "==> C compiler: $CC_DESC"
echo "==> headers"

rm -rf "$DEST"
mkdir -p "$HDR" "$DEST/obj" "$DEST/bin" "$DEST/logs"

# The headers build-qnx.sh installed, so that the tests see what programs do.
cp -a "$SYSROOT_INC/." "$HDR/"
shopt -s nullglob

# Runtime files some tests open by relative path from cwd = $DEST
# (e.g. fopen("stdio/fread.in"), scandir("example_dir/")).
cp -a tests/expected "$DEST/expected" 2>/dev/null || true
cp -a tests/example_dir "$DEST/example_dir" 2>/dev/null || true
while IFS= read -r f; do
    rel=${f#tests/}
    mkdir -p "$DEST/$(dirname "$rel")"
    cp "$f" "$DEST/$rel"
done < <(find tests -name '*.in')
cp qnx/run-on-qnx.sh "$DEST/run-on-qnx.sh"
chmod +x "$DEST/run-on-qnx.sh"

# Shared objects, header-graph helper, and Redox-only tests cannot be linked
# as a static QNX executable. epoll/ptrace/mkfifo are compiled unless SKIP
# names them — they fail at runtime, which is the point of a first full pass.
skip_file() {
    rel=$1
    case "$rel" in
        includes.c|libfoo.c|libfoobar.c|sharedlib.c) return 0 ;;
        redox_ring.c|redox_ring_dyn.c|redox_ring_ramfs.c|redox_syscall.c) return 0 ;;
        dlfcn.c|dlopen_scopes.c) return 0 ;;
    esac
    if [ -n "${SKIP:-}" ]; then
        for pat in $SKIP; do
            case "$rel" in
                *$pat*) return 0 ;;
            esac
        done
    fi
    if [ -n "${ONLY:-}" ]; then
        for pat in $ONLY; do
            case "$rel" in
                *$pat*) return 1 ;;
            esac
        done
        return 0
    fi
    return 1
}

CFLAGS=(
    # relibc's generated headers select their QNX definitions with this, as
    # cbindgen.globdefs.toml spells target_os = "nto".
    -D__QNXNTO__
    # The tests are written for C17, the default before GCC 15 chose C23.
    -std=gnu17
    -ffreestanding
    -fno-builtin
    -fno-stack-protector
    -fno-pic
    -fno-pie
    -mno-outline-atomics
    -Wall
    -Wextra
    -Wno-deprecated-declarations
    -Wno-unused-parameter
    -g
    -nostdinc
    -isystem "$HDR"
    ${CC_INC[@]+"${CC_INC[@]}"}
    -I tests
)
if [ "${WERROR:-0}" = 1 ]; then
    CFLAGS+=(-Werror -pedantic)
fi

ok=0
fail=0
skip=0
: >"$DEST/COMPILE_REPORT"

echo "==> compile + link"
while IFS= read -r src; do
    rel=${src#tests/}
    name=${rel%.c}
    if skip_file "$rel"; then
        echo "SKIP $name" | tee -a "$DEST/COMPILE_REPORT"
        skip=$((skip + 1))
        continue
    fi
    obj="$DEST/obj/$name.o"
    bin="$DEST/bin/$name"
    mkdir -p "$(dirname "$obj")" "$(dirname "$bin")"
    log="$DEST/logs/${name//\//_}.log"
    if ! "$CC_BIN" ${CC_TARGET_FLAG[@]+"${CC_TARGET_FLAG[@]}"} "${CFLAGS[@]}" -c -o "$obj" "$src" >"$log" 2>&1; then
        echo "FAIL compile $name" | tee -a "$DEST/COMPILE_REPORT"
        fail=$((fail + 1))
        continue
    fi
    if ! "$RUST_LLD" -flavor gnu --eh-frame-hdr -o "$bin" \
        "$OUT/crt1.o" "$OUT/crti.o" \
        --start-group "$obj" "$OUT/libc.a" --end-group \
        "$OUT/crtn.o" >>"$log" 2>&1; then
        echo "FAIL link $name" | tee -a "$DEST/COMPILE_REPORT"
        fail=$((fail + 1))
        continue
    fi
    echo "ok   $name" | tee -a "$DEST/COMPILE_REPORT"
    ok=$((ok + 1))
done < <(find tests -name '*.c' | sed 's|^\./||' | sort)

echo
echo "compiled $ok, failed $fail, skipped $skip"
echo "binaries: $DEST/bin"
echo "copy the whole $DEST directory to the board (not only bin/)"
echo "on the board:  cd /path/to/build_qnx && ./run-on-qnx.sh"
echo
echo "Later, skip known-missing Pal pieces:"
echo "  SKIP='sys_epoll ptrace mkfifo unistd/brk' qnx/build-tests-qnx.sh"
