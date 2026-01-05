//! Sockets.
//!
//! On Neutrino the socket API is not a kernel service. The network stack
//! (io-pkt on 7.1, io-sock on 8.0) registers `/dev/socket/`, and the vendor
//! `libsocket.so` is a thin client over it: `socket()` opens a path there,
//! connection management is `devctl`s on the resulting descriptor, and data
//! and options travel as `_IO_WRITE`/`_IO_READ`/`_IO_MSG` messages with the
//! TCPIP xtypes. The vendor `bind`, for instance, is a single
//! `devctl(fd, 0x80100601, addr, addrlen, 5)`: direction `TO`, class 6,
//! subcode 1, and a size field of 16 whatever the address length.
//!
//! What io-sock changed is switched on in `version::sock`: how `socket()`
//! names the type and protocol, the width of the flags and address length in
//! the data messages, and the sockopt subtypes. 8.0's messages are read off
//! `libsocket.so.4`; 7.1's come from its `sys/sockmsg.h`.
//!
//! Everything crossing the boundary goes through `abi::socket`, because
//! relibc's socket ABI is Linux's. Two things are refused rather than
//! approximated:
//!
//! * ancillary data (`msg_control`): passing descriptors needs the vendor's
//!   rewrite of `SCM_RIGHTS` into connection identifiers;
//! * `AF_UNIX` addresses, whose filesystem node the vendor creates and
//!   resolves in the client before talking to the stack.

use core::{mem::size_of, ptr};

use super::{
    Sys,
    abi::{
        devctl as dc, errno,
        iomsg::{IoMsg, IoReadWrite, connect_extra, ftype, iomgr, msg, openfd, xtype},
        neutrino::IoVec,
        socket::{self as sk, OptValue, SOCKADDR_MAX},
    },
    connect, io,
    kernel::msg_sendv,
    version::sock,
};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        errno::{EAFNOSUPPORT, EFAULT, EINVAL, ENOENT, ENOPROTOOPT, EOPNOTSUPP},
        fcntl,
        sys_socket::{
            constants::{SOCK_CLOEXEC, SOCK_NONBLOCK},
            msghdr, socklen_t,
        },
        bits_timeval::timeval,
        sys_uio::iovec,
    },
    out::Out,
    platform::{PalSocket, types::c_int},
};

/// Socket `devctl` subcodes, read off the vendor `libsocket.so`.
mod subcode {
    pub const GETSOCKNAME: u32 = 0;
    pub const BIND: u32 = 1;
    pub const GETPEERNAME: u32 = 2;
    pub const CONNECT: u32 = 3;
    pub const LISTEN: u32 = 4;
    pub const SHUTDOWN: u32 = 5;
}

/// The size field of every address `devctl`, `sizeof(struct sockaddr)`; the
/// real length travels as the `devctl`'s byte count.
const ADDR_CMD_SIZE: usize = 16;

/// Appends `value` in decimal.
fn push_decimal(path: &mut [u8], len: &mut usize, value: c_int) {
    let mut digits = [0u8; 11];
    let mut n = 0;
    let mut v = value.unsigned_abs();
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 {
            break;
        }
    }
    if value < 0 {
        digits[n] = b'-';
        n += 1;
    }
    for &d in digits[..n].iter().rev() {
        path[*len] = d;
        *len += 1;
    }
}

