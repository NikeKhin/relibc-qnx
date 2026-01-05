//! Second connections to an existing server: `dup` and `_sopenfd`, QNX 8.0.
//!
//! 8.0 changed both messages. `_io_dup` (40 bytes) and `_io_openfd` (48 bytes)
//! now carry a 28-byte `_msg_info_hdr` and a `dupfd_handle_t` where 7.1 had a
//! `_msg_info32` and a key. The sequences below are the vendor `fcntl`
//! (`F_DUPFD`) and `_sopenfd`, instruction for instruction:
//!
//! 1. `ConnectServerInfo` on the existing descriptor.
//! 2. `ConnectAttach` to the same channel with `_NTO_COF_CLOEXEC` set, so the
//!    new descriptor cannot leak across a concurrent `exec`, plus
//!    `_NTO_COF_NOSHARE` where the vendor adds it.
//! 3. Send the io message. The header names the *original* descriptor in
//!    `coid`, with this process's pid and the server's chid/scoid; the handle
//!    is zero.
//! 4. Clear `_NTO_COF_CLOEXEC` again unless the caller asked for it.
//!
//! `ConnectDupFd` and `dupfd_handle_t` handles are for passing descriptors
//! between processes and are not needed here.

use core::ptr;

use super::{
    abi::{
        iomsg::{msg, openfd as openfd_xtype},
        neutrino::{MsgInfo, cof},
        oflag::{self, o, sh},
    },
    kernel::{connect_attach, connect_detach, connect_flags, connect_server_info, msg_send_nc},
    tls,
};
use crate::{error::Result, platform::types::*};

/// `struct _msg_info_hdr` (28 bytes), the leading fields of `_msg_info64`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MsgInfoHdr {
    nd: u32,
    srcnd: u32,
    pid: pid_t,
    tid: i32,
    chid: i32,
    scoid: i32,
    coid: i32,
}

/// `struct _io_dup` (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoDup {
    type_: u16,
    combine_len: u16,
    info: MsgInfoHdr,
    handle: u64,
}

/// `struct _io_openfd` (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoOpenfd {
    type_: u16,
    combine_len: u16,
    ioflag: u32,
    sflag: u16,
    xtype: u16,
    info: MsgInfoHdr,
    handle: u64,
}

/// The header both messages carry: who is asking, and which of its
/// connections the new one duplicates.
fn origin(server: &MsgInfo, fd: c_int) -> MsgInfoHdr {
    MsgInfoHdr {
        pid: tls::pid(),
        chid: server.chid,
        scoid: server.scoid,
        coid: fd,
        ..MsgInfoHdr::default()
    }
}

/// Sends `request` on `new_fd`, then drops the provisional close-on-exec flag
/// unless `keep_cloexec`. The descriptor is detached again on failure.
fn finish<T>(new_fd: c_int, request: &T, keep_cloexec: bool) -> Result<c_int> {
    let sent = unsafe {
        msg_send_nc(
            new_fd,
            ptr::from_ref(request).cast::<u8>(),
            size_of::<T>(),
            ptr::null_mut(),
            0,
        )
    };
    if let Err(err) = sent {
        let _ = connect_detach(new_fd);
        return Err(err);
    }
    if !keep_cloexec {
        connect_flags(0, new_fd, cof::CLOEXEC, 0)?;
    }
    Ok(new_fd)
}

/// Opens a second connection to whatever server backs `fd`, at the lowest
/// free descriptor not below `lowest` (`F_DUPFD`). The result is not
/// close-on-exec.
pub fn dup_to(fd: c_int, lowest: c_int) -> Result<c_int> {
    let mut server = MsgInfo::default();
    unsafe { connect_server_info(0, fd, &mut server) }?;

    // The vendor copies bit 0x40 of the server's flags straight through, which
    // is where `_NTO_COF_NOSHARE` lives in the attach flags.
    let share = (server.flags as u16 as c_uint) & cof::NOSHARE;
    let new_fd = connect_attach(
        server.nd,
        server.pid,
        server.chid,
        lowest,
        cof::CLOEXEC | share,
    )?;

    let request = IoDup {
        type_: msg::DUP,
        combine_len: size_of::<IoDup>() as u16,
        info: origin(&server, fd),
        handle: 0,
    };
    finish(new_fd, &request, false)
}

/// `_sopenfd`: a second connection to the server behind `fd`, of type `xtype`.
///
/// Used for the write end of a pipe, `socketpair`, and `accept`.
pub fn openfd(fd: c_int, oflag: c_int, xtype: u16) -> Result<c_int> {
    let mut server = MsgInfo::default();
    unsafe { connect_server_info(0, fd, &mut server) }?;

    let qnx_oflag = oflag::to_qnx(oflag);
    // The vendor attaches pipes, and servers that set bit 0x400 in their info
    // flags, with `_NTO_COF_NOSHARE`.
    let noshare = xtype == openfd_xtype::PIPE || ((server.flags as u16) & 0x400) != 0;
    let attach_flags = cof::CLOEXEC | if noshare { cof::NOSHARE } else { 0 };
    let new_fd = connect_attach(server.nd, server.pid, server.chid, 0, attach_flags)?;

    let ioflag = (qnx_oflag.wrapping_add(1) & 7) | (qnx_oflag & !0x2007) | o::LARGEFILE;
    let request = IoOpenfd {
        type_: msg::OPENFD,
        combine_len: size_of::<IoOpenfd>() as u16,
        ioflag,
        sflag: sh::DENYNO,
        xtype,
        info: origin(&server, fd),
        handle: 0,
    };
    finish(new_fd, &request, (qnx_oflag & o::CLOEXEC) != 0)
}
