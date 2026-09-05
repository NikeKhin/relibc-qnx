#!/bin/sh
# Build relibc for QNX Neutrino with nothing but a Rust toolchain.
#
# Produces, under $CARGO_TARGET_DIR/<target>/release/:
#   libc.a     static libc (position-independent; also as libcS.a)
#   libc.so    shared libc, which is also relibc's dynamic linker
#   crt1.o     start-up (also as crt0.o and Scrt1.o); rcrt1.o for a static
#              PIE, gcrt1.o (mcrt1.o) for -pg, grcrt1.o for both
#   crtbeginS.o for what is position-independent; crti.o crtn.o
#   cxxabi.o   what C++ needs besides GCC's libsupc++
# and a sysroot with all of them and the headers.
#
# QNX 7.1 by default; QNX 8.0 with
#   TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/build-qnx.sh
#
# No QNX SDP, no qcc, no C cross-compiler.
set -eu

# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/ensure-nightly.sh

TARGET_SPEC=${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}
TARGET=$(basename "$TARGET_SPEC" .json)
PROFILE=${PROFILE:-release}
CARGO=${CARGO:-cargo}
# relibc's dynamic linker resolves a DT_NEEDED of libc.so.6 to itself, as
# upstream's does, so that is the SONAME on both releases.
SONAME=libc.so.6

# Custom target specifications are still unstable in both rustc and cargo.
export RUSTFLAGS="${RUSTFLAGS:-} -Zunstable-options"
UNSTABLE="-Z build-std=core,alloc,compiler_builtins -Z json-target-spec"

# * no default features: `check_against_libc_crate` compares relibc's structs
#   against the `libc` crate's QNX definitions, which describe the *vendor*
#   ABI rather than relibc's and so cannot match.
# * math_libm: use the pure-Rust `libm` crate instead of compiling openlibm,
#   which would reintroduce the C compiler dependency.
FEATURES="--no-default-features --features math_libm,no_trace"
# relibc's Redox-only git dependencies resolve to local stand-ins instead of
# being fetched from gitlab.redox-os.org; see qnx/tools/cargo-redox-stubs.toml.
STUBS="--config qnx/tools/cargo-redox-stubs.toml"

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(pwd)/target}
OUT="$CARGO_TARGET_DIR/$TARGET/$PROFILE"

PROFILE_FLAG=""
[ "$PROFILE" = release ] && PROFILE_FLAG="--release"

echo "==> libc.a"
# initial-exec TLS: GNU TLSDESC (outline GD) is what Neutrino's ldd reports
# as an empty `unknown symbol` in libc.so. IE is valid for both a static
# ET_EXEC and for libc.so (always loaded at process start).
# shellcheck disable=SC2086
$CARGO rustc $STUBS $PROFILE_FLAG $UNSTABLE $FEATURES --target="$TARGET_SPEC" \
    -- --emit link="$OUT/libc.a" -Z tls-model=initial-exec

# rust-lld ships with rustc. objcopy/nm/ar are GNU binutils from the
# aarch64-none-elf GCC toolchain, so rustup's llvm-tools are not needed.
# shellcheck disable=SC1091
. qnx/gcc-binutils.sh
HOST=$(rustc -vV | sed -n 's/host: //p')
RUSTLIB="$(rustc --print sysroot)/lib/rustlib/$HOST/bin"
RUST_LLD="$RUSTLIB/rust-lld"
[ -x "$RUST_LLD" ] || { echo "rust-lld not found at $RUST_LLD" >&2; exit 1; }
export NM="${NM:-$(relibc_need_gnu nm)}"
export OBJCOPY="${OBJCOPY:-$(relibc_need_gnu objcopy)}"
GNU_AR="${AR:-$(relibc_need_gnu ar)}"

# The upstream Makefile's post-processing hides Rust allocator internals and
# then runs stripcore.sh, which deletes sqrt, floor, fmin and 25 more from
# every member so that openlibm's win. With math_libm the only strong
# definitions are relibc's own, so that would leave none; compiler-builtins'
# copies are weak and give way to them.
echo "==> rename libc.a"
./renamesyms.sh "$OUT/libc.a" "$CARGO_TARGET_DIR/$TARGET/$PROFILE/deps"
LIBC_A="$(cd "$OUT" && pwd)/libc.a"