/// Opens a connection to the stack for a new socket.
fn open_socket(domain: c_int, kind: c_int, protocol: c_int) -> Result<c_int> {
    let qnx_domain = sk::af_to_kernel(domain).ok_or(Errno(EAFNOSUPPORT))?;
    let cloexec = kind & SOCK_CLOEXEC != 0;
    let nonblock = kind & SOCK_NONBLOCK != 0;
    let kind = kind & !(SOCK_CLOEXEC | SOCK_NONBLOCK);

    // "/dev/socket/" plus up to three decimal ints, separators and a NUL.
    let mut path = [0u8; 64];
    let prefix = b"/dev/socket/";
    path[..prefix.len()].copy_from_slice(prefix);
    let mut len = prefix.len();
    push_decimal(&mut path, &mut len, qnx_domain);

    if sock::OPEN_EXTRA {
        // "/dev/socket/<domain>", with the type and protocol as payload and
        // SOCK_NONBLOCK left for the stack, as the vendor socket() does.
        let kind = if nonblock { kind | sock::SOCK_NONBLOCK } else { kind };
        let mut extra = [0u8; 8];
        extra[..4].copy_from_slice(&kind.to_ne_bytes());
        extra[4..].copy_from_slice(&protocol.to_ne_bytes());
        let oflag = if cloexec { fcntl::O_CLOEXEC } else { fcntl::O_RDONLY };
        let mut request = connect::Request::open(oflag, 0, fcntl::AT_FDCWD);
        request.file_type = ftype::SOCKET;
        request.extra_type = connect_extra::SOCKET;
        request.extra = Some(&extra);
        // SAFETY: `path` is zero-filled past `len`, so it is NUL-terminated.
        let path = unsafe { CStr::from_ptr(path.as_ptr().cast()) };
        match connect::resolve(path, &request)? {
            connect::Resolved::Connection(fd) => Ok(fd),
            connect::Resolved::Done => Err(Errno(ENOENT)),
        }
    } else {
        // "/dev/socket/<domain>.<type>.<protocol>".
        for value in [kind, protocol] {
            path[len] = b'.';
            len += 1;
            push_decimal(&mut path, &mut len, value);
        }
        let mut oflag = fcntl::O_RDWR;
        if cloexec {
            oflag |= fcntl::O_CLOEXEC;
        }
        if nonblock {
            oflag |= fcntl::O_NONBLOCK;
        }
        // SAFETY: as above.
        let path = unsafe { CStr::from_ptr(path.as_ptr().cast()) };
        connect::openat(fcntl::AT_FDCWD, path, oflag, 0)
    }
}

/// Issues a socket `devctl` carrying an address and returns the stack's
/// `dev_info`, which for the queries is the address length.
fn addr_devctl(socket: c_int, dir: u32, subcode: u32, addr: &mut [u8]) -> Result<usize> {
    let cmd = dc::cmd(dir, dc::class::IP, subcode, ADDR_CMD_SIZE);
    let mut info = 0u32;
    // SAFETY: the buffer is a live slice of the length passed.
    unsafe { io::devctl(socket, cmd, addr.as_mut_ptr(), addr.len(), Some(&mut info)) }?;
    Ok(info as usize)
}

/// Issues a socket `devctl` carrying a single integer.
fn int_devctl(socket: c_int, subcode: u32, value: c_int) -> Result<()> {
    let mut value = value;
    let cmd = dc::cmd(dc::TO, dc::class::IP, subcode, size_of::<c_int>());
    // SAFETY: `value` is a live c_int of exactly the declared size.
    unsafe {
        io::devctl(
            socket,
            cmd,
            ptr::from_mut(&mut value).cast::<u8>(),
            size_of::<c_int>(),
            None,
        )
    }
}

/// Translates an address for the stack, refusing the ones it cannot take.
fn kernel_addr(src: &[u8], dst: &mut [u8; SOCKADDR_MAX]) -> Result<usize> {
    let len = sk::sockaddr_to_kernel(src, dst)?;
    if c_int::from(dst[sk::SA_FAMILY]) == sk::af::UNIX {
        return Err(Errno(EOPNOTSUPP));
    }
    Ok(len)
}

/// Reads the local or peer address into `buf` and returns its length.
fn query_addr(socket: c_int, subcode: u32, buf: &mut [u8; SOCKADDR_MAX]) -> Result<usize> {
    let info = addr_devctl(socket, dc::FROM, subcode, &mut buf[..])?;
    Ok(if info == 0 {
        SOCKADDR_MAX
    } else {
        info.min(SOCKADDR_MAX)
    })
}

/// The data-message extension after `_io_write`/`_io_read`, 32 bits wide on
/// io-sock and 16 on io-pkt.
#[repr(C)]
struct WideXfer {
    io: IoReadWrite,
    flags: u32,
    addrlen: u32,
}

