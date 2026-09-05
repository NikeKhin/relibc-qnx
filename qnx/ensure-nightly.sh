#!/bin/sh
# Make sure cargo/rustc are the pin in rust-toolchain.toml.
# A cargo installed by a package manager may come first on PATH, and it
# silently ignores that file.
#
# Usage:  . qnx/ensure-nightly.sh     (from the relibc root)

if [ -f rust-toolchain.toml ]; then
    PINNED=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml | head -1)
fi
PINNED=${PINNED:-nightly-2026-05-24}

if ! command -v rustup >/dev/null 2>&1; then
    echo "This build needs rustup with $PINNED (stable cargo cannot pass -Z)." >&2
    echo "  # and put rustup's bin directory first in PATH" >&2
    exit 1
fi

# Do not rustup-install or component-add. The pin must already be present.
if ! rustup which --toolchain "$PINNED" cargo >/dev/null 2>&1; then
    echo "Need $PINNED already installed; this script will not install it." >&2
    echo "  rustup toolchain install $PINNED --component rust-src" >&2
    echo "  # and put rustup's bin directory first in PATH" >&2
    exit 1
fi
NIGHTLY_BIN=$(dirname "$(rustup which --toolchain "$PINNED" cargo)")
PATH="$NIGHTLY_BIN:$PATH"
export PATH
CARGO=${CARGO:-cargo}

ver=$($CARGO --version 2>/dev/null || true)
case "$ver" in
    *nightly*) ;;
    *)
        echo "cargo is not nightly ($ver)." >&2
        echo "which cargo: $(command -v cargo)" >&2
        echo "Need $PINNED. PATH must start with rustup's bin:" >&2
        echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
        echo "  rustup toolchain install $PINNED --component rust-src" >&2
        echo "  rustup show" >&2
        exit 1
        ;;
esac
