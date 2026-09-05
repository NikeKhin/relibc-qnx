//! QNX Neutrino 7.1 version bindings.
//!
//! Everything in this module is specific to the 7.1 kernel ABI. The values are
//! transcribed from the SDP header `services/system/public/sys/kercalls.h`
//! (revision 878035, branch 7.1.0) and cross-checked against the AArch64
//! disassembly of the vendor `libc.so.5`.
//!
//! The rest of this directory is shared with the 8.0 backend, which includes
//! those files from `../qnx8/mod.rs`; they reach version-specific values only
//! through this module. `../qnx8/version.rs` is its 8.0 counterpart and must
//! export the same names.

use crate::platform::types::*;

/// The `svc` immediate used to enter the kernel on AArch64.
///
/// Verified against every kernel-call veneer in the vendor libc, e.g.
/// `ConnectAttach_r`: `mov w8, #0x27; svc #0x51`.
pub const SVC_IMM: u32 = 0x51;

/// Kernel call numbers, in `sys/kercalls.h` enum order.
///
/// The enum is dense and zero-based, so the discriminants below are simply the
/// positional indices of the SDP header's enumerators.
#[allow(dead_code)]
pub mod ker {
    pub const NOP: u32 = 0;
    pub const TRACE_EVENT: u32 = 1;
    pub const RING0: u32 = 2;
    pub const CACHE_FLUSH: u32 = 3;
    pub const SYS_SRANDOM: u32 = 4;
    pub const MSG_REGISTER_EVENT: u32 = 5;
    pub const MSG_SEND_PULSEPTR: u32 = 6;
    pub const SYS_CPUPAGE_GET: u32 = 7;
    pub const SYS_CPUPAGE_SET: u32 = 8;
    pub const MSG_PAUSE: u32 = 9;
    pub const MSG_CURRENT: u32 = 10;
    pub const MSG_SENDV: u32 = 11;
    pub const MSG_SENDVNC: u32 = 12;
    pub const MSG_ERROR: u32 = 13;
    pub const MSG_RECEIVEV: u32 = 14;
    pub const MSG_REPLYV: u32 = 15;
    pub const MSG_READV: u32 = 16;
    pub const MSG_WRITEV: u32 = 17;
    pub const MSG_READWRITEV: u32 = 18;
    pub const MSG_INFO: u32 = 19;
    pub const MSG_SEND_PULSE: u32 = 20;
    pub const MSG_DELIVER_EVENT: u32 = 21;
    pub const MSG_KEYDATA: u32 = 22;
    pub const MSG_READIOV: u32 = 23;
    pub const MSG_RECEIVEPULSEV: u32 = 24;
    pub const MSG_VERIFY_EVENT: u32 = 25;
    pub const SIGNAL_KILL: u32 = 26;
    pub const SIGNAL_RETURN: u32 = 27;
    pub const SIGNAL_FAULT: u32 = 28;
    pub const SIGNAL_ACTION: u32 = 29;
    pub const SIGNAL_PROCMASK: u32 = 30;
    pub const SIGNAL_SUSPEND: u32 = 31;
    pub const SIGNAL_WAITINFO: u32 = 32;
    pub const SIGNAL_KILL_SIGVAL: u32 = 33;
    pub const CHANNEL_CREATE: u32 = 35;
    pub const CHANNEL_DESTROY: u32 = 36;
    pub const CHANCON_ATTR: u32 = 37;
    pub const CONNECT_ATTACH: u32 = 39;
    pub const CONNECT_DETACH: u32 = 40;
    pub const CONNECT_SERVER_INFO: u32 = 41;
    pub const CONNECT_CLIENT_INFO: u32 = 42;
    pub const CONNECT_FLAGS: u32 = 43;
    pub const THREAD_CREATE: u32 = 46;
    pub const THREAD_DESTROY: u32 = 47;
    pub const THREAD_DESTROYALL: u32 = 48;
    pub const THREAD_DETACH: u32 = 49;
    pub const THREAD_JOIN: u32 = 50;
    pub const THREAD_CANCEL: u32 = 51;
    pub const THREAD_CTL: u32 = 52;
    pub const THREAD_CTLEXT: u32 = 53;
    pub const INTERRUPT_ATTACH: u32 = 55;
    pub const INTERRUPT_DETACH_FUNC: u32 = 56;
    pub const INTERRUPT_DETACH: u32 = 57;
    pub const INTERRUPT_WAIT: u32 = 58;
    pub const INTERRUPT_MASK: u32 = 59;
    pub const INTERRUPT_UNMASK: u32 = 60;
    pub const INTERRUPT_CHARACTERISTIC: u32 = 61;
    pub const CLOCK_TIME: u32 = 65;
    pub const CLOCK_ADJUST: u32 = 66;
    pub const CLOCK_PERIOD: u32 = 67;
    pub const CLOCK_ID: u32 = 68;
    pub const TIMER_CREATE: u32 = 70;
    pub const TIMER_DESTROY: u32 = 71;
    pub const TIMER_SETTIME: u32 = 72;
    pub const TIMER_INFO: u32 = 73;
    pub const TIMER_ALARM: u32 = 74;
    pub const TIMER_TIMEOUT: u32 = 75;
    pub const SYNC_CREATE: u32 = 78;
    pub const SYNC_DESTROY: u32 = 79;
    pub const SYNC_MUTEX_LOCK: u32 = 80;
    pub const SYNC_MUTEX_UNLOCK: u32 = 81;
    pub const SYNC_CONDVAR_WAIT: u32 = 82;
    pub const SYNC_CONDVAR_SIGNAL: u32 = 83;
    pub const SYNC_SEM_POST: u32 = 84;
    pub const SYNC_SEM_WAIT: u32 = 85;
    pub const SYNC_CTL: u32 = 86;
    pub const SYNC_MUTEX_REVIVE: u32 = 87;
    pub const SCHED_GET: u32 = 88;
    pub const SCHED_SET: u32 = 89;
    pub const SCHED_YIELD: u32 = 90;
    pub const SCHED_INFO: u32 = 91;
    pub const SCHED_CTL: u32 = 92;
    pub const NET_CRED: u32 = 93;
    pub const NET_VTID: u32 = 94;
    pub const NET_UNBLOCK: u32 = 95;
    pub const NET_INFOSCOID: u32 = 96;
    pub const NET_SIGNAL_KILL: u32 = 97;
    pub const POWER_PARAMETER: u32 = 100;
    pub const POWER_ACTIVE: u32 = 101;
    pub const SCHED_WAYPOINT: u32 = 102;
    pub const SYS_CUSTOM: u32 = 106;
    pub const BAD: u32 = 107;
}

