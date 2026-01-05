//! QNX memory-mapping flags.
//!
//! Neutrino's `PROT_*` bits live in a different range from Linux's (0x100
//! upwards rather than 1, 2, 4) and its `MAP_*` values differ too, so relibc's
//! own flags are translated rather than passed through.

use crate::{header::sys_mman as re, platform::types::c_int};

/// QNX `PROT_*` values.
#[allow(dead_code)]
pub mod prot {
    pub const NONE: u32 = 0x0000_0000;
    pub const READ: u32 = 0x0000_0100;
    pub const WRITE: u32 = 0x0000_0200;
    pub const EXEC: u32 = 0x0000_0400;
    pub const NOCACHE: u32 = 0x0000_0800;
    pub const MASK: u32 = 0x0000_0f00;
}

/// QNX `MAP_*` values.
#[allow(dead_code)]
pub mod map {
    pub const SHARED: u32 = 0x0000_0001;
    pub const PRIVATE: u32 = 0x0000_0002;
    pub const FIXED: u32 = 0x0000_0010;
    pub const ELF: u32 = 0x0000_0020;
    pub const LAZY: u32 = 0x0000_0080;
    pub const STACK: u32 = 0x0000_1000;
    pub const BELOW: u32 = 0x0000_2000;
    pub const NOINIT: u32 = 0x0000_4000;
    pub const PHYS: u32 = 0x0001_0000;
    pub const NOINHERIT: u32 = 0x0002_0000;
    pub const HANDLE: u32 = 0x0004_0000;
    pub const ANON: u32 = 0x0008_0000;
    pub const SYSRAM: u32 = 0x0100_0000;
    pub const TYPE: u32 = PRIVATE | SHARED;
}

/// QNX `MS_*` values for `msync`.
#[allow(dead_code)]
pub mod ms {
    pub const ASYNC: u32 = 0x000_0001;
    pub const SYNC: u32 = 0x000_0002;
    pub const INVALIDATE: u32 = 0x000_0004;
    pub const INVALIDATE_ICACHE: u32 = 0x0100_0000;
    pub const CACHE_ONLY: u32 = 0x0200_0000;
}

/// QNX `MCL_*` values for `mlockall`.
#[allow(dead_code)]
pub mod mcl {
    pub const CURRENT: u32 = 0x0000_0001;
    pub const FUTURE: u32 = 0x0000_0002;
}

/// QNX `POSIX_MADV_*` values.
#[allow(dead_code)]
pub mod madv {
    pub const NORMAL: u32 = 0;
    pub const SEQUENTIAL: u32 = 1;
    pub const RANDOM: u32 = 2;
    pub const WILLNEED: u32 = 3;
    pub const DONTNEED: u32 = 4;
}

/// Translates relibc's `PROT_*` bits into QNX's.
pub fn to_qnx_prot(value: c_int) -> u32 {
    let value = value as u32;
    let mut out = prot::NONE;
    if value & (re::PROT_READ as u32) != 0 {
        out |= prot::READ;
    }
    if value & (re::PROT_WRITE as u32) != 0 {
        out |= prot::WRITE;
    }
    if value & (re::PROT_EXEC as u32) != 0 {
        out |= prot::EXEC;
    }
    out
}

/// Translates relibc's `MAP_*` bits into QNX's.
pub fn to_qnx_map(value: c_int) -> u32 {
    let value = value as u32;
    let mut out = 0;
    let pairs = [
        (re::MAP_SHARED as u32, map::SHARED),
        (re::MAP_PRIVATE as u32, map::PRIVATE),
        (re::MAP_FIXED as u32, map::FIXED),
        (re::MAP_ANONYMOUS as u32, map::ANON),
    ];
    for (relibc_bit, qnx_bit) in pairs {
        if value & relibc_bit != 0 {
            out |= qnx_bit;
        }
    }
    // MAP_SYSRAM is only meaningful alongside MAP_PHYS, where it selects the
    // pool physical memory comes from. Setting it on an ordinary anonymous
    // mapping makes procnto reply with an address that is not backed, so a
    // plain MAP_ANON | MAP_PRIVATE is what a POSIX mapping sends.
    out
}

/// Translates relibc's `MS_*` bits into QNX's.
pub fn to_qnx_msync(value: c_int) -> u32 {
    let value = value as u32;
    let mut out = 0;
    if value & (re::MS_ASYNC as u32) != 0 {
        out |= ms::ASYNC;
    }
    if value & (re::MS_SYNC as u32) != 0 {
        out |= ms::SYNC;
    }
    if value & (re::MS_INVALIDATE as u32) != 0 {
        out |= ms::INVALIDATE;
    }
    if value & (re::MS_INVALIDATE_ICACHE as u32) != 0 {
        out |= ms::INVALIDATE_ICACHE;
    }
    out
}
