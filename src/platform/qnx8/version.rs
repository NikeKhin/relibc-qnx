//! QNX Neutrino 8.0 version bindings.
//!
//! The 8.0 counterpart of `../qnx7/version.rs`; it exports the same names. The
//! values were recovered from the SDP 8.0 `libc.so.6` (GCC 12.2,
//! `QNXOS_qos804` build): kernel-call numbers from the `mov w8, #n; svc #0x51`
//! veneers, layouts and enums from the DWARF in `libc.so.6.sym`.
//!
//! Every kernel call the shared code issues kept its 7.1 number. What changed
//! is around them: 42 is now `ConnectClientInfoExtraArgs` and 32
//! `SignalWaitinfoMask` (both take extra arguments, which `kernel` passes as
//! zero on both versions), `ThreadDestroyAll` is gone, and the Qnet calls at
//! 93-97 became message queues and pipes.

use crate::platform::types::*;

/// The `svc` immediate used to enter the kernel on AArch64. Unchanged, as is
/// the skip-an-instruction error return; e.g. `ChannelCreate_r`:
/// `mov w8, #0x23; svc #0x51; ret; neg x0, x0; ret`.
pub const SVC_IMM: u32 = 0x51;

/// Kernel call numbers, named as in `../qnx7/version.rs`. The vendor 8.0
/// veneer is noted where its name differs.
#[allow(dead_code)]
pub mod ker {
    /// `__kernop`.
    pub const NOP: u32 = 0;
    pub const TRACE_EVENT: u32 = 1;
    pub const RING0: u32 = 2;
    pub const CACHE_CONTROL: u32 = 3;
    pub const SYS_SRANDOM: u32 = 4;
    pub const MSG_REGISTER_EVENT: u32 = 5;
    pub const MSG_SEND_PULSEPTR: u32 = 6;
    pub const SYS_CPUPAGE_GET: u32 = 7;
    pub const SYS_CPUPAGE_SET: u32 = 8;
    pub const MSG_PAUSE: u32 = 9;
    pub const MSG_CURRENT: u32 = 10;
    /// `MsgSend`, `MsgSendv`, `MsgSendsv`, `MsgSendvs`.
    pub const MSG_SENDV: u32 = 11;
    /// The `nc` (non-cancellation-point) forms of the above.
    pub const MSG_SENDVNC: u32 = 12;
    pub const MSG_ERROR: u32 = 13;
    /// `MsgReceive`, `MsgReceivev`.
    pub const MSG_RECEIVEV: u32 = 14;
    pub const MSG_REPLYV: u32 = 15;
    pub const MSG_READV: u32 = 16;
    pub const MSG_WRITEV: u32 = 17;
    pub const MSG_INFO: u32 = 19;
    pub const MSG_SEND_PULSE: u32 = 20;
    pub const MSG_DELIVER_EVENT: u32 = 21;
    /// `MsgReceivePulse`.
    pub const MSG_RECEIVEPULSEV: u32 = 24;
    pub const MSG_VERIFY_EVENT: u32 = 25;
    pub const SIGNAL_KILL: u32 = 26;
    pub const SIGNAL_RETURN: u32 = 27;
    pub const SIGNAL_ACTION: u32 = 29;
    pub const SIGNAL_PROCMASK: u32 = 30;
    pub const SIGNAL_SUSPEND: u32 = 31;
    /// `SignalWaitinfoMask(set, info, mask)`; `SignalWaitinfo` passes a null
    /// mask.
    pub const SIGNAL_WAITINFO: u32 = 32;
    pub const SIGNAL_KILL_SIGVAL: u32 = 33;
    pub const CHANNEL_CREATE: u32 = 35;
    pub const CHANNEL_DESTROY: u32 = 36;
    /// `ChannelCtl` (7.1: `ChanConAttr`).
    pub const CHANNEL_CTL: u32 = 37;
    pub const CONNECT_ATTACH: u32 = 39;
    pub const CONNECT_DETACH: u32 = 40;
    pub const CONNECT_SERVER_INFO: u32 = 41;
    /// `ConnectClientInfoExtraArgs(scoid, info, ngroups, able, nable, extra)`.
    pub const CONNECT_CLIENT_INFO: u32 = 42;
    pub const CONNECT_FLAGS: u32 = 43;
    /// New in 8.0: descriptor passing between processes.
    pub const CONNECT_DUPFD: u32 = 44;
    pub const THREAD_CREATE: u32 = 46;
    pub const THREAD_DESTROY: u32 = 47;
    pub const THREAD_DETACH: u32 = 49;
    pub const THREAD_JOIN: u32 = 50;
    pub const THREAD_CANCEL: u32 = 51;
    pub const THREAD_CTL: u32 = 52;
    pub const THREAD_CTLEXT: u32 = 53;
    pub const INTERRUPT_ATTACH: u32 = 55;
    pub const INTERRUPT_QUERY: u32 = 56;
    pub const INTERRUPT_DETACH: u32 = 57;
    pub const INTERRUPT_WAIT: u32 = 58;
    pub const INTERRUPT_MASK: u32 = 59;
    pub const INTERRUPT_UNMASK: u32 = 60;
    pub const INTERRUPT_UNBLOCK: u32 = 61;
    pub const INTERRUPT_CTL: u32 = 62;
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
    pub const TIMER_DELEGATE: u32 = 76;
    pub const SYNC_RWLOCK: u32 = 77;
    /// `SyncTypeCreate`.
    pub const SYNC_CREATE: u32 = 78;
    pub const SYNC_DESTROY: u32 = 79;
    pub const SYNC_MUTEX_LOCK: u32 = 80;
    pub const SYNC_MUTEX_UNLOCK: u32 = 81;
    pub const SYNC_CONDVAR_WAIT: u32 = 82;
    pub const SYNC_CONDVAR_SIGNAL: u32 = 83;
    pub const SYNC_SEM_POST: u32 = 84;
    pub const SYNC_SEM_WAIT: u32 = 85;
    pub const SYNC_CTL: u32 = 86;
    /// 7.1's `SyncMutexRevive` slot.
    pub const SYNC_BARRIER_WAIT: u32 = 87;
    pub const SCHED_GET: u32 = 88;
    pub const SCHED_SET: u32 = 89;
    pub const SCHED_YIELD: u32 = 90;
    pub const SCHED_INFO: u32 = 91;
    pub const SCHED_CTL: u32 = 92;
    /// 93-97 were Qnet (`NetCred` ...) on 7.1.
    pub const MSG_QUEUE_OPEN: u32 = 93;
    pub const MSG_QUEUE_SEND: u32 = 94;
    pub const MSG_QUEUE_RECEIVE: u32 = 95;
    pub const MSG_QUEUE_CLOSE: u32 = 96;
    pub const MSG_QUEUE_CTL: u32 = 97;
    pub const PIPE_OPEN: u32 = 98;
    pub const PIPE_CTL: u32 = 99;
    pub const PIPE_CLOSE: u32 = 100;
    pub const SYNC_TYPE_DESTROY: u32 = 104;
    pub const TEST: u32 = 105;
    pub const SYS_CUSTOM: u32 = 106;
    pub const BAD: u32 = 107;
}