#[repr(C)]
struct NarrowXfer {
    io: IoReadWrite,
    flags: u16,
    addrlen: u16,
}

/// Builds the TCPIP-xtype header for a transfer of `nbytes`.
fn xfer_header(type_: u16, nbytes: usize, flags: c_int, addrlen: usize) -> Result<(WideXfer, NarrowXfer)> {
    let io = |combine_len: usize| IoReadWrite {
        type_,
        combine_len: combine_len as u16,
        nbytes: nbytes as u32,
        xtype: xtype::TCPIP,
        zero: 0,
    };
    let narrow_flags = u16::try_from(flags).map_err(|_| Errno(EOPNOTSUPP))?;
    Ok((
        WideXfer {
            io: io(size_of::<WideXfer>()),
            flags: flags as u32,
            addrlen: addrlen as u32,
        },
        NarrowXfer {
            io: io(size_of::<NarrowXfer>()),
            flags: narrow_flags,
            addrlen: addrlen as u16,
        },
    ))
}

/// `sendto` with kernel-ready flags and address.
fn send_data(socket: c_int, buf: &[u8], flags: c_int, addr: &[u8]) -> Result<usize> {
    let buf = &buf[..buf.len().min(u32::MAX as usize)];
    let (wide, narrow) = xfer_header(msg::WRITE, buf.len(), flags, addr.len())?;
    let header = if sock::WIDE_HEADERS {
        IoVec::new(ptr::from_ref(&wide).cast::<u8>(), size_of::<WideXfer>())
    } else {
        IoVec::new(ptr::from_ref(&narrow).cast::<u8>(), size_of::<NarrowXfer>())
    };
    let send = [
        header,
        IoVec::new(addr.as_ptr(), addr.len()),
        IoVec::new(buf.as_ptr(), buf.len()),
    ];
    // SAFETY: every part is a live buffer of the length it declares.
    let ret = unsafe { msg_sendv(socket, send.as_ptr(), 3, ptr::null_mut(), 0) }?;
    Ok(ret as usize)
}

/// `recvfrom` into `len` bytes at `data`, with kernel-ready flags; the
/// sender's address, if wanted, lands in `addr`. Returns the byte count and
/// the address length.
///
/// # Safety
///
/// `data` must be valid for writing `len` bytes.
unsafe fn recv_data(
    socket: c_int,
    data: *mut u8,
    len: usize,
    flags: c_int,
    addr: Option<&mut [u8; SOCKADDR_MAX]>,
) -> Result<(usize, usize)> {
    let len = len.min(u32::MAX as usize);
    let addr_cap = if addr.is_some() { SOCKADDR_MAX } else { 0 };
    let (wide, narrow) = xfer_header(msg::READ, len, flags, addr_cap)?;
    let (header, reply_width) = if sock::WIDE_HEADERS {
        (
            IoVec::new(ptr::from_ref(&wide).cast::<u8>(), size_of::<WideXfer>()),
            size_of::<u32>(),
        )
    } else {
        (
            IoVec::new(ptr::from_ref(&narrow).cast::<u8>(), size_of::<NarrowXfer>()),
            size_of::<u16>(),
        )
    };
    // The reply starts with the address length, in the header's width.
    let mut reply_len = [0u8; 4];
    let mut scratch = [0u8; SOCKADDR_MAX];
    let addr_ptr = addr.map_or(scratch.as_mut_ptr(), |a| a.as_mut_ptr());
    let mut recv = [
        IoVec::new(reply_len.as_mut_ptr(), reply_width),
        IoVec::new(addr_ptr, addr_cap),
        IoVec::new(data, len),
    ];
    // SAFETY: the reply parts are live buffers of the declared lengths; the
    // caller guarantees `data`.
    let n = unsafe { msg_sendv(socket, &header, 1, recv.as_mut_ptr(), 3) }?;
    let addr_len = if sock::WIDE_HEADERS {
        u32::from_ne_bytes(reply_len) as usize
    } else {
        usize::from(u16::from_ne_bytes([reply_len[0], reply_len[1]]))
    };
    Ok((n as usize, addr_len.min(addr_cap)))
}

