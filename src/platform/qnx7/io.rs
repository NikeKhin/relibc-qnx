//! File descriptor operations.
//!
//! On Neutrino a file descriptor *is* a connection id: `open()` returns the
//! coid produced by `ConnectAttach`, and every operation on it is a message to
//! the resource manager on the other end. procnto owns the descriptor table, so
//! relibc keeps no fd bookkeeping of its own.
//!
//! The message layouts below were checked instruction by instruction against
//! the vendor libc. For example `read()` there builds a 16-byte `_io_read` and
//! calls `MsgSend(fd, &msg, 16, buf, nbytes)`, switching the message type from
//! `_IO_READ` to `_IO_READ64` and stashing the length's high half in the `zero`
//! field whenever `nbytes >= 2^32`.

use core::{mem::MaybeUninit, ptr};

use super::{
    abi::{
        devctl as dc,
        iomsg::{
            IoChmod, IoChown, IoClose, IoDevctl, IoFdinfo, IoFdinfoReply, IoLock, IoLseek,
            IoReadWrite, IoSpace, IoStat, IoSync, IoUtime, IoUtime64, MsgStatvfs, QnxFlock,
            Stat as QnxStat, flock_type, lock_cmd, lock_type, msg, space,
        },
        neutrino::{IoVec, IoVecPair},
        oflag::{self, o},
    },
    kernel::{connect_detach, msg_send, msg_send_struct, msg_sendv},
};

pub use super::dupfd::{dup_to, openfd};
use crate::{
    error::{Errno, Result},
    header::{
        errno::{EBADE, EINTR, EINVAL, ESPIPE},
        fcntl::{F_GETLK, F_RDLCK, F_SETLK, F_SETLKW, F_UNLCK, F_WRLCK, flock as Flock},
        sys_file::{LOCK_EX, LOCK_NB, LOCK_SH, LOCK_UN},
        sys_stat::stat,
        sys_statvfs::statvfs,
        time::timespec,
    },
    out::Out,
    platform::types::*,
};

/// Builds the `_io_read`/`_io_write` header for a transfer of `len` bytes,
/// picking the 64-bit message type when the length does not fit in 32 bits.
fn transfer_header(base: u16, base64: u16, len: usize) -> IoReadWrite {
    let high = (len >> 32) as u32;
    IoReadWrite {
        type_: if high == 0 { base } else { base64 },
        combine_len: 0,
        nbytes: len as u32,
        xtype: 0,
        zero: high,
    }
}

/// Validates a transfer count reported by a resource manager.
///
/// A server that claims to have moved more bytes than were requested is
/// misbehaving; the vendor libc reports `EBADE` for exactly this case rather
/// than trusting the count.
fn checked_transfer(ret: isize, requested: usize) -> Result<usize> {
    if ret < 0 || ret as usize > requested {
        Err(Errno(EBADE))
    } else {
        Ok(ret as usize)
    }
}

/// `read()`.
pub fn read(fd: c_int, buf: &mut [u8]) -> Result<usize> {
    // SAFETY: a live slice is valid for writing its whole length.
    unsafe { read_raw(fd, buf.as_mut_ptr(), buf.len()) }
}

/// `read()` into memory that need not be initialised.
///
/// # Safety
///
/// `buf` must be valid for writing `len` bytes.
pub unsafe fn read_raw(fd: c_int, buf: *mut u8, len: usize) -> Result<usize> {
    let msg = transfer_header(msg::READ, msg::READ64, len);
    let ret = unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoReadWrite>(),
            buf,
            len,
        )
    }?;
    checked_transfer(ret, len)
}

/// `write()`.
///
/// The payload is sent as a second iovec part rather than being copied into a
/// bounce buffer, matching the vendor implementation.
pub fn write(fd: c_int, buf: &[u8]) -> Result<usize> {
    let msg = transfer_header(msg::WRITE, msg::WRITE64, buf.len());
    let iov = IoVecPair {
        header: IoVec::new(
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoReadWrite>(),
        ),
        payload: IoVec::new(buf.as_ptr(), buf.len()),
    };
    let ret = unsafe {
        msg_sendv(
            fd,
            ptr::from_ref(&iov).cast::<IoVec>(),
            2,
            ptr::null_mut(),
            0,
        )
    }?;
    checked_transfer(ret, buf.len())
}

