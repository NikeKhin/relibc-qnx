//! QNX Neutrino `errno` values and their translation to relibc's numbering.
//!
//! relibc defines its own `errno.h` (currently the Linux numbering) and uses it
//! consistently on every platform, so the values the Neutrino kernel reports
//! have to be translated at the kernel-call boundary — exactly the way the
//! Redox backend translates its own kernel's error codes.
//!
//! The QNX numbering is recovered from `sys_errlist`, an array of pointers
//! indexed by error number. Each slot's `R_AARCH64_RELATIVE` addend points at
//! its message, so resolving the relocations gives the table exactly, gaps
//! included. The QNX numbering is sparse: unused slots all point at one shared
//! `"Unknown error"` string, and the networking errors start at 236. That is
//! why the message *pool* cannot be used instead — the linker merges the
//! duplicate placeholders, so counting pooled strings drops every gap and
//! shifts every error above the first one (59) downwards. `ENOSYS` is 89
//! ("Function not implemented"); 84 is `ELIBBAD`.
//!
//! Verified against the QNX 8.0 `libc.so.6`. The numbering is the same on 7.1:
//! the vendor `errno.h` has not changed, and the earlier 7.1 table differed
//! only by exactly the number of merged placeholders below each value.
//!
//! Only the kernel-facing direction is needed. Nothing in relibc hands a QNX
//! error number back to the kernel.

use crate::{header::errno as re, platform::types::c_int};

/// Error numbers as reported by the Neutrino kernel and by resource managers.
#[allow(dead_code)]
pub mod qnx {
    use crate::platform::types::c_int;