fn timeval_to_kernel(value: &[u8]) -> Result<sk::Timeval> {
    if value.len() < size_of::<timeval>() {
        return Err(Errno(EINVAL));
    }
    // SAFETY: the slice holds at least one `timeval`, and every bit pattern
    // is a valid one.
    let tv = unsafe { value.as_ptr().cast::<timeval>().read_unaligned() };
    Ok(sk::Timeval {
        tv_sec: tv.tv_sec as i64,
        tv_usec: i32::try_from(tv.tv_usec).map_err(|_| Errno(EINVAL))?,
        pad: 0,
    })
}

fn timeval_from_kernel(raw: &sk::Timeval, dst: &mut [u8]) -> usize {
    let tv = timeval {
        tv_sec: raw.tv_sec as _,
        tv_usec: raw.tv_usec as _,
    };
    let n = size_of::<timeval>().min(dst.len());
    // SAFETY: copies at most one `timeval` into the caller's buffer.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(&tv).cast::<u8>(), dst.as_mut_ptr(), n) };
    n
}

/// The `_IO_MSG` header of a sockopt request.
#[repr(C)]
struct OptHeader {
    msg: IoMsg,
    level: u32,
    optname: u32,
    optlen: u32,
    zero: u32,
}

fn opt_header(set: bool, level: c_int, name: c_int, optlen: usize) -> OptHeader {
    let header_len = size_of::<OptHeader>();
    OptHeader {
        msg: IoMsg {
            type_: msg::MSG,
            // io-sock counts a set's value in `combine_len`; io-pkt did not.
            combine_len: if set && sock::WIDE_HEADERS {
                (header_len + optlen) as u16
            } else {
                header_len as u16
            },
            mgrid: iomgr::TCPIP,
            subtype: if set { sock::SETOPT } else { sock::GETOPT },
        },
        level: level as u32,
        optname: name as u32,
        optlen: optlen as u32,
        zero: 0,
    }
}

/// Reads an option in the stack's encoding into `value`; returns its length.
fn get_opt(socket: c_int, level: c_int, name: c_int, value: &mut [u8]) -> Result<usize> {
    let mut header = opt_header(false, level, name, value.len());
    let header_ptr = ptr::from_mut(&mut header).cast::<u8>();
    let value_len = value.len();
    let value_ptr = value.as_mut_ptr();
    let send = [
        IoVec::new(header_ptr, size_of::<OptHeader>()),
        IoVec::new(value_ptr, value_len),
    ];
    if sock::WIDE_HEADERS {
        // io-sock answers with the header, whose `optlen` is the result's
        // length, followed by the value.
        let mut recv = [
            IoVec::new(header_ptr, size_of::<OptHeader>()),
            IoVec::new(value_ptr, value_len),
        ];
        // SAFETY: both parts are live buffers of the declared lengths.
        unsafe { msg_sendv(socket, send.as_ptr(), 2, recv.as_mut_ptr(), 2) }?;
        Ok((header.optlen as usize).min(value_len))
    } else {
        // io-pkt answers with the value alone. The high bit of the name
        // marks options whose value is also an input.
        const GETSOCKOPT_EXTRA: u32 = 0x8000_0000;
        let extra = (name as u32) & GETSOCKOPT_EXTRA != 0;
        let mut recv = IoVec::new(value_ptr, value_len);
        // SAFETY: as above.
        let n = unsafe {
            msg_sendv(socket, send.as_ptr(), if extra { 2 } else { 1 }, &mut recv, 1)
        }?;
        Ok((n as usize).min(value_len))
    }
}