/// Offsets into the kernel's `struct _thread_local_storage`. The block's
/// layout is 7.1's; only how it is found changed (see `tls`).
///
/// Verified against the vendor libc, which reaches the block through
/// `[tpidr_el0 + 8]`:
/// * `__get_errno_ptr`: `ldr x1, [x0, #8]; ldr x0, [x1, #16]`
/// * `getpid`: `ldr x1, [x0, #8]; ldr w0, [x1, #32]`
/// * `gettid`: `ldr x1, [x0, #8]; ldr w0, [x1, #36]`
#[allow(dead_code)]
pub mod tls {
    /// 7.1's `__exitfunc` slot; 8.0 keeps the stack size here.
    pub const STACKSIZE: usize = 0;
    pub const ARG: usize = 8;
    pub const ERRPTR: usize = 16;
    pub const ERRVAL: usize = 24;
    pub const FLAGS: usize = 28;
    pub const PID: usize = 32;
    pub const TID: usize = 36;
    pub const OWNER: usize = 40;
    pub const STACKADDR: usize = 48;
}

/// `SysCpupageGet` indices (`CPUPAGE_*`, from the DWARF enum).
#[allow(dead_code)]
pub mod cpupage {
    pub const ADDR: usize = usize::MAX;
    pub const TLS: usize = 0;
    pub const PLS: usize = 1;
    pub const SYSPAGE: usize = 2;
    pub const CPU: usize = 3;
    pub const STATE: usize = 4;
}