# renamesyms.sh renames the code of relibc's Rust dependencies, core among
# them, so that a Rust program's own copies do not collide with it. Their
# data needs the same: a program built by the same toolchain has the same
# tables, and the same constants LLVM promotes between codegen units, under
# the same names. Weak definitions never collide and keep theirs.
echo "==> rename the data of relibc's Rust dependencies"
RENAME=$(mktemp)
"$NM" -A --defined-only "$LIBC_A" 2>/dev/null | awk '
    { split($1, path, ":"); member = path[2]; type = $2; sym = $3 }
    member ~ /^relibc-/ { if (type ~ /^[A-Z]$/) own[sym] = 1; next }
    type ~ /^[DRBGS]$/ { data[sym] = 1 }
    END { for (s in data) if (!(s in own)) print s, "__relibc_" s }' | sort >"$RENAME"
"$OBJCOPY" --redefine-syms="$RENAME" "$LIBC_A"
echo "    $(wc -l <"$RENAME" | tr -d ' ') symbols"
rm -f "$RENAME"

# compiler-builtins also defines sqrt, floor, fmin and others, weak and
# hidden (rust-lang/rust#142119), and one hidden definition makes the symbol
# hidden: libc.so would not export relibc's own. stripcore.sh removes the
# symbols everywhere for openlibm to define; here relibc defines them, so
# only compiler-builtins' copies are made local.
echo "==> localize compiler-builtins' copies of relibc's functions"
DUPS=$(mktemp)
"$NM" -A --defined-only "$LIBC_A" 2>/dev/null | awk '
    { split($1, path, ":"); member = path[2]; type = $2; sym = $3 }
    member ~ /^compiler_builtins-/ { if (type == "W") weak[sym] = 1; next }
    type ~ /^[TDRB]$/ { strong[sym] = 1 }
    END { for (s in weak) if (s in strong) print s }' | sort >"$DUPS"
if [ -s "$DUPS" ]; then
    WORK=$(mktemp -d)
    MEMBERS=$("$GNU_AR" t "$LIBC_A" | grep '^compiler_builtins-')
    # shellcheck disable=SC2086
    (cd "$WORK" && "$GNU_AR" x "$LIBC_A" $MEMBERS)
    for member in $MEMBERS; do
        "$OBJCOPY" --localize-symbols="$DUPS" "$WORK/$member"
    done
    # shellcheck disable=SC2086
    (cd "$WORK" && "$GNU_AR" r "$LIBC_A" $MEMBERS)
    rm -rf "$WORK"
fi
echo "    $(wc -l <"$DUPS" | tr -d ' ') symbols"
rm -f "$DUPS"

for crt in crt0 crti crtn; do
    echo "==> $crt.o"
    # shellcheck disable=SC2086
    $CARGO rustc $STUBS $PROFILE_FLAG $UNSTABLE --target="$TARGET_SPEC" \
        --manifest-path "src/$crt/Cargo.toml" \
        -- --emit obj="$OUT/$crt.o" -C panic=abort
done

echo "==> ld_so.o"
# shellcheck disable=SC2086
$CARGO rustc $STUBS $PROFILE_FLAG $UNSTABLE --target="$TARGET_SPEC" \
    --manifest-path ld_so/Cargo.toml \
    -- --emit obj="$OUT/ld_so.o" -C panic=abort

echo "==> libc.so"
# As upstream builds it, libc.so is also relibc's dynamic linker: ld_so.o's
# _start is where procnto enters it when a program names it as its
# interpreter (PT_INTERP), and the programs and libraries it then loads find
# their DT_NEEDED libc.so.6 in it. Programs that run under ldqnx-64.so.2 are
# not relibc's. --no-undefined fails the link if anything is still looking
# for a symbol outside libc.so.
"$RUST_LLD" -flavor gnu -shared \
    -e _start \
    --dynamic-list=dynlst.txt \
    --eh-frame-hdr \
    --gc-sections \
    --sort-common \
    --no-undefined \
    --hash-style=both \
    --pack-dyn-relocs=none \
    -Bsymbolic-functions \
    -z now \
    -soname "$SONAME" \
    "$OUT/ld_so.o" --whole-archive "$OUT/libc.a" --no-whole-archive \
    -o "$OUT/libc.so"
