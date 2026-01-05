//! Path resolution and the `_IO_CONNECT` protocol.
//!
//! Neutrino has no kernel filesystem. A path is resolved by asking procnto's
//! path manager which resource managers have registered a matching mountpoint,
//! then replaying the request against each candidate until one accepts it:
//!
//! 1. Send `_IO_CONNECT` to [`PATHMGR_COID`], which is connection zero on the
//!    kernel's side channel and exists from process creation.
//! 2. procnto answers with a *negative* status carrying `_IO_CONNECT_RET_LINK`
//!    and a reply body of [`IoConnectLinkReply`], `nentries` records of
//!    [`IoConnectEntry`], and the resolved path. An `nentries` of zero means the
//!    path is a symbolic link and resolution restarts with the returned path.
//! 3. For each entry, `ConnectAttach` to that manager and re-send the connect
//!    with the entry's `handle` and the part of the path its mountpoint did not
//!    consume. Ordinary io messages never return a negative status, so a
//!    non-negative reply means the manager accepted the request.
//!
//! For `_IO_CONNECT_OPEN` the connection reached that way *is* the file
//! descriptor. The other subtypes are one-shot operations and the connection is
//! dropped again.

use core::{mem::MaybeUninit, ptr};

use super::{
    abi::{
        iomsg::{
            IoConnect, IoConnectEntry, IoConnectLinkReply, connect_extra, connect_ret,
            connect_subtype, eflag, ftype, msg, path,
        },
        neutrino::{IoVec, PATHMGR_COID, SYSMGR_HANDLE, cof},
        oflag::{self, o, sh},
    },
    kernel::{connect_attach, connect_detach, msg_sendv},
};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        errno::{EINVAL, ELOOP, ENAMETOOLONG, ENOENT, ENOSYS},
        fcntl,
    },
    platform::types::*,
};

/// Longest path relibc will hand to the path manager.
pub(crate) const PATH_MAX: usize = 1024;

/// How many candidate managers to ask procnto for in one round trip.
const ENTRY_MAX: usize = 8;

/// How many links to follow before giving up, mirroring `SYMLOOP_MAX`.
const SYMLOOP_MAX: u32 = 8;

/// Reply buffer for a connect: header, candidate table, then the path.
///
/// procnto packs the reply as `[header][nentries entries][path]`. The `entries`
/// array is sized for the maximum we ask for so the receive buffer is large
/// enough; the path does **not** live at a fixed offset after `ENTRY_MAX`
/// slots, which is why [`LinkReply::resolved_path`] computes it from
/// `nentries`.
#[repr(C)]
struct LinkReply {
    header: IoConnectLinkReply,
    entries: [IoConnectEntry; ENTRY_MAX],
    /// Tail storage; the real path starts after `nentries` records, not here.
    _path: [u8; PATH_MAX],
}

impl LinkReply {
    fn zeroed() -> Self {
        // SAFETY: every field is a plain integer type for which an all-zero
        // bit pattern is valid.
        unsafe { MaybeUninit::<Self>::zeroed().assume_init() }
    }

    fn nentries(&self) -> usize {
        usize::from(self.header.nentries).min(ENTRY_MAX)
    }

    fn candidates(&self) -> &[IoConnectEntry] {
        &self.entries[..self.nentries()]
    }

    /// Path as packed by procnto: immediately after the `nentries` records.
    fn resolved_path(&self) -> &[u8] {
        let off = size_of::<IoConnectLinkReply>() + self.nentries() * size_of::<IoConnectEntry>();
        let len = usize::from(self.header.path_len).min(PATH_MAX);
        let total = size_of::<Self>();
        if off >= total || len == 0 {
            return &[];
        }
        let len = len.min(total - off);
        // SAFETY: `off..off+len` is inside this receive buffer.
        unsafe { core::slice::from_raw_parts(ptr::from_ref(self).cast::<u8>().add(off), len) }
    }
}

