//! The QNX Neutrino ABI as relibc needs to speak it.
//!
//! Nothing in here issues kernel calls; it is purely the vocabulary — error
//! numbers, kernel types, message layouts and flag values — shared by the rest
//! of the backend and by both the 7.1 and 8.0 version modules.
//!
//! The struct layouts come from the DWARF in the SDP's `libc.so.5.sym`, read
//! with `qnx/tools/qnx-dwarf-types.py`. The flag values are macros, which DWARF
//! cannot carry, so they were taken from the SDP headers and cross-checked
//! against the vendor libc's disassembly wherever a call site pinned them down.

pub mod devctl;
pub mod errno;
pub mod iomsg;
pub mod mman;
pub mod neutrino;
pub mod oflag;
pub mod signal;
pub mod socket;
pub mod termios;