ln -sf libc.so "$OUT/$SONAME"

# The QNX gcc specs file names the startup object crt1.o, not crt0.o. It is
# position-independent, so it also starts a PIE (Scrt1.o, as GCC names that).
cp "$OUT/crt0.o" "$OUT/crt1.o"
cp "$OUT/crt0.o" "$OUT/Scrt1.o"

# The start objects crt0 does not cover: a static PIE relocates itself
# (rcrt1.o), a profiled program starts gprof (gcrt1.o, mcrt1.o as QNX names
# it), and a profiled static PIE does both (grcrt1.o). crtbeginS.o, for
# everything position-independent, is the one GCC's bare-metal toolchain
# lacks. cxxabi.o is what C++ programs and libraries need besides GCC's
# libsupc++. crtbeginS.o has no unwind tables of its own, as its .eh_frame
# only marks where the next object's start.
for crt in rcrt1 gcrt1 grcrt1 crtbeginS cxxabi; do
    echo "==> $crt.o"
    CRT_FLAGS=""
    [ "$crt" = crtbeginS ] && CRT_FLAGS="-C force-unwind-tables=no"
    # shellcheck disable=SC2086
    $CARGO rustc --locked $PROFILE_FLAG $UNSTABLE --target="$TARGET_SPEC" \
        --manifest-path qnx/crt/Cargo.toml \
        -- --emit obj="$OUT/$crt.o" -C panic=abort --cfg "relibc_crt=\"$crt\"" $CRT_FLAGS
done
cp "$OUT/gcrt1.o" "$OUT/mcrt1.o"

# Lay out a sysroot in the shape the QNX gcc specs file expects, so that a
# cross-gcc can be pointed at it with -B/--sysroot:
#
#   %R/aarch64le/lib/crti.o crtbegin.o%s %R/aarch64le/lib/crt1.o
#   ... crtend.o%s %R/aarch64le/lib/crtn.o
#
# crtbegin/crtend come from the compiler, not from libc.
SYSROOT=${SYSROOT:-$OUT/sysroot}
LIBDIR="$SYSROOT/aarch64le/lib"
rm -rf "$LIBDIR"
mkdir -p "$LIBDIR" "$SYSROOT/usr/include"
CRTS="crt1.o Scrt1.o rcrt1.o gcrt1.o mcrt1.o grcrt1.o crtbeginS.o crti.o crtn.o cxxabi.o"
for f in libc.a libc.so $CRTS; do cp "$OUT/$f" "$LIBDIR/"; done
# g++ links -lstdc++, which this directory, searched first, answers with a
# linker script: the toolchain's own libstdc++ is built for newlib, so C++
# gets GCC's libsupc++ instead, after cxxabi.o with relibc's replacements
# for its single-threaded parts. -l: looks only in the library path.
cat >"$LIBDIR/libstdc++.a" <<'EOF'
/* relibc for QNX: C++ links GCC's libsupc++, not the bare-metal toolchain's
   libstdc++, which is built for newlib. cxxabi.o replaces the parts of
   libsupc++ that toolchain builds for a single thread. */