/// QNX `_io_connect::dirfd` value meaning "no directory fd" (`(uint16_t)-1`).
const QNX_DIRFD_NONE: u16 = 0xffff;

fn qnx_dirfd(dirfd: c_int) -> u16 {
    if dirfd == fcntl::AT_FDCWD {
        QNX_DIRFD_NONE
    } else {
        dirfd as u16
    }
}

/// One `_IO_CONNECT` request.
pub struct Request<'a> {
    pub subtype: u16,
    pub file_type: u32,
    /// QNX `O_*` flags; the permission bits are derived from these.
    pub qnx_oflag: u32,
    pub mode: mode_t,
    pub eflag: u8,
    /// Directory descriptor for `*at()` resolution.
    pub dirfd: c_int,
    /// Payload appended after the path, used by `rename`, `link` and
    /// `symlink` to carry their second path.
    pub extra: Option<&'a [u8]>,
    pub extra_type: u8,
    /// Whether the caller wants to keep the resulting connection.
    pub keep_connection: bool,
}

impl Request<'_> {
    /// A request that opens a path.
    pub fn open(oflag: c_int, mode: mode_t, dirfd: c_int) -> Self {
        let mut qnx_oflag = oflag::to_qnx(oflag);
        if dirfd != fcntl::AT_FDCWD {
            qnx_oflag |= o::DIRFD;
        }
        Self {
            subtype: connect_subtype::OPEN,
            file_type: ftype::ANY,
            qnx_oflag,
            mode,
            eflag: 0,
            dirfd,
            extra: None,
            extra_type: connect_extra::NONE,
            keep_connection: true,
        }
    }

    /// A request that performs a one-shot namespace operation.
    pub fn action(subtype: u16, dirfd: c_int) -> Self {
        let mut qnx_oflag = o::LARGEFILE;
        if dirfd != fcntl::AT_FDCWD {
            qnx_oflag |= o::DIRFD;
        }
        Self {
            subtype,
            file_type: ftype::ANY,
            qnx_oflag,
            mode: 0,
            eflag: 0,
            dirfd,
            extra: None,
            extra_type: connect_extra::NONE,
            keep_connection: false,
        }
    }
}

/// What a successful [`resolve`] produced.
pub enum Resolved {
    /// The connection to the accepting manager, still attached.
    Connection(c_int),
    /// A one-shot operation completed; the connection has been detached.
    Done,
}

/// Sends an `_io_connect` header, its path and any extra payload as one
/// gather, so nothing has to be copied into a staging buffer.
fn send_connect(
    coid: c_int,
    header: &IoConnect,
    path: &[u8],
    extra: Option<&[u8]>,
    reply: *mut u8,
    reply_len: usize,
) -> Result<isize> {
    let iov = [
        IoVec::new(ptr::from_ref(header).cast::<u8>(), size_of::<IoConnect>()),
        IoVec::new(path.as_ptr(), path.len()),
        match extra {
            Some(extra) => IoVec::new(extra.as_ptr(), extra.len()),
            None => IoVec::empty(),
        },
    ];
    let parts = if extra.is_some() { 3 } else { 2 };
    let mut riov = IoVec::new(reply, reply_len);
    unsafe { msg_sendv(coid, iov.as_ptr(), parts, ptr::from_mut(&mut riov), 1) }
}

/// True if a connect reply status is a continuation rather than a completion.
///
/// The status is a 32-bit `int`. A link reply is `_IO_CONNECT_RET_FLAG |
/// _IO_CONNECT_RET_LINK` (`0x80010000`), which is negative as an `i32`. On
/// AArch64 the kernel often returns that value zero-extended in `x0`, so it
/// is *positive* as an `isize` and must be narrowed before the sign test.
fn is_continuation(status: isize) -> bool {
    let status = status as i32;
    status < 0 && (status as u32) & connect_ret::FLAG != 0
}

