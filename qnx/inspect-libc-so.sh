#!/bin/sh
# Dump the dynamic relocations of libc.so, which is relibc's dynamic linker
# too, and fail on any kind that linker does not apply.
# Uses readelf and nm from the aarch64-none-elf GCC toolchain. Does not
# install anything.
set -eu
# Run from the relibc root, whatever the caller's directory.
cd "$(dirname "$0")/.."
# shellcheck disable=SC1091
. qnx/gcc-binutils.sh

# QNX 7.1 by default; QNX 8.0 with
#   TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/inspect-libc-so.sh
# or name any libc.so as the argument.
TARGET_SPEC=${TARGET_SPEC:-targets/aarch64-unknown-nto-qnx710.json}
TARGET=$(basename "$TARGET_SPEC" .json)
SO=${1:-${CARGO_TARGET_DIR:-target}/$TARGET/${PROFILE:-release}/libc.so}
[ -f "$SO" ] || { echo "missing $SO; run qnx/build-qnx.sh first" >&2; exit 1; }

READELF=$(relibc_need_gnu readelf)
NM=$(relibc_need_gnu nm)

echo "==> DT_NEEDED / flags"
"$READELF" -dW "$SO" | grep -E 'NEEDED|SONAME|FLAGS|FLAGS_1|REL|JMPREL|PLT|TEXTREL|BIND_NOW' || true
echo
echo "==> undefined dynsyms"
"$NM" -D "$SO" 2>/dev/null | awk '$1=="U"{print}' || true
echo
echo "==> non-RELATIVE relocs"
"$READELF" -rW "$SO" | grep -vE 'Relocation section|Offset|RELATIVE|^$' || true
echo
echo "==> PT_TLS / INTERP / GNU_EH_FRAME"
"$READELF" -lW "$SO" | grep -E 'TLS|INTERP|GNU_EH_FRAME|RELRO' || true
echo
# The kinds relibc's dynamic linker applies (src/ld_so/dso.rs); libc.so
# relocates itself with it, and it is the only loader of relibc programs.
supported='NONE|RELATIVE|GLOB_DAT|JUMP_SLOT|ABS64|COPY|IRELATIVE|TLS_DTPMOD(64)?|TLS_DTPREL(64)?|TLS_TPREL(64)?|TLSDESC'
bad=$("$READELF" -rW "$SO" | grep -oE 'R_AARCH64_[A-Z0-9_]+' | grep -cvE "^R_AARCH64_($supported)\$" || true)
: "${bad:=0}"
if [ "$bad" -gt 0 ]; then
    echo "FAIL: $bad relocation(s) relibc's dynamic linker does not apply" >&2
    exit 1
fi
echo "OK: every relocation is one relibc's dynamic linker applies"