/// `pread()`, expressed as an `_IO_READ` carrying an explicit offset.
///
/// Neutrino has no positional read message; the vendor libc combines an
/// `_IO_LSEEK` and an `_IO_READ` into one send so that the pair is atomic with
/// respect to other users of the descriptor.
pub fn pread(fd: c_int, buf: &mut [u8], offset: off_t) -> Result<usize> {
    #[repr(C)]
    struct Combine {
        lseek: IoLseek,
        read: IoReadWrite,
    }

    let combine = Combine {
        lseek: IoLseek {
            type_: msg::LSEEK,
            combine_len: size_of::<IoLseek>() as u16,
            whence: crate::header::unistd::SEEK_SET as i16,
            flags: 0,
            offset: offset as u64,
        },
        read: transfer_header(msg::READ, msg::READ64, buf.len()),
    };
    let ret = unsafe {
        msg_send(
            fd,
            ptr::from_ref(&combine).cast::<u8>(),
            size_of::<Combine>(),
            buf.as_mut_ptr(),
            buf.len(),
        )
    }?;
    checked_transfer(ret, buf.len())
}

/// `pwrite()`; see [`pread`] for why this is a combine message.
pub fn pwrite(fd: c_int, buf: &[u8], offset: off_t) -> Result<usize> {
    #[repr(C)]
    struct Combine {
        lseek: IoLseek,
        write: IoReadWrite,
    }

    let combine = Combine {
        lseek: IoLseek {
            type_: msg::LSEEK,
            combine_len: size_of::<IoLseek>() as u16,
            whence: crate::header::unistd::SEEK_SET as i16,
            flags: 0,
            offset: offset as u64,
        },
        write: transfer_header(msg::WRITE, msg::WRITE64, buf.len()),
    };
    let iov = IoVecPair {
        header: IoVec::new(
            ptr::from_ref(&combine).cast::<u8>(),
            size_of::<Combine>(),
        ),
        payload: IoVec::new(buf.as_ptr(), buf.len()),
    };
    let ret = unsafe {
        msg_sendv(
            fd,
            ptr::from_ref(&iov).cast::<IoVec>(),
            2,
            ptr::null_mut(),
            0,
        )
    }?;
    checked_transfer(ret, buf.len())
}

/// `close()`.
///
/// The connection is detached even when the server reports an error, because
/// the descriptor is gone either way. `EINTR` is the one exception: the vendor
/// libc leaves the connection in place so the caller can retry.
pub fn close(fd: c_int) -> Result<()> {
    let msg = IoClose {
        type_: msg::CLOSE,
        combine_len: size_of::<IoClose>() as u16,
    };
    let result = unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoClose>(),
            ptr::null_mut(),
            0,
        )
    };
    match result {
        Err(Errno(EINTR)) => Err(Errno(EINTR)),
        other => {
            let detach = connect_detach(fd);
            other.map(|_| ()).and(detach)
        }
    }
}

/// `lseek()`.
pub fn lseek(fd: c_int, offset: off_t, whence: c_int) -> Result<off_t> {
    let msg = IoLseek {
        type_: msg::LSEEK,
        combine_len: size_of::<IoLseek>() as u16,
        whence: whence as i16,
        flags: 0,
        offset: offset as u64,
    };
    let mut reply: u64 = 0;
    unsafe { msg_send_struct(fd, &msg, &mut reply) }?;
    Ok(reply as off_t)
}