/// Offsets into the kernel-supplied thread local storage block addressed by
/// `tpidrro_el0` (`struct _thread_local_storage`).
///
/// Verified against the vendor libc:
/// * `__get_errno_ptr`: `mrs x0, tpidrro_el0; ldr x0, [x0, #16]`
/// * `getpid`: `mrs x0, tpidrro_el0; ldr w0, [x0, #32]`
/// * `gettid`: `mrs x0, tpidrro_el0; ldr w0, [x0, #36]`
#[allow(dead_code)]
pub mod tls {
    pub const EXITFUNC: usize = 0;
    pub const ARG: usize = 8;
    pub const ERRPTR: usize = 16;
    pub const ERRVAL: usize = 24;
    pub const FLAGS: usize = 28;
    pub const PID: usize = 32;
    pub const TID: usize = 36;
    pub const OWNER: usize = 40;
    pub const STACKADDR: usize = 48;
}

/// `SysCpupageGet` indices (`CPUPAGE_*`).
///
/// `tpidrro_el0` is supposed to hold the TLS pointer already; this kernel call
/// is the fallback if the register is still zero when we first need pid/tid.
///
/// `CPUPAGE_ADDR` is -1 and `CPUPAGE_TLS` 0, per the enum in the vendor debug
/// information (and `_init_libc` registering the PLS at index 1). The earlier
/// transcription started at `ADDR = 0`, which made the fallback fetch the PLS.
#[allow(dead_code)]
pub mod cpupage {
    pub const ADDR: usize = usize::MAX;
    pub const TLS: usize = 0;
    pub const PLS: usize = 1;
    pub const SYSPAGE: usize = 2;
}

/// `struct sigaction` as the 7.1 kernel sees it: a handler, flags, and an
/// 8-byte signal mask, 24 bytes in total (DWARF). 7.1's `sigset_t` is
/// `_Uint32t __bits[2]`, so the mask packs at offset 12. (8.0 widened
/// `sigset_t` to one 64-bit word, pushing the mask to offset 16; see
/// `../qnx8/version.rs`.) The shared flag translation in `abi::neutrino`
/// reaches the fields only through [`Sigaction::new`] and the accessors, so it
/// does not care which layout it is talking to.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Sigaction {
    /// The handler, whichever arm of the union is in use.
    handler: usize,
    flags: c_int,
    mask: [u32; 2],
    reserved: u32,
}

