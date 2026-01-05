//! `ptrace`.
//!
//! Neutrino debugging goes through `/proc/<pid>` and `devctl()` rather than a
//! `ptrace` system call, so there is nothing to forward this to.

use super::Sys;
use crate::{
    error::{Errno, Result},
    header::errno::ENOSYS,
    platform::{PalPtrace, types::*},
};

impl PalPtrace for Sys {
    unsafe fn ptrace(
        _request: c_int,
        _pid: pid_t,
        _addr: *mut c_void,
        _data: *mut c_void,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}
