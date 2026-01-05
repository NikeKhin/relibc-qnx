//! Second connections to an existing server: `dup` and `_sopenfd`.
//!
//! These are the two io messages whose layout changed in QNX 8.0 — `_io_dup`
//! and `_io_openfd` carry a `_msg_info32` plus a key on 7.1, and a
//! `_msg_info_hdr` plus a `dupfd_handle_t` on 8.0 — so each backend has its
//! own copy of this file. `io` re-exports both functions.

use core::ptr;

use super::{
    abi::{
        iomsg::{IoDup, IoOpenfd, MsgInfo32, msg},
        neutrino::MsgInfo,
        oflag::{self, o},
    },
    kernel::{connect_attach, connect_detach, connect_server_info, msg_send},
};
use crate::{error::Result, platform::types::*};

/// Opens a second connection to whatever server backs `fd`.
///
/// A descriptor is a connection, so duplicating one means attaching a fresh
/// connection to the same channel and then telling the server about it with
/// `_IO_DUP` so that its open-control-block reference count stays right. The
/// caller chooses the lowest acceptable descriptor number, which is what
/// separates `dup` from `dup2`.
pub fn dup_to(fd: c_int, lowest: c_int) -> Result<c_int> {
    let mut server = MsgInfo::default();
    // Learn which node, process and channel the existing connection reaches.
    unsafe { connect_server_info(0, fd, &mut server) }?;

    let new_fd = connect_attach(server.nd, server.pid, server.chid, lowest, 0)?;

    let request = IoDup {
        type_: msg::DUP,
        combine_len: size_of::<IoDup>() as u16,
        info: MsgInfo32 {
            nd: server.nd,
            pid: super::tls::pid(),
            tid: super::tls::tid(),
            chid: server.chid,
            scoid: server.scoid,
            coid: new_fd,
            ..MsgInfo32::default()
        },
        reserved: 0,
        key: 0,
    };
    match unsafe {
        msg_send(
            new_fd,
            ptr::from_ref(&request).cast::<u8>(),
            size_of::<IoDup>(),
            ptr::null_mut(),
            0,
        )
    } {
        Ok(_) => Ok(new_fd),
        Err(err) => {
            let _ = connect_detach(new_fd);
            Err(err)
        }
    }
}

/// `_sopenfd`: a second connection to the server behind `fd`, of type `xtype`.
///
/// Used for the write end of a pipe, `socketpair`, and `accept`.
pub fn openfd(fd: c_int, oflag: c_int, xtype: u16) -> Result<c_int> {
    let mut server = MsgInfo::default();
    unsafe { connect_server_info(0, fd, &mut server) }?;

    let qnx_oflag = oflag::to_qnx(oflag);
    let attach_flags = if qnx_oflag & o::CLOEXEC != 0 {
        super::abi::neutrino::cof::CLOEXEC
    } else {
        0
    };
    let new_fd = connect_attach(server.nd, server.pid, server.chid, 0, attach_flags)?;

    let ioflag = (qnx_oflag.wrapping_add(1) & 7) | (qnx_oflag & !0x2007) | o::LARGEFILE;
    let request = IoOpenfd {
        type_: msg::OPENFD,
        combine_len: size_of::<IoOpenfd>() as u16,
        ioflag,
        sflag: super::abi::oflag::sh::DENYNO,
        xtype,
        info: MsgInfo32 {
            nd: server.nd,
            pid: super::tls::pid(),
            tid: super::tls::tid(),
            chid: server.chid,
            scoid: server.scoid,
            coid: new_fd,
            ..MsgInfo32::default()
        },
        reserved2: 0,
        key: 0,
    };
    match unsafe {
        msg_send(
            new_fd,
            ptr::from_ref(&request).cast::<u8>(),
            size_of::<IoOpenfd>(),
            ptr::null_mut(),
            0,
        )
    } {
        Ok(_) => Ok(new_fd),
        Err(err) => {
            let _ = connect_detach(new_fd);
            Err(err)
        }
    }
}