impl Sigaction {
    pub fn new(handler: usize, flags: c_int, mask: [u32; 2]) -> Self {
        Self {
            handler,
            flags,
            mask,
            reserved: 0,
        }
    }
    pub fn handler(&self) -> usize {
        self.handler
    }
    pub fn flags(&self) -> c_int {
        self.flags
    }
    pub fn mask(&self) -> [u32; 2] {
        self.mask
    }
}

/// `struct _msg_info`, filled in by `ConnectServerInfo`, `MsgInfo` and
/// `MsgReceive` (48 bytes on 7.1).
///
/// Shared code only reads the leading `nd`..`coid` fields, which sit at the
/// same offsets on 8.0.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MsgInfo {
    pub nd: u32,
    pub srcnd: u32,
    pub pid: pid_t,
    pub tid: i32,
    pub chid: i32,
    pub scoid: i32,
    pub coid: i32,
    pub msglen: i32,
    pub srcmsglen: i32,
    pub dstmsglen: i32,
    pub priority: i16,
    pub flags: i16,
    pub reserved: i32,
}

/// `_IO_CONNECT_EXTRA_LINK`, the extra type carried by a `link()` request.
pub const CONNECT_EXTRA_LINK: u8 = 1;

/// `sigevent::sigev_notify` for a 64-bit 7.1 process: `SIGEV_64BIT` (0x80)
/// ORed with the notification type. Not re-verified against a 7.1 SDP here;
/// 8.0 dropped the flag (see `../qnx8/version.rs`).
#[allow(dead_code)]
pub mod sigev {
    pub const NONE: i32 = 0x80;
    pub const SIGNAL: i32 = 0x81;
    pub const THREAD: i32 = 0x87;
}

/// Whether procnto speaks the 7.1 `_PROC_SPAWN` message that `proc::spawn`
/// builds.
pub const HAS_PROC_SPAWN: bool = true;

/// Whether `ThreadCtl(_NTO_TCTL_SIGSTACK)` sets the alternate signal stack.
/// 8.0's vendor `sigaltstack` is built on it; for 7.1 it is unverified, so
/// `sigaltstack` refuses to install a stack it cannot prove the kernel uses.
pub const HAS_TCTL_SIGSTACK: bool = false;

/// Socket numbering the two network stacks disagree on, and the switches for
/// the socket message protocol (see `socket.rs`).
///
/// io-pkt follows NetBSD. These were not re-read from a 7.1 SDP; the
/// self-test's `sockets` step checks them on a target.
pub mod sock {
    use crate::platform::types::c_int;

    pub const AF_INET6: c_int = 24;
    pub const SO_SNDTIMEO: c_int = 0x100b;
    pub const SO_RCVTIMEO: c_int = 0x100c;
    /// io-pkt has neither; 0 turns them off.
    pub const SO_PROTOCOL: c_int = 0;
    pub const SO_DOMAIN: c_int = 0;
    pub const MSG_NOSIGNAL: c_int = 0x0400;
    pub const MSG_CMSG_CLOEXEC: c_int = 0x0800;
    pub const TCP_KEEPIDLE: c_int = 3;
    pub const TCP_KEEPINTVL: c_int = 5;
    pub const TCP_KEEPCNT: c_int = 6;
    /// Unused: io-pkt takes non-blocking mode from the open flags.
    pub const SOCK_NONBLOCK: c_int = 0;

    /// `socket()` opens `/dev/socket/<domain>.<type>.<protocol>`.
    pub const OPEN_EXTRA: bool = false;
    /// `sendto`/`recvfrom` carry 16-bit flags and address lengths.
    pub const WIDE_HEADERS: bool = false;
    /// `_IO_SOCK_SOPT2` / `_IO_SOCK_GOPT2`.
    pub const SETOPT: u16 = 5;
    pub const GETOPT: u16 = 6;
}

/// Path `pipe()` resolves, with `_FTYPE_PIPE`, to reach the pipe manager.
pub const PIPE_PATH: &core::ffi::CStr = c"/dev/pipe";