/// Runs the `_IO_CONNECT` protocol for `path`.
pub fn resolve(path: CStr, request: &Request<'_>) -> Result<Resolved> {
    let mut current = [0u8; PATH_MAX];
    let bytes = path.to_bytes_with_nul();
    if bytes.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    if bytes.len() <= 1 {
        return Err(Errno(ENOENT));
    }
    current[..bytes.len()].copy_from_slice(bytes);
    let mut current_len = bytes.len();

    let base_header = IoConnect {
        type_: msg::CONNECT,
        subtype: request.subtype,
        file_type: request.file_type,
        reply_max: size_of::<LinkReply>() as u16,
        entry_max: ENTRY_MAX as u16,
        key: 0,
        handle: SYSMGR_HANDLE,
        ioflag: oflag::to_ioflag(request.qnx_oflag),
        mode: oflag::mode_to_qnx(request.mode),
        sflag: sh::DENYNO,
        access: oflag::ACCESS_ALL,
        dirfd: qnx_dirfd(request.dirfd),
        path_len: 0,
        eflag: request.eflag,
        extra_type: request.extra_type,
        extra_len: request.extra.map_or(0, |e| e.len() as u16),
    };

    for _ in 0..SYMLOOP_MAX {
        let mut reply = LinkReply::zeroed();
        let mut header = base_header;
        header.path_len = current_len as u16;

        // Ask procnto which managers claim this path.
        let status = send_connect(
            PATHMGR_COID,
            &header,
            &current[..current_len],
            request.extra,
            ptr::from_mut(&mut reply).cast::<u8>(),
            size_of::<LinkReply>(),
        )?;

        if !is_continuation(status) {
            // procnto served the request itself, which happens for paths it
            // owns outright such as entries under /proc.
            return Ok(Resolved::Done);
        }
        if (status as u32) & connect_ret::LINK == 0 {
            return Err(Errno(ENOSYS));
        }

        let packed_path = reply.resolved_path();
        let nentries = reply.nentries();

        if nentries == 0 {
            // A symbolic link: restart resolution with the returned path.
            if packed_path.is_empty() {
                return Err(Errno(ELOOP));
            }
            current[..packed_path.len()].copy_from_slice(packed_path);
            current_len = packed_path.len();
            continue;
        }

        // The path table follows the entries, so procnto's rewritten path is
        // what the managers should be asked about.
        let resolved: &[u8] = if packed_path.is_empty() {
            &current[..current_len]
        } else {
            packed_path
        };

        let mut last_error = Errno(ENOENT);
        let mut follow_link = false;

        for entry in reply.candidates() {
            // The mountpoint consumed a prefix; the manager wants the rest.
            let prefix = usize::from(entry.prefix_len).min(resolved.len());
            let suffix = &resolved[prefix..];

            let attach_flags = if request.qnx_oflag & o::CLOEXEC != 0 {
                cof::CLOEXEC
            } else {
                0
            };
            let coid = match connect_attach(entry.nd, entry.pid, entry.chid, 0, attach_flags) {
                Ok(coid) => coid,
                Err(err) => {
                    last_error = err;
                    continue;
                }
            };

            let mut header = base_header;
            header.handle = entry.handle;
            header.key = entry.key;
            header.file_type = entry.file_type;
            header.path_len = suffix.len() as u16;
            // Only the path manager expands the candidate table.
            header.entry_max = 0;

            let mut server_reply = LinkReply::zeroed();
            let status = send_connect(
                coid,
                &header,
                suffix,
                request.extra,
                ptr::from_mut(&mut server_reply).cast::<u8>(),
                size_of::<LinkReply>(),
            );

            match status {
                Ok(status) if is_continuation(status) => {
                    let _ = connect_detach(coid);
                    if (status as u32) & connect_ret::LINK != 0 {
                        let packed_path = server_reply.resolved_path();
                        if packed_path.is_empty() {
                            last_error = Errno(ELOOP);
                            continue;
                        }
                        current[..packed_path.len()].copy_from_slice(packed_path);
                        current_len = packed_path.len();
                        follow_link = true;
                        break;
                    }
                    last_error = Errno(ENOSYS);
                }
                Ok(_) => {
                    return if request.keep_connection {
                        Ok(Resolved::Connection(coid))
                    } else {
                        connect_detach(coid)?;
                        Ok(Resolved::Done)
                    };
                }
                Err(err) => {
                    let _ = connect_detach(coid);
                    last_error = err;
                }
            }
        }

        if !follow_link {
            return Err(last_error);
        }
    }

    Err(Errno(ELOOP))
}