fn set_opt(socket: c_int, level: c_int, name: c_int, value: &[u8]) -> Result<()> {
    let header = opt_header(true, level, name, value.len());
    let send = [
        IoVec::new(ptr::from_ref(&header).cast::<u8>(), size_of::<OptHeader>()),
        IoVec::new(value.as_ptr(), value.len()),
    ];
    // SAFETY: both parts are live buffers of the declared lengths.
    unsafe {
        msg_sendv(
            socket,
            send.as_ptr(),
            if value.is_empty() { 1 } else { 2 },
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// The iovecs of a `msghdr`.
///
/// # Safety
///
/// `iov` must point to `len` valid iovecs that outlive `'a`, or be null.
unsafe fn msg_iovs<'a>(iov: *const iovec, len: usize) -> &'a [iovec] {
    if iov.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(iov, len) }
    }
}

impl PalSocket for Sys {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> Result<c_int> {
        open_socket(domain, kind, protocol)
    }

    fn bind(socket: c_int, address_raw: &[u8]) -> Result<()> {
        let mut addr = [0u8; SOCKADDR_MAX];
        let len = kernel_addr(address_raw, &mut addr)?;
        addr_devctl(socket, dc::TO, subcode::BIND, &mut addr[..len]).map(|_| ())
    }

    fn connect(socket: c_int, address_raw: &[u8]) -> Result<c_int> {
        let mut addr = [0u8; SOCKADDR_MAX];
        let len = kernel_addr(address_raw, &mut addr)?;
        addr_devctl(socket, dc::TO, subcode::CONNECT, &mut addr[..len]).map(|_| 0)
    }

    fn listen(socket: c_int, backlog: c_int) -> Result<()> {
        int_devctl(socket, subcode::LISTEN, backlog)
    }

    fn accept(socket: c_int, address_dst: Option<&mut [u8]>) -> Result<(c_int, socklen_t)> {
        let accepted = io::openfd(socket, fcntl::O_RDWR, openfd::ACCEPT)?;
        let Some(dst) = address_dst else {
            return Ok((accepted, 0));
        };
        let mut addr = [0u8; SOCKADDR_MAX];
        match query_addr(accepted, subcode::GETPEERNAME, &mut addr) {
            Ok(n) => Ok((
                accepted,
                sk::sockaddr_from_kernel(&addr[..n], dst) as socklen_t,
            )),
            Err(err) => {
                let _ = io::close(accepted);
                Err(err)
            }
        }
    }

    fn getsockname(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        let mut addr = [0u8; SOCKADDR_MAX];
        let n = query_addr(socket, subcode::GETSOCKNAME, &mut addr)?;
        Ok(sk::sockaddr_from_kernel(&addr[..n], address_dst) as socklen_t)
    }

    fn getpeername(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        let mut addr = [0u8; SOCKADDR_MAX];
        let n = query_addr(socket, subcode::GETPEERNAME, &mut addr)?;
        Ok(sk::sockaddr_from_kernel(&addr[..n], address_dst) as socklen_t)
    }

    fn shutdown(socket: c_int, how: c_int) -> Result<()> {
        // SHUT_RD/SHUT_WR/SHUT_RDWR are numbered alike.
        int_devctl(socket, subcode::SHUTDOWN, how)
    }

    fn sendto(socket: c_int, buf: &[u8], flags: c_int, dest_addr: Option<&[u8]>) -> Result<usize> {
        let flags = sk::msg_flags_to_kernel(flags)?;
        if flags == 0 && dest_addr.is_none() {
            return io::write(socket, buf);
        }
        let mut addr = [0u8; SOCKADDR_MAX];
        let addr_len = match dest_addr {
            Some(dest) => kernel_addr(dest, &mut addr)?,
            None => 0,
        };
        send_data(socket, buf, flags, &addr[..addr_len])
    }

    fn recvfrom(
        socket: c_int,
        mut buf: Out<[u8]>,
        flags: c_int,
        address_raw: Option<&mut [u8]>,
    ) -> Result<(usize, socklen_t)> {
        let flags = sk::msg_flags_to_kernel(flags)?;
        let data = buf.as_mut_ptr().cast::<u8>();
        let len = buf.len();
        let Some(dst) = address_raw else {
            // SAFETY: `Out` guarantees `len` writable bytes.
            let n = if flags == 0 {
                unsafe { io::read_raw(socket, data, len) }?
            } else {
                unsafe { recv_data(socket, data, len, flags, None) }?.0
            };
            return Ok((n, 0));
        };
        let mut addr = [0u8; SOCKADDR_MAX];
        // SAFETY: as above.
        let (n, addr_len) = unsafe { recv_data(socket, data, len, flags, Some(&mut addr)) }?;
        let full = sk::sockaddr_from_kernel(&addr[..addr_len], dst);
        Ok((n, full as socklen_t))
    }

    fn getsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &mut [u8],
    ) -> Result<socklen_t> {
        let (level, name, kind) =
            sk::sockopt_to_kernel(level, option_name).ok_or(Errno(ENOPROTOOPT))?;
        let n = match kind {
            OptValue::Plain => get_opt(socket, level, name, option_value)?,
            OptValue::Timeval => {
                let mut raw = sk::Timeval::default();
                // SAFETY: a `Timeval` is plain data, viewed as its bytes.
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut(
                        ptr::from_mut(&mut raw).cast::<u8>(),
                        size_of::<sk::Timeval>(),
                    )
                };
                get_opt(socket, level, name, bytes)?;
                timeval_from_kernel(&raw, option_value)
            }
            OptValue::Errno | OptValue::Family => {
                let mut raw = [0u8; 4];
                get_opt(socket, level, name, &mut raw)?;
                let value = c_int::from_ne_bytes(raw);
                let value = if kind == OptValue::Errno {
                    errno::from_kernel(value)
                } else {
                    sk::af_from_kernel(value)
                };
                let bytes = value.to_ne_bytes();
                let n = bytes.len().min(option_value.len());
                option_value[..n].copy_from_slice(&bytes[..n]);
                n
            }
        };
        Ok(n as socklen_t)
    }

    fn setsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &[u8],
    ) -> Result<()> {
        let (level, name, kind) =
            sk::sockopt_to_kernel(level, option_name).ok_or(Errno(ENOPROTOOPT))?;
        match kind {
            OptValue::Timeval => {
                let raw = timeval_to_kernel(option_value)?;
                // SAFETY: a `Timeval` is plain data without padding.
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        ptr::from_ref(&raw).cast::<u8>(),
                        size_of::<sk::Timeval>(),
                    )
                };
                set_opt(socket, level, name, bytes)
            }
            // SO_ERROR, SO_DOMAIN and SO_PROTOCOL are read-only; the stack
            // refuses them itself.
            _ => set_opt(socket, level, name, option_value),
        }
    }

    unsafe fn recvmsg(socket: c_int, msgp: *mut msghdr, flags: c_int) -> Result<usize> {
        // SAFETY: non-null, and otherwise the caller's msghdr.
        let hdr = unsafe { msgp.as_mut() }.ok_or(Errno(EFAULT))?;
        let flags = sk::msg_flags_to_kernel(flags)?;
        let iovs = unsafe { msg_iovs(hdr.msg_iov, hdr.msg_iovlen) };
        let mut data = unsafe { crate::header::bits_iovec::gather(iovs) };
        let mut addr = [0u8; SOCKADDR_MAX];
        let (n, addr_len, reply_flags) = if sock::WIDE_HEADERS {
            // io-sock's recvmsg message also carries ancillary data; without
            // any, it is recvfrom.
            let (n, addr_len) = unsafe {
                recv_data(socket, data.as_mut_ptr(), data.len(), flags, Some(&mut addr))
            }?;
            (n, addr_len, 0)
        } else {
            unsafe { recvmsg_pkt(socket, &mut data, flags, &mut addr) }?
        };
        hdr.msg_namelen = if hdr.msg_name.is_null() {
            0
        } else {
            let dst = unsafe {
                core::slice::from_raw_parts_mut(hdr.msg_name.cast::<u8>(), hdr.msg_namelen as usize)
            };
            sk::sockaddr_from_kernel(&addr[..addr_len], dst) as socklen_t
        };
        hdr.msg_controllen = 0;
        hdr.msg_flags = sk::msg_flags_from_kernel(reply_flags);
        unsafe { crate::header::bits_iovec::scatter(iovs, &data) };
        Ok(n)
    }

    unsafe fn sendmsg(socket: c_int, msgp: *const msghdr, flags: c_int) -> Result<usize> {
        // SAFETY: non-null, and otherwise the caller's msghdr.
        let hdr = unsafe { msgp.as_ref() }.ok_or(Errno(EFAULT))?;
        if !hdr.msg_control.is_null() && hdr.msg_controllen != 0 {
            return Err(Errno(EOPNOTSUPP));
        }
        let flags = sk::msg_flags_to_kernel(flags)?;
        let mut addr = [0u8; SOCKADDR_MAX];
        let addr_len = if hdr.msg_name.is_null() || hdr.msg_namelen == 0 {
            0
        } else {
            let name = unsafe {
                core::slice::from_raw_parts(hdr.msg_name.cast::<u8>(), hdr.msg_namelen as usize)
            };
            kernel_addr(name, &mut addr)?
        };
        let data = unsafe { crate::header::bits_iovec::gather(msg_iovs(hdr.msg_iov, hdr.msg_iovlen)) };
        send_data(socket, &data, flags, &addr[..addr_len])
    }

    fn socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: &mut [c_int; 2]) -> Result<()> {
        let mut oflag = fcntl::O_RDWR;
        if kind & SOCK_CLOEXEC != 0 {
            oflag |= fcntl::O_CLOEXEC;
        }
        if kind & SOCK_NONBLOCK != 0 {
            oflag |= fcntl::O_NONBLOCK;
        }
        let a = open_socket(domain, kind, protocol)?;
        match io::openfd(a, oflag, openfd::PIPE) {
            Ok(b) => {
                sv[0] = a;
                sv[1] = b;
                Ok(())
            }
            Err(err) => {
                let _ = io::close(a);
                Err(err)
            }
        }
    }
}