    pub const EOK: c_int = 0;
    pub const EPERM: c_int = 1;
    pub const ENOENT: c_int = 2;
    pub const ESRCH: c_int = 3;
    pub const EINTR: c_int = 4;
    pub const EIO: c_int = 5;
    pub const ENXIO: c_int = 6;
    pub const E2BIG: c_int = 7;
    pub const ENOEXEC: c_int = 8;
    pub const EBADF: c_int = 9;
    pub const ECHILD: c_int = 10;
    pub const EAGAIN: c_int = 11;
    pub const ENOMEM: c_int = 12;
    pub const EACCES: c_int = 13;
    pub const EFAULT: c_int = 14;
    pub const ENOTBLK: c_int = 15;
    pub const EBUSY: c_int = 16;
    pub const EEXIST: c_int = 17;
    pub const EXDEV: c_int = 18;
    pub const ENODEV: c_int = 19;
    pub const ENOTDIR: c_int = 20;
    pub const EISDIR: c_int = 21;
    pub const EINVAL: c_int = 22;
    pub const ENFILE: c_int = 23;
    pub const EMFILE: c_int = 24;
    pub const ENOTTY: c_int = 25;
    pub const ETXTBSY: c_int = 26;
    pub const EFBIG: c_int = 27;
    pub const ENOSPC: c_int = 28;
    pub const ESPIPE: c_int = 29;
    pub const EROFS: c_int = 30;
    pub const EMLINK: c_int = 31;
    pub const EPIPE: c_int = 32;
    pub const EDOM: c_int = 33;
    pub const ERANGE: c_int = 34;
    pub const ENOMSG: c_int = 35;
    pub const EIDRM: c_int = 36;
    pub const ECHRNG: c_int = 37;
    pub const EL2NSYNC: c_int = 38;
    pub const EL3HLT: c_int = 39;
    pub const EL3RST: c_int = 40;
    pub const ELNRNG: c_int = 41;
    pub const EUNATCH: c_int = 42;
    pub const ENOCSI: c_int = 43;
    pub const EL2HLT: c_int = 44;
    pub const EDEADLK: c_int = 45;
    pub const ENOLCK: c_int = 46;
    pub const ECANCELED: c_int = 47;
    pub const ENOTSUP: c_int = 48;
    pub const EDQUOT: c_int = 49;
    pub const EBADE: c_int = 50;
    pub const EBADR: c_int = 51;
    pub const EXFULL: c_int = 52;
    pub const ENOANO: c_int = 53;
    pub const EBADRQC: c_int = 54;
    pub const EBADSLT: c_int = 55;
    pub const EDEADLOCK: c_int = 56;
    pub const EBFONT: c_int = 57;
    pub const EOWNERDEAD: c_int = 58;
    pub const ENOSTR: c_int = 60;
    pub const ENODATA: c_int = 61;
    pub const ETIME: c_int = 62;
    pub const ENOSR: c_int = 63;
    pub const ENONET: c_int = 64;
    pub const ENOPKG: c_int = 65;
    pub const EREMOTE: c_int = 66;
    pub const ENOLINK: c_int = 67;
    pub const EADV: c_int = 68;
    pub const ESRMNT: c_int = 69;
    pub const ECOMM: c_int = 70;
    pub const EPROTO: c_int = 71;
    pub const EMULTIHOP: c_int = 74;
    pub const EBADMSG: c_int = 77;
    pub const ENAMETOOLONG: c_int = 78;
    pub const EOVERFLOW: c_int = 79;
    pub const ENOTUNIQ: c_int = 80;
    pub const EBADFD: c_int = 81;
    pub const EREMCHG: c_int = 82;
    pub const ELIBACC: c_int = 83;
    pub const ELIBBAD: c_int = 84;
    pub const ELIBSCN: c_int = 85;
    pub const ELIBMAX: c_int = 86;
    pub const ELIBEXEC: c_int = 87;
    pub const EILSEQ: c_int = 88;
    pub const ENOSYS: c_int = 89;
    pub const ELOOP: c_int = 90;
    pub const ERESTART: c_int = 91;
    pub const ESTRPIPE: c_int = 92;
    pub const ENOTEMPTY: c_int = 93;
    pub const EUSERS: c_int = 94;
    pub const ENOTRECOVERABLE: c_int = 95;
    pub const EOPNOTSUPP: c_int = 103;
    /// "File position error".
    pub const EFPOS: c_int = 110;
    /// "Potentially recoverable i/o error".
    pub const ESTALE: c_int = 122;
    pub const EINPROGRESS: c_int = 236;
    pub const EALREADY: c_int = 237;
    pub const ENOTSOCK: c_int = 238;
    pub const EDESTADDRREQ: c_int = 239;
    pub const EMSGSIZE: c_int = 240;
    pub const EPROTOTYPE: c_int = 241;
    pub const ENOPROTOOPT: c_int = 242;
    pub const EPROTONOSUPPORT: c_int = 243;
    pub const ESOCKTNOSUPPORT: c_int = 244;
    pub const EPFNOSUPPORT: c_int = 246;
    pub const EAFNOSUPPORT: c_int = 247;
    pub const EADDRINUSE: c_int = 248;
    pub const EADDRNOTAVAIL: c_int = 249;
    pub const ENETDOWN: c_int = 250;
    pub const ENETUNREACH: c_int = 251;
    pub const ENETRESET: c_int = 252;
    pub const ECONNABORTED: c_int = 253;
    pub const ECONNRESET: c_int = 254;
    pub const ENOBUFS: c_int = 255;
    pub const EISCONN: c_int = 256;
    pub const ENOTCONN: c_int = 257;
    pub const ESHUTDOWN: c_int = 258;
    pub const ETOOMANYREFS: c_int = 259;
    pub const ETIMEDOUT: c_int = 260;
    pub const ECONNREFUSED: c_int = 261;
    pub const EHOSTDOWN: c_int = 264;
    pub const EHOSTUNREACH: c_int = 265;
    /// "Corrupted file system detected".
    pub const EBADFSYS: c_int = 302;
    /// "Server fault on msg pass": the server faulted accessing our buffers.
    pub const ESRVRFAULT: c_int = 312;
}