/// `openat()`.
pub fn openat(dirfd: c_int, path: CStr, oflag: c_int, mode: mode_t) -> Result<c_int> {
    match resolve(path, &Request::open(oflag, mode, dirfd))? {
        Resolved::Connection(fd) => Ok(fd),
        // A manager that completed an open without giving back a connection
        // has not produced a descriptor, so there is nothing to return.
        Resolved::Done => Err(Errno(ENOENT)),
    }
}

/// `faccessat()`, which QNX expresses as an open with `O_PERMCHECK`: the
/// manager performs the permission check and the descriptor is closed again.
pub fn faccessat(dirfd: c_int, path: CStr, amode: c_int) -> Result<()> {
    let mut request = Request::open(fcntl::O_RDONLY, 0, dirfd);
    request.qnx_oflag |= o::PERMCHECK;
    // The access mode being tested rides in the permission bits.
    let mut access = 0;
    if amode & crate::header::unistd::R_OK != 0 {
        access |= o::RDONLY;
    }
    if amode & crate::header::unistd::W_OK != 0 {
        access |= o::WRONLY;
    }
    if amode & crate::header::unistd::X_OK != 0 {
        access |= o::EXEC;
    }
    request.qnx_oflag = (request.qnx_oflag & !o::ACCMODE) | access;
    request.keep_connection = false;
    resolve(path, &request).map(|_| ())
}

/// `unlinkat()` without `AT_REMOVEDIR`.
pub fn unlinkat(dirfd: c_int, path: CStr) -> Result<()> {
    resolve(path, &Request::action(connect_subtype::UNLINK, dirfd)).map(|_| ())
}

/// `rmdir()`.
pub fn rmdir(dirfd: c_int, path: CStr) -> Result<()> {
    let mut request = Request::action(connect_subtype::UNLINK, dirfd);
    request.eflag = eflag::DIR;
    resolve(path, &request).map(|_| ())
}

/// `mkdirat()`, which QNX spells as `_IO_CONNECT_MKNOD` of a directory.
pub fn mkdirat(dirfd: c_int, path: CStr, mode: mode_t) -> Result<()> {
    let mut request = Request::action(connect_subtype::MKNOD, dirfd);
    request.mode = mode | crate::header::sys_stat::S_IFDIR;
    request.eflag = eflag::DIR;
    resolve(path, &request).map(|_| ())
}

/// `mknodat()`.
pub fn mknodat(dirfd: c_int, path: CStr, mode: mode_t) -> Result<()> {
    let mut request = Request::action(connect_subtype::MKNOD, dirfd);
    request.mode = mode;
    resolve(path, &request).map(|_| ())
}

/// `mkfifoat()`.
pub fn mkfifoat(dirfd: c_int, path: CStr, mode: mode_t) -> Result<()> {
    mknodat(dirfd, path, mode | crate::header::sys_stat::S_IFIFO)
}

/// `symlinkat()`, where the link target travels as the extra payload.
pub fn symlinkat(target: CStr, dirfd: c_int, path: CStr) -> Result<()> {
    let target = target.to_bytes_with_nul();
    if target.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    let mut request = Request::action(connect_subtype::LINK, dirfd);
    request.extra_type = connect_extra::SYMLINK;
    request.extra = Some(target);
    request.mode = crate::header::sys_stat::S_IFLNK;
    resolve(path, &request).map(|_| ())
}