/// io-pkt's `recvmsg` message (`_IO_XTYPE_TCPIP_MSG`), asking for no
/// ancillary data. Returns the byte count, the address length and the flags
/// the stack reports.
///
/// # Safety
///
/// Only the buffers passed are accessed.
unsafe fn recvmsg_pkt(
    socket: c_int,
    data: &mut [u8],
    flags: c_int,
    addr: &mut [u8; SOCKADDR_MAX],
) -> Result<(usize, usize, c_int)> {
    #[repr(C)]
    struct Header {
        read: IoReadWrite,
        pad: u16,
        flags: u16,
        addrlen: u16,
        controllen: u16,
    }
    #[repr(C)]
    struct Reply {
        pad: u16,
        flags: u16,
        addrlen: u16,
        controllen: u16,
    }
    let header = Header {
        read: IoReadWrite {
            type_: msg::READ,
            combine_len: size_of::<Header>() as u16,
            nbytes: data.len().min(u32::MAX as usize) as u32,
            xtype: xtype::TCPIP_MSG,
            zero: 0,
        },
        pad: 0,
        flags: u16::try_from(flags).map_err(|_| Errno(EOPNOTSUPP))?,
        addrlen: SOCKADDR_MAX as u16,
        controllen: 0,
    };
    let mut reply = Reply {
        pad: 0,
        flags: 0,
        addrlen: 0,
        controllen: 0,
    };
    let send = IoVec::new(ptr::from_ref(&header).cast::<u8>(), size_of::<Header>());
    let mut recv = [
        IoVec::new(ptr::from_mut(&mut reply).cast::<u8>(), size_of::<Reply>()),
        IoVec::new(addr.as_mut_ptr(), SOCKADDR_MAX),
        IoVec::new(data.as_mut_ptr(), data.len()),
    ];
    // SAFETY: every part is a live buffer of the declared length.
    let n = unsafe { msg_sendv(socket, &send, 1, recv.as_mut_ptr(), 3) }?;
    Ok((
        n as usize,
        usize::from(reply.addrlen).min(SOCKADDR_MAX),
        c_int::from(reply.flags),
    ))
}
