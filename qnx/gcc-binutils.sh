#!/bin/sh
# GNU binutils from an aarch64-none-elf GCC toolchain, such as the Arm GNU
# Toolchain. Its bin directory must be on PATH, or GCC_PREFIX must name the
# toolchain's root. Never installs anything.
#
# Usage:  . qnx/gcc-binutils.sh     (from the relibc root)

if [ -z "${GCC_BIN:-}" ] && [ -n "${GCC_PREFIX:-}" ] && [ -d "$GCC_PREFIX/bin" ]; then
    GCC_BIN=$GCC_PREFIX/bin
fi
if [ -n "${GCC_BIN:-}" ]; then
    PATH="$GCC_BIN:$PATH"
    export PATH
fi

relibc_need_gnu() {
    # $1 = tool base name (objcopy, nm, readelf, ar)
    pref=aarch64-none-elf-$1
    if command -v "$pref" >/dev/null 2>&1; then
        command -v "$pref"
        return 0
    fi
    echo "need $pref from an aarch64-none-elf GCC toolchain." >&2
    echo "  put its bin directory on PATH, or: export GCC_PREFIX=/path/to/toolchain" >&2
    return 1
}