/// `fstat()`, converting QNX's `struct stat` into relibc's.
pub fn fstat(fd: c_int, mut buf: Out<stat>) -> Result<()> {
    let msg = IoStat {
        type_: msg::STAT,
        combine_len: size_of::<IoStat>() as u16,
        zero: 0,
    };
    let mut qnx = MaybeUninit::<QnxStat>::zeroed();
    unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoStat>(),
            qnx.as_mut_ptr().cast::<u8>(),
            size_of::<QnxStat>(),
        )
    }?;
    buf.write(convert_stat(&unsafe { qnx.assume_init() }));
    Ok(())
}

/// Translates a QNX `struct stat` into relibc's.
///
/// The two have nothing in common beyond the field names: QNX leads with
/// `st_ino`/`st_size` and keeps both 32-bit legacy and 64-bit timestamps, while
/// relibc follows the Linux ordering.
pub fn convert_stat(qnx: &QnxStat) -> stat {
    stat {
        st_dev: dev_t::from(qnx.st_dev),
        st_ino: qnx.st_ino,
        st_nlink: nlink_t::from(qnx.st_nlink),
        st_mode: qnx.st_mode,
        st_uid: qnx.st_uid as uid_t,
        st_gid: qnx.st_gid as gid_t,
        st_rdev: dev_t::from(qnx.st_rdev),
        st_size: qnx.st_size,
        st_blksize: qnx.st_blksize as blksize_t,
        st_blocks: qnx.st_blocks,
        st_atim: timespec {
            tv_sec: qnx.st_atim.tv_sec,
            tv_nsec: qnx.st_atim.tv_nsec,
        },
        st_mtim: timespec {
            tv_sec: qnx.st_mtim.tv_sec,
            tv_nsec: qnx.st_mtim.tv_nsec,
        },
        st_ctim: timespec {
            tv_sec: qnx.st_ctim.tv_sec,
            tv_nsec: qnx.st_ctim.tv_nsec,
        },
        _pad: [0; 24],
    }
}

