//! `ioctl`.
//!
//! relibc issues its terminal requests with Linux's numbers and structures:
//! its termios, `sys_ioctl`, pty and unistd code take the Linux path on QNX
//! too. Those become the terminal `devctl`s, with the attributes translated
//! by `abi::termios`. Any other Linux terminal request is refused with
//! `ENOTTY`; everything else is passed through as a `devctl` whose command
//! word declares its own payload size.

use core::{mem::size_of, ptr};

use super::{
    abi::{
        devctl as dc,
        termios::{self as qt, Termios, chr, tioc},
    },
    io, time,
};
use crate::{
    error::{Errno, Result},
    header::{errno::ENOTTY, fcntl::O_NONBLOCK, sys_ioctl as re, termios::termios, time::timespec},
    platform::types::*,
};

/// `devctl` with a payload of `len` bytes at `buf`; returns `dev_info`.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes in whichever direction `cmd` implies.
unsafe fn dcmd(fd: c_int, cmd: u32, buf: *mut c_void, len: usize) -> Result<c_int> {
    let mut info = 0;
    unsafe { io::devctl(fd, cmd, buf.cast::<u8>(), len, Some(&mut info)) }?;
    Ok(info as c_int)
}

/// `devctl` passing one `int` by value.
fn dcmd_int(fd: c_int, cmd: u32, value: c_int) -> Result<c_int> {
    let mut value = value;
    // SAFETY: `value` is a live c_int.
    unsafe { dcmd(fd, cmd, ptr::from_mut(&mut value).cast(), size_of::<c_int>()) }
}

fn get_attr(fd: c_int) -> Result<Termios> {
    let mut attr = Termios::ZERO;
    // SAFETY: `attr` is a live `struct termios`.
    unsafe { dcmd(fd, chr::TCGETATTR, ptr::from_mut(&mut attr).cast(), size_of::<Termios>()) }?;
    Ok(attr)
}

/// Whether `cmd` is one of Linux's terminal requests (type `'T'`).
fn is_linux_tty(cmd: c_ulong) -> bool {
    cmd & 0xff00 == 0x5400
}

/// # Safety
///
/// `buf` must be what `cmd` expects: for the translated requests, the Linux
/// argument; otherwise valid for the size the command word declares.
pub unsafe fn ioctl(fd: c_int, cmd: c_ulong, buf: *mut c_void) -> Result<c_int> {
    // The requests below that take an int pointer or a struct pointer are
    // handed the caller's buffer, whose layout is the same on both sides.
    match cmd {
        re::TCGETS => {
            let attr = get_attr(fd)?;
            unsafe { buf.cast::<termios>().write_unaligned(qt::from_kernel(&attr)) };
            Ok(0)
        }
        re::TCSETS | re::TCSETSW | re::TCSETSF => {
            // SAFETY: tcsetattr passes a `struct termios`.
            let new = unsafe { buf.cast::<termios>().read_unaligned() };
            let mut attr = get_attr(fd)?;
            qt::apply(&new, &mut attr)?;
            let set = match cmd {
                re::TCSETS => chr::TCSETATTR,
                re::TCSETSW => chr::TCSETATTRD,
                _ => chr::TCSETATTRF,
            };
            unsafe { dcmd(fd, set, ptr::from_mut(&mut attr).cast(), size_of::<Termios>()) }?;
            Ok(0)
        }
        // The argument is the selector itself; TC*FLUSH and TCOOFF..TCION
        // are numbered alike.
        re::TCFLSH => dcmd_int(fd, chr::TCFLUSH, buf as usize as c_int).map(|_| 0),
        re::TCXONC => dcmd_int(fd, chr::TCFLOW, buf as usize as c_int).map(|_| 0),
        // A zero argument sends a break; anything else waits for the output
        // to drain.
        re::TCSBRK if buf.is_null() => {
            unsafe { dcmd(fd, tioc::SBRK, ptr::null_mut(), 0) }?;
            let quarter = timespec {
                tv_sec: 0,
                tv_nsec: 250_000_000,
            };
            let _ = unsafe { time::nanosleep(&quarter, ptr::null_mut()) };
            unsafe { dcmd(fd, tioc::CBRK, ptr::null_mut(), 0) }.map(|_| 0)
        }
        re::TCSBRK => unsafe { dcmd(fd, chr::TCDRAIN, ptr::null_mut(), 0) }.map(|_| 0),
        re::TIOCGWINSZ => unsafe { dcmd(fd, chr::GETSIZE, buf, 8) }.map(|_| 0),
        re::TIOCSWINSZ => unsafe { dcmd(fd, chr::SETSIZE, buf, 8) }.map(|_| 0),
        re::TIOCGPGRP => unsafe { dcmd(fd, chr::TCGETPGRP, buf, size_of::<pid_t>()) }.map(|_| 0),
        re::TIOCSPGRP => unsafe { dcmd(fd, chr::TCSETPGRP, buf, size_of::<pid_t>()) }.map(|_| 0),
        re::TIOCGSID => unsafe { dcmd(fd, chr::TCGETSID, buf, size_of::<pid_t>()) }.map(|_| 0),
        re::TIOCSCTTY => unsafe { dcmd(fd, tioc::SCTTY, ptr::null_mut(), 0) }.map(|_| 0),
        re::TIOCNOTTY => unsafe { dcmd(fd, tioc::NOTTY, ptr::null_mut(), 0) }.map(|_| 0),
        re::TIOCOUTQ => unsafe { dcmd(fd, chr::OSCHARS, buf, size_of::<c_int>()) }.map(|_| 0),
        re::FIONREAD => unsafe { dcmd(fd, chr::ISCHARS, buf, size_of::<c_int>()) }.map(|_| 0),
        re::FIONBIO => {
            // SAFETY: FIONBIO passes an int.
            let on = unsafe { buf.cast::<c_int>().read_unaligned() } != 0;
            let flags = io::getfl(fd)?;
            io::setfl(fd, if on { flags | O_NONBLOCK } else { flags & !O_NONBLOCK })?;
            Ok(0)
        }
        _ if is_linux_tty(cmd) => Err(Errno(ENOTTY)),
        _ => {
            let cmd = u32::try_from(cmd).map_err(|_| Errno(ENOTTY))?;
            unsafe { dcmd(fd, cmd, buf, dc::cmd_size(cmd)) }
        }
    }
}