/// `struct sigaction` as the 8.0 kernel sees it (24 bytes, DWARF). `sigset_t`
/// is now a single `_Uint64t`, so after the 4-byte `sa_flags` the mask is
/// padded out to offset 16, where 7.1 had it at 12.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Sigaction {
    handler: usize,
    flags: c_int,
    reserved: u32,
    mask: [u32; 2],
}

impl Sigaction {
    pub fn new(handler: usize, flags: c_int, mask: [u32; 2]) -> Self {
        Self {
            handler,
            flags,
            reserved: 0,
            mask,
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

/// `struct _msg_info64` (64 bytes), which 8.0's `ConnectServerInfo`, `MsgInfo`
/// and `MsgReceive` write. A 7.1-sized buffer here would be overrun by 16
/// bytes. `nd`..`coid` keep their 7.1 offsets; the lengths grew to 64 bits and
/// moved behind `priority`/`flags`.
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
    pub priority: i16,
    pub flags: i16,
    pub msglen: i64,
    pub srcmsglen: i64,
    pub dstmsglen: i64,
    pub type_id: u32,
    pub reserved: u32,
}

/// `_IO_CONNECT_EXTRA_LINK`: 13 on 8.0, where 1 is now
/// `_IO_CONNECT_EXTRA_LINK_RESERVED`.
pub const CONNECT_EXTRA_LINK: u8 = 13;

/// `sigevent::sigev_notify` values (`SIGEV_*`). 8.0 is 64-bit only and has no
/// `SIGEV_64BIT` flag; the low byte (`SIGEV_TYPE_MASK`) is the type, so 7.1's
/// `0x81` would be an invalid type here.
#[allow(dead_code)]
pub mod sigev {
    pub const NONE: i32 = 0;
    pub const SIGNAL: i32 = 1;
    pub const THREAD: i32 = 7;
}

/// 8.0 has no `struct _proc_spawn`: `_PROC_SPAWN_START` moved to subtype 6 and
/// spawning goes through `_PROC_POSIX_SPAWN` (`struct _proc_posix_spawn`), which
/// this backend does not build yet. Spawn and exec report `ENOSYS`.
pub const HAS_PROC_SPAWN: bool = false;

/// Whether `ThreadCtl(_NTO_TCTL_SIGSTACK)` sets the alternate signal stack:
/// the vendor `sigaltstack` is exactly that call.
pub const HAS_TCTL_SIGSTACK: bool = true;

/// Socket numbering the two network stacks disagree on, and the switches for
/// the socket message protocol (see `socket.rs`).
///
/// io-sock follows FreeBSD. The protocol switches are read off the vendor
/// `libsocket.so.4`, which has no header describing it.
pub mod sock {
    use crate::platform::types::c_int;

    pub const AF_INET6: c_int = 28;
    pub const SO_SNDTIMEO: c_int = 0x1005;
    pub const SO_RCVTIMEO: c_int = 0x1006;
    pub const SO_PROTOCOL: c_int = 0x1016;
    pub const SO_DOMAIN: c_int = 0x1019;
    pub const MSG_NOSIGNAL: c_int = 0x0002_0000;
    pub const MSG_CMSG_CLOEXEC: c_int = 0x0004_0000;
    pub const TCP_KEEPIDLE: c_int = 0x100;
    pub const TCP_KEEPINTVL: c_int = 0x200;
    pub const TCP_KEEPCNT: c_int = 0x400;
    pub const SOCK_NONBLOCK: c_int = 0x2000_0000;

    /// `socket()` opens `/dev/socket/<domain>` and passes the type and
    /// protocol as an `_IO_CONNECT_EXTRA_SOCKET` payload.
    pub const OPEN_EXTRA: bool = true;
    /// `sendto`/`recvfrom` carry 32-bit flags and address lengths, and a
    /// set's value counts in the sockopt `combine_len`.
    pub const WIDE_HEADERS: bool = true;
    /// The sockopt `_IO_MSG` subtypes.
    pub const SETOPT: u16 = 0;
    pub const GETOPT: u16 = 1;
}

/// 8.0 pipes are kernel pipes (`sys/kpipe.h`) behind the path manager; the
/// vendor `pipe()` resolves `/` with `_FTYPE_PIPE`. There is no `/dev/pipe`.
pub const PIPE_PATH: &core::ffi::CStr = c"/";