/// `linkat()`, where the existing path travels as the extra payload.
pub fn linkat(existing: CStr, dirfd: c_int, path: CStr) -> Result<()> {
    let existing = existing.to_bytes_with_nul();
    if existing.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    let mut request = Request::action(connect_subtype::LINK, dirfd);
    // 1 on 7.1, 13 on 8.0 (1 became reserved).
    request.extra_type = super::version::CONNECT_EXTRA_LINK;
    request.extra = Some(existing);
    resolve(path, &request).map(|_| ())
}

/// `renameat()`, where the old path travels as the extra payload.
pub fn renameat(old: CStr, new_dirfd: c_int, new: CStr) -> Result<()> {
    let old = old.to_bytes_with_nul();
    if old.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    let mut request = Request::action(connect_subtype::RENAME, new_dirfd);
    request.extra_type = connect_extra::RENAME;
    request.extra = Some(old);
    resolve(new, &request).map(|_| ())
}

/// `readlinkat()`.
///
/// The manager replies with the link's contents rather than a connection, so
/// this drives the protocol directly instead of going through [`resolve`].
pub fn readlinkat(dirfd: c_int, path: CStr, out: &mut [u8]) -> Result<usize> {
    if out.is_empty() {
        return Err(Errno(EINVAL));
    }
    let bytes = path.to_bytes_with_nul();
    if bytes.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }

    let mut qnx_oflag = o::LARGEFILE | o::NOFOLLOW;
    if dirfd != fcntl::AT_FDCWD {
        qnx_oflag |= o::DIRFD;
    }
    let header = IoConnect {
        type_: msg::CONNECT,
        subtype: connect_subtype::READLINK,
        file_type: ftype::ANY,
        reply_max: size_of::<LinkReply>() as u16,
        entry_max: ENTRY_MAX as u16,
        key: 0,
        handle: SYSMGR_HANDLE,
        ioflag: oflag::to_ioflag(qnx_oflag),
        mode: 0,
        sflag: sh::DENYNO,
        access: oflag::ACCESS_ALL,
        dirfd: qnx_dirfd(dirfd),
        path_len: bytes.len() as u16,
        eflag: 0,
        extra_type: connect_extra::NONE,
        extra_len: 0,
    };

    let mut reply = LinkReply::zeroed();
    let status = send_connect(
        PATHMGR_COID,
        &header,
        bytes,
        None,
        ptr::from_mut(&mut reply).cast::<u8>(),
        size_of::<LinkReply>(),
    )?;

    if !is_continuation(status) || (status as u32) & connect_ret::LINK == 0 {
        return Err(Errno(ENOENT));
    }
    // A symbolic link resolves to zero entries, and the packed path holds the
    // link's contents.
    let packed = reply.resolved_path();
    if packed.is_empty() {
        return Err(Errno(ENOENT));
    }
    // The reply is NUL-terminated but readlink() must not copy the terminator.
    let len = packed.iter().position(|&byte| byte == 0).unwrap_or(packed.len());
    let copied = len.min(out.len());
    out[..copied].copy_from_slice(&packed[..copied]);
    Ok(copied)
}

/// Asks the path manager to change this process's working directory.
fn path_chdir(path: CStr) -> Result<()> {
    let bytes = path.to_bytes_with_nul();
    if bytes.len() > PATH_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    let header = IoConnect {
        type_: path::CHDIR,
        path_len: bytes.len() as u16,
        ..IoConnect::default()
    };
    send_connect(
        PATHMGR_COID,
        &header,
        bytes,
        None,
        ptr::null_mut(),
        0,
    )
    .map(|_| ())
}

/// `chdir()`.
pub fn chdir(path: CStr) -> Result<()> {
    // Confirm the path names a directory before asking procnto to switch.
    let fd = openat(
        fcntl::AT_FDCWD,
        path,
        fcntl::O_RDONLY | fcntl::O_DIRECTORY,
        0,
    )?;
    let _ = super::io::close(fd);
    path_chdir(path)
}