/// Translates a Neutrino error number into relibc's `errno.h` numbering.
///
/// The first 34 values are identical in both. Every other real QNX value below
/// 134 is mapped explicitly, because passing one through would collide with an
/// unrelated relibc error — QNX 110 is `EFPOS`, but relibc 110 is `ETIMEDOUT`.
/// Values from 134 up are QNX-only (RPC and Neutrino-internal errors), cannot
/// collide, and are passed through so they stay distinguishable.
pub fn from_kernel(err: c_int) -> c_int {
    match err {
        0..=34 => err,
        qnx::ENOMSG => re::ENOMSG,
        qnx::EIDRM => re::EIDRM,
        qnx::ECHRNG => re::ECHRNG,
        qnx::EL2NSYNC => re::EL2NSYNC,
        qnx::EL3HLT => re::EL3HLT,
        qnx::EL3RST => re::EL3RST,
        qnx::ELNRNG => re::ELNRNG,
        qnx::EUNATCH => re::EUNATCH,
        qnx::ENOCSI => re::ENOCSI,
        qnx::EL2HLT => re::EL2HLT,
        qnx::EDEADLK => re::EDEADLK,
        qnx::ENOLCK => re::ENOLCK,
        qnx::ECANCELED => re::ECANCELED,
        qnx::ENOTSUP => re::ENOTSUP,
        qnx::EDQUOT => re::EDQUOT,
        qnx::EBADE => re::EBADE,
        qnx::EBADR => re::EBADR,
        qnx::EXFULL => re::EXFULL,
        qnx::ENOANO => re::ENOANO,
        qnx::EBADRQC => re::EBADRQC,
        qnx::EBADSLT => re::EBADSLT,
        qnx::EDEADLOCK => re::EDEADLK,
        qnx::EBFONT => re::EBFONT,
        qnx::EOWNERDEAD => re::EOWNERDEAD,
        qnx::ENOSTR => re::ENOSTR,
        qnx::ENODATA => re::ENODATA,
        qnx::ETIME => re::ETIME,
        qnx::ENOSR => re::ENOSR,
        qnx::ENONET => re::ENONET,
        qnx::ENOPKG => re::ENOPKG,
        qnx::EREMOTE => re::EREMOTE,
        qnx::ENOLINK => re::ENOLINK,
        qnx::EADV => re::EADV,
        qnx::ESRMNT => re::ESRMNT,
        qnx::ECOMM => re::ECOMM,
        qnx::EPROTO => re::EPROTO,
        qnx::EMULTIHOP => re::EMULTIHOP,
        qnx::EBADMSG => re::EBADMSG,
        qnx::ENAMETOOLONG => re::ENAMETOOLONG,
        qnx::EOVERFLOW => re::EOVERFLOW,
        qnx::ENOTUNIQ => re::ENOTUNIQ,
        qnx::EBADFD => re::EBADFD,
        qnx::EREMCHG => re::EREMCHG,
        qnx::ELIBACC => re::ELIBACC,
        qnx::ELIBBAD => re::ELIBBAD,
        qnx::ELIBSCN => re::ELIBSCN,
        qnx::ELIBMAX => re::ELIBMAX,
        qnx::ELIBEXEC => re::ELIBEXEC,
        qnx::EILSEQ => re::EILSEQ,
        qnx::ENOSYS => re::ENOSYS,
        qnx::ELOOP => re::ELOOP,
        qnx::ERESTART => re::ERESTART,
        qnx::ESTRPIPE => re::ESTRPIPE,
        qnx::ENOTEMPTY => re::ENOTEMPTY,
        qnx::EUSERS => re::EUSERS,
        qnx::ENOTRECOVERABLE => re::ENOTRECOVERABLE,
        qnx::EOPNOTSUPP => re::EOPNOTSUPP,
        // No relibc equivalent. EINVAL is the honest generic; passing 110
        // through would read as ETIMEDOUT.
        qnx::EFPOS => re::EINVAL,
        qnx::ESTALE => re::ESTALE,
        // Neutrino's filesystem-integrity error; the I/O is what failed.
        qnx::EBADFSYS => re::EIO,
        qnx::ESRVRFAULT => re::EFAULT,
        qnx::EINPROGRESS => re::EINPROGRESS,
        qnx::EALREADY => re::EALREADY,
        qnx::ENOTSOCK => re::ENOTSOCK,
        qnx::EDESTADDRREQ => re::EDESTADDRREQ,
        qnx::EMSGSIZE => re::EMSGSIZE,
        qnx::EPROTOTYPE => re::EPROTOTYPE,
        qnx::ENOPROTOOPT => re::ENOPROTOOPT,
        qnx::EPROTONOSUPPORT => re::EPROTONOSUPPORT,
        qnx::ESOCKTNOSUPPORT => re::ESOCKTNOSUPPORT,
        qnx::EPFNOSUPPORT => re::EPFNOSUPPORT,
        qnx::EAFNOSUPPORT => re::EAFNOSUPPORT,
        qnx::EADDRINUSE => re::EADDRINUSE,
        qnx::EADDRNOTAVAIL => re::EADDRNOTAVAIL,
        qnx::ENETDOWN => re::ENETDOWN,
        qnx::ENETUNREACH => re::ENETUNREACH,
        qnx::ENETRESET => re::ENETRESET,
        qnx::ECONNABORTED => re::ECONNABORTED,
        qnx::ECONNRESET => re::ECONNRESET,
        qnx::ENOBUFS => re::ENOBUFS,
        qnx::EISCONN => re::EISCONN,
        qnx::ENOTCONN => re::ENOTCONN,
        qnx::ESHUTDOWN => re::ESHUTDOWN,
        qnx::ETOOMANYREFS => re::ETOOMANYREFS,
        qnx::ETIMEDOUT => re::ETIMEDOUT,
        qnx::ECONNREFUSED => re::ECONNREFUSED,
        qnx::EHOSTDOWN => re::EHOSTDOWN,
        qnx::EHOSTUNREACH => re::EHOSTUNREACH,
        other => other,
    }
}
