//! `epoll`.
//!
//! `epoll` is a Linux interface with no Neutrino equivalent. The native
//! mechanism is a channel plus `MsgReceive`, or `ionotify`/`select` on top of
//! it, and mapping `epoll` onto that is a design decision for whoever needs it
//! rather than something to guess at here.

use super::Sys;
use crate::{
    error::{Errno, Result},
    header::{bits_sigset_t::sigset_t, errno::ENOSYS, sys_epoll::epoll_event},
    platform::{PalEpoll, types::c_int},
};

impl PalEpoll for Sys {
    fn epoll_create1(_flags: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn epoll_ctl(
        _epfd: c_int,
        _op: c_int,
        _fd: c_int,
        _event: Option<&mut epoll_event>,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn epoll_pwait(
        _epfd: c_int,
        _events: &mut [epoll_event],
        _timeout: c_int,
        _sigmask: Option<&sigset_t>,
    ) -> Result<usize> {
        Err(Errno(ENOSYS))
    }
}
