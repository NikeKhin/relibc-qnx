//! The socket ABI of QNX's network stacks, and its translation from relibc's.
//!
//! relibc's socket ABI is Linux's. io-pkt (7.1) and io-sock (8.0) speak BSD's:
//! an address starts with a length byte and a one-byte family, `SOL_SOCKET` is
//! 0xffff, and options and message flags are numbered differently. The values
//! below are the ones both stacks share; those they number differently from
//! each other are in `version::sock`.

use crate::{
    error::{Errno, Result},
    header::{
        errno::{EAFNOSUPPORT, EINVAL, ENAMETOOLONG, EOPNOTSUPP},
        netinet_in as re_in, netinet_tcp as re_tcp,
        sys_socket::constants as re,
    },
    platform::{sys::version::sock, types::c_int},
};

/// Longest socket address exchanged with the stack (`sockaddr_storage`).
pub const SOCKADDR_MAX: usize = 128;
/// `sizeof(struct sockaddr_in)`.
pub const SOCKADDR_IN_LEN: usize = 16;
/// `sizeof(struct sockaddr_in6)`.
pub const SOCKADDR_IN6_LEN: usize = 28;
/// `sizeof(struct sockaddr_un)`: `sun_path` holds 104 bytes, relibc's 108.
pub const SOCKADDR_UN_LEN: usize = 106;

/// Where `struct sockaddr` keeps its length and family bytes; the members
/// after them sit at the same offsets as in relibc's.
pub const SA_LEN: usize = 0;
pub const SA_FAMILY: usize = 1;

pub const SOL_SOCKET: c_int = 0xffff;

/// `struct timeval`, whose `tv_usec` is 32 bits wide where relibc's is 64.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i32,
    pub pad: u32,
}

/// Address families.
pub mod af {
    use crate::platform::types::c_int;

    pub const UNSPEC: c_int = 0;
    pub const UNIX: c_int = 1;
    pub const INET: c_int = 2;
}

/// `SOL_SOCKET` options.
pub mod so {
    use crate::platform::types::c_int;

    pub const DEBUG: c_int = 0x0001;
    pub const ACCEPTCONN: c_int = 0x0002;
    pub const REUSEADDR: c_int = 0x0004;
    pub const KEEPALIVE: c_int = 0x0008;
    pub const DONTROUTE: c_int = 0x0010;
    pub const BROADCAST: c_int = 0x0020;
    pub const LINGER: c_int = 0x0080;
    pub const OOBINLINE: c_int = 0x0100;
    pub const REUSEPORT: c_int = 0x0200;
    pub const SNDBUF: c_int = 0x1001;
    pub const RCVBUF: c_int = 0x1002;
    pub const SNDLOWAT: c_int = 0x1003;
    pub const RCVLOWAT: c_int = 0x1004;
    pub const ERROR: c_int = 0x1007;
    pub const TYPE: c_int = 0x1008;
}

/// Message flags.
pub mod msg {
    use crate::platform::types::c_int;

    pub const OOB: c_int = 0x0001;
    pub const PEEK: c_int = 0x0002;
    pub const DONTROUTE: c_int = 0x0004;
    pub const EOR: c_int = 0x0008;
    pub const TRUNC: c_int = 0x0010;
    pub const CTRUNC: c_int = 0x0020;
    pub const WAITALL: c_int = 0x0040;
    pub const DONTWAIT: c_int = 0x0080;
}

/// `IPPROTO_IP` options.
pub mod ip {
    use crate::platform::types::c_int;

    pub const TOS: c_int = 3;
    pub const TTL: c_int = 4;
    pub const MULTICAST_IF: c_int = 9;
    pub const MULTICAST_TTL: c_int = 10;
    pub const MULTICAST_LOOP: c_int = 11;
    pub const ADD_MEMBERSHIP: c_int = 12;
    pub const DROP_MEMBERSHIP: c_int = 13;
}

/// `IPPROTO_IPV6` options.
pub mod ipv6 {
    use crate::platform::types::c_int;

    pub const UNICAST_HOPS: c_int = 4;
    pub const MULTICAST_IF: c_int = 9;
    pub const MULTICAST_HOPS: c_int = 10;
    pub const MULTICAST_LOOP: c_int = 11;
    pub const JOIN_GROUP: c_int = 12;
    pub const LEAVE_GROUP: c_int = 13;
    pub const V6ONLY: c_int = 27;
}

/// `IPPROTO_TCP` options.
pub mod tcp {
    use crate::platform::types::c_int;