INPUT(-l:cxxabi.o -lsupc++)
EOF
# GCC's bare-metal libsupc++ is not position-independent: it keeps the
# vtables and type_info objects it relocates in .rodata, which in a PIE, a
# static PIE or a shared library would take text relocations. The sysroot's
# copy has them in .data.rel.ro, writable until relocated, as
# position-independent code keeps them.
SUPCXX=$(aarch64-none-elf-g++ -print-file-name=libsupc++.a 2>/dev/null || true)
if [ -f "$SUPCXX" ]; then
    GNU_READELF=$(relibc_need_gnu readelf)
    WORK=$(mktemp -d)
    (cd "$WORK" && "$GNU_AR" x "$SUPCXX")
    for obj in "$WORK"/*.o; do
        renames=""
        for sec in $("$GNU_READELF" -SW "$obj" | grep -oE '\.rela\.rodata[^ ]*' | sed 's/^\.rela//'); do
            renames="$renames --rename-section $sec=.data.rel.ro${sec#.rodata},alloc,load,data,contents"
        done
        # shellcheck disable=SC2086
        [ -z "$renames" ] || "$OBJCOPY" $renames "$obj"
    done
    rm -f "$LIBDIR/libsupc++.a"
    # shellcheck disable=SC2046
    (cd "$WORK" && "$GNU_AR" rcs "$LIBDIR/libsupc++.a" $("$GNU_AR" t "$SUPCXX"))
    rm -rf "$WORK"
    CXXRT="GCC's libsupc++, with cxxabi.o"
else
    CXXRT="none, as there is no aarch64-none-elf-g++"
fi
ln -sf libc.so "$LIBDIR/$SONAME"
# libc.a is position-independent, so it is also QNX's libcS.a, the archive
# for linking into a shared object.
cp "$OUT/libc.a" "$LIBDIR/libcS.a"
# What other systems split into libm, libdl, libpthread, librt, libutil and
# libcrypt, and QNX into libsocket, is all in libc; these are empty archives,
# with QNX's S names too, so that the -l options for them keep working.
if [ -x "$GNU_AR" ]; then
    for stub in libm libdl libpthread librt libsocket libutil libcrypt; do
        for name in "$stub.a" "${stub}S.a"; do
            rm -f "$LIBDIR/$name"
            "$GNU_AR" rcs "$LIBDIR/$name"
        done
    done
fi

# Headers as the upstream Makefile's `headers` target writes them with
# USE_RUST_LIBM — math.h from relibc's own math rather than openlibm's — but
# without that target's rebuild of relibc for the host's default features.
# Then the declarations of what the QNX backend provides ahead of relibc.
echo "==> headers"
INC="$SYSROOT/usr/include"
rm -rf "$INC"
mkdir -p "$INC"
cp -r include/* "$INC/"
# relibc's fenv.h is openlibm's, whose AArch64 implementation is all inline
# functions in its headers; they are needed even with math_libm.
cp openlibm/include/openlibm_fenv.h openlibm/include/openlibm_fenv_aarch64.h \
    openlibm/include/openlibm_defs.h openlibm/src/cdefs-compat.h "$INC/"
CBINDGEN=${CBINDGEN:-cbindgen}
if command -v "$CBINDGEN" >/dev/null 2>&1; then
    : > "$OUT/headers.log"
    for dir in src/header/*/; do
        header=$(basename "$dir")
        # Redox-only headers declare functions only Redox has.
        case "$header" in _* | redox_*) continue ;; esac
        [ -f "$dir/cbindgen.toml" ] || continue
        out="$INC/$(echo "$header" | sed 's/_/\//g').h"
        mkdir -p "$(dirname "$out")"
        cat "$dir/cbindgen.toml" cbindgen.globdefs.toml |
            "$CBINDGEN" "$dir/mod.rs" --config=/dev/stdin --output "$out" \
                >>"$OUT/headers.log" 2>&1 ||
            { echo "cbindgen failed for $header; see $OUT/headers.log" >&2; exit 1; }
    done
    (cd qnx/tools/qnx-headers && find . -name '*.h') | while read -r extra; do
        mkdir -p "$(dirname "$INC/$extra")"
        cat "qnx/tools/qnx-headers/$extra" >>"$INC/$extra"
    done
    HEADERS="generated by $CBINDGEN"
else
    HEADERS="only relibc's hand-written ones; install cbindgen for the rest"
fi

echo
echo "Built in $OUT:"
ls -l "$OUT/libc.a" "$OUT/libc.so" "$OUT/crt1.o" "$OUT/rcrt1.o" "$OUT/gcrt1.o" \
    "$OUT/grcrt1.o" "$OUT/crti.o" "$OUT/crtn.o" "$OUT/cxxabi.o"
echo
echo "Sysroot laid out in $SYSROOT"
echo "  headers: $HEADERS"
echo "  C++ runtime: $CXXRT"