/// `fchmod()`.
pub fn fchmod(fd: c_int, mode: mode_t) -> Result<()> {
    let msg = IoChmod {
        type_: msg::CHMOD,
        combine_len: size_of::<IoChmod>() as u16,
        mode,
    };
    unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoChmod>(),
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// `fchown()`.
pub fn fchown(fd: c_int, uid: uid_t, gid: gid_t) -> Result<()> {
    let msg = IoChown {
        type_: msg::CHOWN,
        combine_len: size_of::<IoChown>() as u16,
        gid: gid as i32,
        uid: uid as i32,
    };
    unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoChown>(),
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// `fsync()` and `fdatasync()`.
pub fn fsync(fd: c_int, flag: u32) -> Result<()> {
    let msg = IoSync {
        type_: msg::SYNC,
        combine_len: size_of::<IoSync>() as u16,
        flag,
    };
    unsafe {
        msg_send(
            fd,
            ptr::from_ref(&msg).cast::<u8>(),
            size_of::<IoSync>(),
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// `ftruncate()` and `posix_fallocate()`, both of which are `_IO_SPACE`.
pub fn space(fd: c_int, subtype: u16, start: u64, len: u64) -> Result<u64> {
    if len > i64::MAX as u64 {
        return Err(Errno(EINVAL));
    }
    let msg = IoSpace {
        type_: msg::SPACE,
        combine_len: size_of::<IoSpace>() as u16,
        subtype,
        whence: crate::header::unistd::SEEK_SET as i16,
        start,
        len,
    };
    let mut reply: u64 = 0;
    unsafe { msg_send_struct(fd, &msg, &mut reply) }?;
    Ok(reply)
}

/// `ftruncate()`, the vendor's way: free the space past `length`, whose reply
/// is the size that leaves, and if that is still short of `length`, grow the
/// file by the difference — allocating it instead where the manager cannot
/// grow. A manager that cannot do either at all reports `ESPIPE`, which is
/// `EINVAL` here.
pub fn ftruncate(fd: c_int, length: off_t) -> Result<()> {
    let length = u64::try_from(length).map_err(|_| Errno(EINVAL))?;
    let espipe = |err: Errno| if err.0 == ESPIPE { Errno(EINVAL) } else { err };
    let size = space(fd, space::FREE, length, 0).map_err(espipe)?;
    if size >= length {
        return Ok(());
    }
    match space(fd, space::GROW, size, length - size) {
        Err(Errno(EINVAL)) => space(fd, space::ALLOC, size, length - size)
            .map(|_| ())
            .map_err(espipe),
        other => other.map(|_| ()).map_err(espipe),
    }
}

/// `devctl()`, QNX's equivalent of `ioctl()`.
///
/// The payload follows the message header in both directions; which of them
/// actually carries data is encoded in the command word, but sending and
/// receiving the buffer unconditionally is what the vendor libc does and keeps
/// the caller from having to care.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes in whichever direction `cmd` implies.
pub unsafe fn devctl(
    fd: c_int,
    cmd: u32,
    buf: *mut u8,
    len: usize,
    info: Option<&mut u32>,
) -> Result<()> {
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Reply {
        zero: [u32; 2],
        ret_val: i32,
        zero2: i32,
    }

    let header = IoDevctl {
        type_: msg::DEVCTL,
        combine_len: size_of::<IoDevctl>() as u16,
        dcmd: cmd as i32,
        nbytes: len as u32,
        zero: 0,
    };
    let send = IoVecPair {
        header: IoVec::new(
            ptr::from_ref(&header).cast::<u8>(),
            size_of::<IoDevctl>(),
        ),
        payload: IoVec::new(buf, len),
    };
    let mut reply = Reply::default();
    let mut receive = IoVecPair {
        header: IoVec::new(
            ptr::from_mut(&mut reply).cast::<u8>(),
            size_of::<Reply>(),
        ),
        payload: IoVec::new(buf, len),
    };

    unsafe {
        msg_sendv(
            fd,
            ptr::from_ref(&send).cast::<IoVec>(),
            2,
            ptr::from_mut(&mut receive).cast::<IoVec>(),
            2,
        )
    }?;

    if let Some(info) = info {
        *info = reply.ret_val as u32;
    }
    Ok(())
}

/// `iofdinfo`: returns the path associated with `fd`.
pub fn fpath(fd: c_int, out: &mut [u8]) -> Result<usize> {
    let header = IoFdinfo {
        type_: msg::FDINFO,
        combine_len: size_of::<IoFdinfo>() as u16,
        flags: 0,
        path_len: out.len() as u32,
        reserved: 0,
    };
    let mut reply = IoFdinfoReply::default();
    out.fill(0);
    let send = IoVec::new(ptr::from_ref(&header).cast::<u8>(), size_of::<IoFdinfo>());
    let mut recv = [
        IoVec::new(
            ptr::from_mut(&mut reply).cast::<u8>(),
            size_of::<IoFdinfoReply>(),
        ),
        IoVec::new(out.as_mut_ptr(), out.len()),
    ];
    unsafe {
        msg_sendv(
            fd,
            ptr::from_ref(&send).cast::<IoVec>(),
            1,
            recv.as_mut_ptr(),
            if out.is_empty() { 1 } else { 2 },
        )
    }?;
    let _ = reply;
    let n = out.iter().position(|&b| b == 0).unwrap_or(out.len());
    Ok(n)
}

/// `F_GETFL`.
pub fn getfl(fd: c_int) -> Result<c_int> {
    let mut flags: u32 = 0;
    unsafe {
        devctl(
            fd,
            dc::all::GETFLAGS,
            ptr::from_mut(&mut flags).cast::<u8>(),
            size_of::<u32>(),
            None,
        )
    }?;
    Ok(oflag::from_qnx(flags))
}

/// `F_SETFL`.
pub fn setfl(fd: c_int, oflag: c_int) -> Result<()> {
    let mut current: u32 = 0;
    unsafe {
        devctl(
            fd,
            dc::all::GETFLAGS,
            ptr::from_mut(&mut current).cast::<u8>(),
            size_of::<u32>(),
            None,
        )
    }?;
    let acc = current & o::ACCMODE;
    let mut qnx = oflag::to_qnx(oflag);
    qnx = (qnx & !o::ACCMODE) | acc;
    unsafe {
        devctl(
            fd,
            dc::all::SETFLAGS,
            ptr::from_mut(&mut qnx).cast::<u8>(),
            size_of::<u32>(),
            None,
        )
    }
}

/// `fstatvfs()`.
pub fn fstatvfs(fd: c_int, mut buf: Out<statvfs>) -> Result<()> {
    let mut msg = MsgStatvfs::default();
    unsafe {
        devctl(
            fd,
            dc::fsys::STATVFS,
            ptr::from_mut(&mut msg).cast::<u8>(),
            size_of::<MsgStatvfs>(),
            None,
        )
    }?;
    buf.write(statvfs {
        f_bsize: msg.f_bsize as _,
        f_frsize: msg.f_frsize as _,
        f_blocks: msg.f_blocks,
        f_bfree: msg.f_bfree,
        f_bavail: msg.f_bavail,
        f_files: msg.f_files,
        f_ffree: msg.f_ffree,
        f_favail: msg.f_favail,
        f_fsid: msg.f_fsid as _,
        f_flag: msg.f_flag as _,
        f_namemax: msg.f_namemax as _,
    });
    Ok(())
}

fn to_qnx_flock(src: &Flock) -> QnxFlock {
    QnxFlock {
        l_type: match i32::from(src.l_type) {
            x if x == F_RDLCK => lock_type::RDLCK,
            x if x == F_WRLCK => lock_type::WRLCK,
            x if x == F_UNLCK => lock_type::UNLCK,
            other => other as i16,
        },
        l_whence: src.l_whence,
        pad: 0,
        l_start: src.l_start as i64,
        l_len: src.l_len as i64,
        l_pid: src.l_pid,
        l_sysid: 0,
    }
}

fn from_qnx_flock(src: &QnxFlock) -> Flock {
    Flock {
        l_type: match src.l_type {
            lock_type::RDLCK => F_RDLCK as c_short,
            lock_type::WRLCK => F_WRLCK as c_short,
            lock_type::UNLCK => F_UNLCK as c_short,
            other => other,
        },
        l_whence: src.l_whence,
        l_start: src.l_start,
        l_len: src.l_len,
        l_pid: src.l_pid,
    }
}

/// `fcntl` `F_GETLK` / `F_SETLK` / `F_SETLKW`.
pub fn fcntl_lock(fd: c_int, cmd: c_int, arg: c_ulonglong) -> Result<c_int> {
    let ptr = arg as *mut Flock;
    if ptr.is_null() {
        return Err(Errno(EINVAL));
    }
    // SAFETY: POSIX `fcntl` passes a `struct flock *` in the third argument.
    let src = unsafe { &*ptr };
    let mut qnx = to_qnx_flock(src);
    let subtype = match cmd {
        F_GETLK => lock_cmd::GETLK,
        F_SETLK => lock_cmd::SETLK,
        F_SETLKW => lock_cmd::SETLKW,
        _ => return Err(Errno(EINVAL)),
    };
    send_lock(fd, subtype, &mut qnx, cmd == F_GETLK)?;
    if cmd == F_GETLK {
        unsafe { ptr.write(from_qnx_flock(&qnx)) };
    }
    Ok(0)
}

/// Sends `_IO_LOCK` with `lock` as its payload, reading the lock back into it
/// when `reply` is set (`F_GETLK`).
fn send_lock(fd: c_int, subtype: u32, lock: &mut QnxFlock, reply: bool) -> Result<()> {
    let header = IoLock {
        type_: msg::LOCK,
        combine_len: size_of::<IoLock>() as u16,
        subtype,
        nbytes: size_of::<QnxFlock>() as u32,
    };
    let lock_ptr = ptr::from_mut(lock).cast::<u8>();
    let send = [
        IoVec::new(ptr::from_ref(&header).cast::<u8>(), size_of::<IoLock>()),
        IoVec::new(lock_ptr, size_of::<QnxFlock>()),
    ];
    let mut recv_pad = [0u32; 3];
    let mut recv = [
        IoVec::new(
            ptr::from_mut(&mut recv_pad).cast::<u8>(),
            size_of::<[u32; 3]>(),
        ),
        IoVec::new(lock_ptr, size_of::<QnxFlock>()),
    ];
    unsafe {
        msg_sendv(
            fd,
            send.as_ptr(),
            2,
            recv.as_mut_ptr(),
            if reply { 2 } else { 0 },
        )
    }
    .map(|_| ())
}

/// `flock()`: a whole-file BSD lock, which QNX encodes in `l_type` with its
/// own values (see `flock_type`) rather than as a record lock.
pub fn flock(fd: c_int, operation: c_int) -> Result<()> {
    let l_type = if operation & LOCK_UN != 0 {
        flock_type::UN
    } else if operation & LOCK_EX != 0 {
        flock_type::EX
    } else if operation & LOCK_SH != 0 {
        flock_type::SH
    } else {
        return Err(Errno(EINVAL));
    };
    let subtype = if operation & LOCK_NB != 0 {
        lock_cmd::SETLK
    } else {
        lock_cmd::SETLKW
    };
    let mut lock = QnxFlock {
        l_type,
        l_whence: 0,
        pad: 0,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
        l_sysid: 0,
    };
    send_lock(fd, subtype, &mut lock, false)
}

/// `_IO_UTIME` / `_IO_UTIME64` on an open descriptor.
pub fn futimens(fd: c_int, times: *const timespec) -> Result<()> {
    if times.is_null() {
        let header = IoUtime {
            type_: msg::UTIME,
            combine_len: size_of::<IoUtime>() as u16,
            cur_flag: 1,
        };
        return unsafe {
            msg_send(
                fd,
                ptr::from_ref(&header).cast::<u8>(),
                size_of::<IoUtime>(),
                ptr::null_mut(),
                0,
            )
        }
        .map(|_| ());
    }
    // SAFETY: POSIX `futimens` passes an array of two `timespec`s.
    let times = unsafe { core::slice::from_raw_parts(times, 2) };
    let now = crate::header::sys_stat::UTIME_NOW;
    let omit = crate::header::sys_stat::UTIME_OMIT;

    let resolve = |ts: timespec, current: timespec| -> Result<timespec> {
        if ts.tv_nsec == now {
            let mut now_ts = timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            super::time::clock_gettime(
                crate::header::time::CLOCK_REALTIME,
                Out::from_mut(&mut now_ts),
            )?;
            Ok(now_ts)
        } else if ts.tv_nsec == omit {
            Ok(current)
        } else {
            Ok(ts)
        }
    };

    if times[0].tv_nsec == now && times[1].tv_nsec == now {
        return futimens(fd, ptr::null());
    }
    if times[0].tv_nsec == omit && times[1].tv_nsec == omit {
        return Ok(());
    }

    let mut st = stat::default();
    if times[0].tv_nsec == omit || times[1].tv_nsec == omit {
        fstat(fd, Out::from_mut(&mut st))?;
    }
    let at = resolve(times[0].clone(), st.st_atim.clone())?;
    let mt = resolve(times[1].clone(), st.st_mtim.clone())?;
    let header = IoUtime64 {
        type_: msg::UTIME64,
        combine_len: size_of::<IoUtime64>() as u16,
        zero: 0,
        atime_s: at.tv_sec as i64,
        atime_ns: at.tv_nsec as i64,
        mtime_s: mt.tv_sec as i64,
        mtime_ns: mt.tv_nsec as i64,
    };
    unsafe {
        msg_send(
            fd,
            ptr::from_ref(&header).cast::<u8>(),
            size_of::<IoUtime64>(),
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}