    pub const NODELAY: c_int = 1;
    pub const MAXSEG: c_int = 2;
}

pub fn af_to_kernel(family: c_int) -> Option<c_int> {
    Some(match family {
        re::AF_UNSPEC => af::UNSPEC,
        re::AF_UNIX => af::UNIX,
        re::AF_INET => af::INET,
        re::AF_INET6 => sock::AF_INET6,
        _ => return None,
    })
}

/// Families relibc has no name for pass through, as `AF_LINK` from the stack.
pub fn af_from_kernel(family: c_int) -> c_int {
    match family {
        af::UNSPEC => re::AF_UNSPEC,
        af::UNIX => re::AF_UNIX,
        af::INET => re::AF_INET,
        sock::AF_INET6 => re::AF_INET6,
        other => other,
    }
}

/// Rewrites a relibc address into `dst` in the stack's layout and returns its
/// length. The members after the family sit at the same offsets in both.
pub fn sockaddr_to_kernel(src: &[u8], dst: &mut [u8; SOCKADDR_MAX]) -> Result<usize> {
    if src.len() < 2 {
        return Err(Errno(EINVAL));
    }
    let family = c_int::from(u16::from_ne_bytes([src[0], src[1]]));
    let qnx_family = af_to_kernel(family).ok_or(Errno(EAFNOSUPPORT))?;
    let len = match qnx_family {
        af::INET => SOCKADDR_IN_LEN,
        sock::AF_INET6 => SOCKADDR_IN6_LEN,
        af::UNIX => {
            let path = &src[2..];
            let path_len = path.iter().position(|&b| b == 0).unwrap_or(path.len());
            if 2 + path_len > SOCKADDR_UN_LEN {
                return Err(Errno(ENAMETOOLONG));
            }
            2 + path_len
        }
        _ => src.len().min(SOCKADDR_MAX),
    };
    if src.len() < len {
        return Err(Errno(EINVAL));
    }
    *dst = [0; SOCKADDR_MAX];
    dst[SA_LEN] = len as u8;
    dst[SA_FAMILY] = qnx_family as u8;
    dst[2..len].copy_from_slice(&src[2..len]);
    Ok(len)
}

/// Rewrites an address from the stack into relibc's layout, truncated to
/// `dst`, and returns its full length as POSIX reports it.
pub fn sockaddr_from_kernel(src: &[u8], dst: &mut [u8]) -> usize {
    if src.len() < 2 {
        return 0;
    }
    let len = match usize::from(src[SA_LEN]) {
        0 => src.len(),
        n => n.clamp(2, src.len()),
    };
    let family = af_from_kernel(c_int::from(src[SA_FAMILY])) as u16;
    let mut out = [0u8; SOCKADDR_MAX];
    out[..2].copy_from_slice(&family.to_ne_bytes());
    out[2..len].copy_from_slice(&src[2..len]);
    let n = len.min(dst.len());
    dst[..n].copy_from_slice(&out[..n]);
    len
}

/// How an option's value crosses between the two ABIs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptValue {
    /// Laid out alike (`int`, `struct linger`, `struct ip_mreq`, ...).
    Plain,
    /// `struct timeval`, whose `tv_usec` is 32 bits wide on QNX.
    Timeval,
    /// An errno, numbered the QNX way.
    Errno,
    /// An address family.
    Family,
}

