//! QNX `open()` flags and the `_io_connect::ioflag` encoding.
//!
//! These are not the Linux values relibc's `fcntl.h` uses, so every path that
//! reaches a resource manager has to translate. Two of them explain the magic
//! constants in the vendor `openat`, which sets `ioflag |= 0x00008000` for an
//! ordinary open and `ioflag |= 0x00208000` when resolving against a directory
//! descriptor: those bits are `O_LARGEFILE` and `O_DIRFD`.

use crate::{
    header::fcntl as re,
    platform::types::{c_int, mode_t},
};

/// QNX `O_*` values.
#[allow(dead_code)]
pub mod o {
    pub const RDONLY: u32 = 0o0;
    pub const WRONLY: u32 = 0o1;
    pub const RDWR: u32 = 0o2;
    pub const EXEC: u32 = 0o3;
    pub const ACCMODE: u32 = 0o7;
    pub const APPEND: u32 = 0o10;
    pub const DSYNC: u32 = 0o20;
    pub const SYNC: u32 = 0o40;
    pub const RSYNC: u32 = 0o100;
    pub const NONBLOCK: u32 = 0o200;
    pub const CREAT: u32 = 0o400;
    pub const TRUNC: u32 = 0o1000;
    pub const EXCL: u32 = 0o2000;
    pub const NOCTTY: u32 = 0o4000;
    pub const NOFOLLOW: u32 = 0o10000;
    pub const CLOEXEC: u32 = 0o20000;
    pub const REALIDS: u32 = 0o40000;
    pub const LARGEFILE: u32 = 0o100000;
    pub const ASYNC: u32 = 0o200000;
    pub const NOSYMLINK: u32 = 0o400000;
    pub const ANON: u32 = 0o1000000;
    /// Perform an access check and close the descriptor again; this is how
    /// `access()`/`faccessat()` are implemented on QNX.
    pub const PERMCHECK: u32 = 0o2000000;
    pub const DIRECTORY: u32 = 0o4000000;
    /// The `dirfd` field of `_io_connect` is meaningful.
    pub const DIRFD: u32 = 0o10000000;
}

/// `_io_connect::ioflag` permission bits, which occupy the low three bits that
/// `O_ACCMODE` would otherwise use.
#[allow(dead_code)]
pub mod io_flag {
    pub const RD: u32 = 0x1;
    pub const WR: u32 = 0x2;
    pub const EX: u32 = 0x4;
    pub const MASK: u32 = 0x7;
}

/// `sflag` share modes from `share.h`.
#[allow(dead_code)]
pub mod sh {
    pub const COMPAT: u16 = 0x00;
    pub const DENYRW: u16 = 0x10;
    pub const DENYWR: u16 = 0x20;
    pub const DENYRD: u16 = 0x30;
    pub const DENYNO: u16 = 0x40;
    pub const MASK: u16 = 0x70;
}

/// Translates relibc's `O_*` bits into QNX's.
pub fn to_qnx(oflag: c_int) -> u32 {
    let oflag = oflag as u32;
    let mut out = match oflag & (re::O_ACCMODE as u32) {
        0 => o::RDONLY,
        1 => o::WRONLY,
        _ => o::RDWR,
    };
    let map = [
        (re::O_APPEND, o::APPEND),
        (re::O_NONBLOCK, o::NONBLOCK),
        (re::O_CREAT, o::CREAT),
        (re::O_TRUNC, o::TRUNC),
        (re::O_EXCL, o::EXCL),
        (re::O_NOCTTY, o::NOCTTY),
        (re::O_CLOEXEC, o::CLOEXEC),
        (re::O_DIRECTORY, o::DIRECTORY),
        (re::O_NOFOLLOW, o::NOFOLLOW),
    ];
    for (relibc_bit, qnx_bit) in map {
        if oflag & (relibc_bit as u32) != 0 {
            out |= qnx_bit;
        }
    }
    // Every open from a 64-bit process is a large-file open, which is why the
    // vendor libc unconditionally sets this bit.
    out | o::LARGEFILE
}

/// Builds the `ioflag` field from QNX open flags.
///
/// The low three bits carry the permission mask rather than the access mode:
/// `O_RDONLY` becomes `_IO_FLAG_RD`, `O_WRONLY` becomes `_IO_FLAG_WR`,
/// `O_RDWR` becomes both, and `O_EXEC` becomes `_IO_FLAG_EX` — which is exactly
/// `(oflag & O_ACCMODE) + 1`.
pub fn to_ioflag(qnx_oflag: u32) -> u32 {
    let access = (qnx_oflag & o::ACCMODE) + 1;
    (qnx_oflag & !io_flag::MASK) | (access & io_flag::MASK)
}

/// Translates QNX `O_*` bits back into relibc's. Kernel-only bits such as
/// `O_LARGEFILE`, `O_ANON` and `O_DIRFD` are dropped so they never leak into
/// `F_GETFL`.
pub fn from_qnx(qnx_oflag: u32) -> c_int {
    let acc = qnx_oflag & o::ACCMODE;
    let mut out = match acc {
        0 => re::O_RDONLY,
        1 => re::O_WRONLY,
        _ => re::O_RDWR,
    } as u32;
    let map = [
        (o::APPEND, re::O_APPEND),
        (o::NONBLOCK, re::O_NONBLOCK),
        (o::CREAT, re::O_CREAT),
        (o::TRUNC, re::O_TRUNC),
        (o::EXCL, re::O_EXCL),
        (o::NOCTTY, re::O_NOCTTY),
        (o::CLOEXEC, re::O_CLOEXEC),
        (o::DIRECTORY, re::O_DIRECTORY),
        (o::NOFOLLOW, re::O_NOFOLLOW),
    ];
    for (qnx_bit, relibc_bit) in map {
        if qnx_oflag & qnx_bit != 0 {
            out |= relibc_bit as u32;
        }
    }
    out as c_int
}

/// The `access` field of `_io_connect`.
///
/// The vendor `openat` stores 7 here for every open, which the disassembly
/// shows as the constant `0x00070040` covering `sflag` and `access` together.
pub const ACCESS_ALL: u16 = 0o7;

/// Translates a relibc `mode_t` into QNX's.
///
/// The `S_I*` permission bits and `S_IF*` type bits agree between the two, so
/// this is the identity — it exists to mark the places where a mode crosses the
/// kernel boundary.
pub fn mode_to_qnx(mode: mode_t) -> u32 {
    mode as u32
}