/// Translates a relibc `(level, name)` pair; `None` means the stack has no
/// such option.
pub fn sockopt_to_kernel(level: c_int, name: c_int) -> Option<(c_int, c_int, OptValue)> {
    let plain = |name| Some((level, name, OptValue::Plain));
    match level {
        re::SOL_SOCKET => {
            let (name, value) = match name {
                re::SO_DEBUG => (so::DEBUG, OptValue::Plain),
                re::SO_ACCEPTCONN => (so::ACCEPTCONN, OptValue::Plain),
                re::SO_REUSEADDR => (so::REUSEADDR, OptValue::Plain),
                re::SO_KEEPALIVE => (so::KEEPALIVE, OptValue::Plain),
                re::SO_DONTROUTE => (so::DONTROUTE, OptValue::Plain),
                re::SO_BROADCAST => (so::BROADCAST, OptValue::Plain),
                re::SO_LINGER => (so::LINGER, OptValue::Plain),
                re::SO_OOBINLINE => (so::OOBINLINE, OptValue::Plain),
                re::SO_REUSEPORT => (so::REUSEPORT, OptValue::Plain),
                re::SO_SNDBUF => (so::SNDBUF, OptValue::Plain),
                re::SO_RCVBUF => (so::RCVBUF, OptValue::Plain),
                re::SO_SNDLOWAT => (so::SNDLOWAT, OptValue::Plain),
                re::SO_RCVLOWAT => (so::RCVLOWAT, OptValue::Plain),
                re::SO_SNDTIMEO => (sock::SO_SNDTIMEO, OptValue::Timeval),
                re::SO_RCVTIMEO => (sock::SO_RCVTIMEO, OptValue::Timeval),
                re::SO_ERROR => (so::ERROR, OptValue::Errno),
                re::SO_TYPE => (so::TYPE, OptValue::Plain),
                re::SO_PROTOCOL if sock::SO_PROTOCOL != 0 => (sock::SO_PROTOCOL, OptValue::Plain),
                re::SO_DOMAIN if sock::SO_DOMAIN != 0 => (sock::SO_DOMAIN, OptValue::Family),
                _ => return None,
            };
            Some((SOL_SOCKET, name, value))
        }
        _ if level == c_int::from(re_in::IPPROTO_IP) => plain(match name {
            re_in::IP_TOS => ip::TOS,
            re_in::IP_TTL => ip::TTL,
            re_in::IP_MULTICAST_IF => ip::MULTICAST_IF,
            re_in::IP_MULTICAST_TTL => ip::MULTICAST_TTL,
            re_in::IP_MULTICAST_LOOP => ip::MULTICAST_LOOP,
            re_in::IP_ADD_MEMBERSHIP => ip::ADD_MEMBERSHIP,
            re_in::IP_DROP_MEMBERSHIP => ip::DROP_MEMBERSHIP,
            _ => return None,
        }),
        _ if level == c_int::from(re_in::IPPROTO_IPV6) => plain(match name {
            re_in::IPV6_UNICAST_HOPS => ipv6::UNICAST_HOPS,
            re_in::IPV6_MULTICAST_IF => ipv6::MULTICAST_IF,
            re_in::IPV6_MULTICAST_HOPS => ipv6::MULTICAST_HOPS,
            re_in::IPV6_MULTICAST_LOOP => ipv6::MULTICAST_LOOP,
            re_in::IPV6_JOIN_GROUP => ipv6::JOIN_GROUP,
            re_in::IPV6_LEAVE_GROUP => ipv6::LEAVE_GROUP,
            re_in::IPV6_V6ONLY => ipv6::V6ONLY,
            _ => return None,
        }),
        _ if level == c_int::from(re_in::IPPROTO_TCP) => plain(match name {
            re_tcp::TCP_NODELAY => tcp::NODELAY,
            re_tcp::TCP_MAXSEG => tcp::MAXSEG,
            re_tcp::TCP_KEEPIDLE => sock::TCP_KEEPIDLE,
            re_tcp::TCP_KEEPINTVL => sock::TCP_KEEPINTVL,
            re_tcp::TCP_KEEPCNT => sock::TCP_KEEPCNT,
            _ => return None,
        }),
        _ => None,
    }
}

const MSG_FLAGS: [(c_int, c_int); 10] = [
    (re::MSG_OOB, msg::OOB),
    (re::MSG_PEEK, msg::PEEK),
    (re::MSG_DONTROUTE, msg::DONTROUTE),
    (re::MSG_EOR, msg::EOR),
    (re::MSG_TRUNC, msg::TRUNC),
    (re::MSG_CTRUNC, msg::CTRUNC),
    (re::MSG_WAITALL, msg::WAITALL),
    (re::MSG_DONTWAIT, msg::DONTWAIT),
    (re::MSG_NOSIGNAL, sock::MSG_NOSIGNAL),
    (re::MSG_CMSG_CLOEXEC, sock::MSG_CMSG_CLOEXEC),
];

/// Translates `send`/`recv` flags; a flag the stack has no equivalent for is
/// refused rather than dropped.
pub fn msg_flags_to_kernel(flags: c_int) -> Result<c_int> {
    let mut out = 0;
    let mut rest = flags;
    for (relibc, qnx) in MSG_FLAGS {
        if flags & relibc != 0 {
            out |= qnx;
            rest &= !relibc;
        }
    }
    if rest != 0 {
        return Err(Errno(EOPNOTSUPP));
    }
    Ok(out)
}

/// Translates the flags `recvmsg` reports.
pub fn msg_flags_from_kernel(flags: c_int) -> c_int {
    MSG_FLAGS
        .iter()
        .filter(|(_, qnx)| flags & qnx != 0)
        .fold(0, |out, (relibc, _)| out | relibc)
}
