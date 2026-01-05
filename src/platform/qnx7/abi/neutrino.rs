//! Core Neutrino kernel types and constants.

use crate::platform::types::*;

/// Scatter/gather element, identical in layout to `struct iovec`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoVec {
    pub base: *mut c_void,
    pub len: usize,
}

impl IoVec {
    #[inline]
    pub fn new(base: *const u8, len: usize) -> Self {
        Self {
            base: base.cast_mut().cast::<c_void>(),
            len,
        }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            base: core::ptr::null_mut(),
            len: 0,
        }
    }
}

/// A two-part scatter/gather list, laid out so that a pointer to it is a valid
/// `IoVec[2]` for `MsgSendv`.
///
/// Writes use this to send the message header and the caller's payload without
/// copying the payload into a bounce buffer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoVecPair {
    pub header: IoVec,
    pub payload: IoVec,
}

/// Marks a connection id as belonging to the kernel's side channel rather than
/// the process's file descriptor space.
pub const NTO_SIDE_CHANNEL: c_int = 0x4000_0000;

/// The process manager's process id. procnto is always process 1.
pub const SYSMGR_PID: pid_t = 1;

/// The process manager's channel id.
pub const SYSMGR_CHID: c_int = 1;

/// The pre-established side-channel connection to procnto.
///
/// Verified in the vendor libc: `setuid` issues `MsgSendnc` with `w0` loaded
/// from `mov w0, #0x40000000`, i.e. side-channel connection 0, and no
/// `ConnectAttach` anywhere in the call path — the connection exists from
/// process creation.
pub const SYSMGR_COID: c_int = NTO_SIDE_CHANNEL;

/// The path manager's connection id.
///
/// procnto is both the process manager and the path manager, and serves both on
/// the same channel.
pub const PATHMGR_COID: c_int = SYSMGR_COID;

/// procnto's handle for its own namespace root.
pub const SYSMGR_HANDLE: u32 = 0;

/// `ConnectAttach` flags (`_NTO_COF_*`).
#[allow(dead_code)]
pub mod cof {
    use crate::platform::types::c_uint;

    /// Close the connection on `exec()`.
    pub const CLOEXEC: c_uint = 0x0001;
    pub const DEAD: c_uint = 0x0002;
    pub const NOSHARE: c_uint = 0x0040;
}

/// `ChannelCreate` flags (`_NTO_CHF_*`).
#[allow(dead_code)]
pub mod chf {
    use crate::platform::types::c_uint;

    pub const FIXED_PRIORITY: c_uint = 0x0001;
    pub const UNBLOCK: c_uint = 0x0002;
    pub const THREAD_DEATH: c_uint = 0x0004;
    pub const DISCONNECT: c_uint = 0x0008;
    pub const NET_MSG: c_uint = 0x0010;
    pub const SENDER_LEN: c_uint = 0x0020;
    pub const COID_DISCONNECT: c_uint = 0x0040;
    pub const REPLY_LEN: c_uint = 0x0080;
    pub const PULSE_POOL: c_uint = 0x0100;
    pub const GLOBAL: c_uint = 0x0800;
    pub const PRIVATE: c_uint = 0x1000;
    pub const MSG_PAUSING: c_uint = 0x2000;
    pub const INHERIT_RUNMASK: c_uint = 0x4000;
    pub const UNBLOCK_TIMER: c_uint = 0x8000;
}

/// Clock identifiers as understood by `ClockTime` and `ClockPeriod`.
///
/// These are the kernel's own identifiers and are unrelated to the
/// `clockid_t` values relibc exposes in `time.h`.
#[allow(dead_code)]
pub mod clock {
    use crate::platform::types::c_int;

    pub const REALTIME: c_int = 0;
    pub const SOFTTIME: c_int = 1;
    pub const MONOTONIC: c_int = 2;
    pub const PROCESS_CPUTIME_ID: c_int = 3;
    pub const THREAD_CPUTIME_ID: c_int = 4;
}

/// Thread blocking states. A thread's state number doubles as the bit position
/// of the corresponding `TimerTimeout` flag.
#[allow(dead_code)]
pub mod state {
    pub const DEAD: u32 = 0;
    pub const RUNNING: u32 = 1;
    pub const READY: u32 = 2;
    pub const STOPPED: u32 = 3;
    pub const SEND: u32 = 4;
    pub const RECEIVE: u32 = 5;
    pub const REPLY: u32 = 6;
    pub const WAITPAGE: u32 = 9;
    pub const SIGSUSPEND: u32 = 10;
    pub const SIGWAITINFO: u32 = 11;
    pub const NANOSLEEP: u32 = 12;
    pub const MUTEX: u32 = 13;
    pub const CONDVAR: u32 = 14;
    pub const JOIN: u32 = 15;
    pub const INTR: u32 = 16;
    pub const SEM: u32 = 17;
    pub const WAITCTX: u32 = 18;
}

/// `_NTO_TIMEOUT_*` flags for `TimerTimeout`, each `1 << state`.
///
/// `NANOSLEEP` is verified: the vendor `nanosleep` passes `0x1000`, which is
/// `1 << STATE_NANOSLEEP`.
#[allow(dead_code)]
pub mod timeout {
    use super::state;
    use crate::platform::types::c_uint;

    pub const SEND: c_uint = 1 << state::SEND;
    pub const RECEIVE: c_uint = 1 << state::RECEIVE;
    pub const REPLY: c_uint = 1 << state::REPLY;
    pub const SIGSUSPEND: c_uint = 1 << state::SIGSUSPEND;
    pub const SIGWAITINFO: c_uint = 1 << state::SIGWAITINFO;
    pub const NANOSLEEP: c_uint = 1 << state::NANOSLEEP;
    pub const MUTEX: c_uint = 1 << state::MUTEX;
    pub const CONDVAR: c_uint = 1 << state::CONDVAR;
    pub const JOIN: c_uint = 1 << state::JOIN;
    pub const INTR: c_uint = 1 << state::INTR;
    pub const SEM: c_uint = 1 << state::SEM;
}

/// Kernel synchronisation object, the storage behind a QNX mutex, condvar,
/// semaphore or spinlock.
///
/// Neutrino keeps the contention state in user memory and only enters the
/// kernel when a thread has to block, which is why this type is laid out in
/// user space rather than being an opaque handle.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sync {
    /// Recursion count and flags for a mutex; type-specific payload otherwise.
    pub count: c_uint,
    /// Owning thread for a mutex, or a [`sync_owner`] sentinel.
    pub owner: c_uint,
}

impl Sync {
    /// An unlocked, uncontended mutex.
    ///
    /// Neutrino's own `_mutex_init` writes exactly this — count zero and
    /// `_NTO_SYNC_MUTEX_FREE` — specifically so that the first lock does not
    /// have to enter the kernel. No `SyncTypeCreate` call is needed, which is
    /// what makes a statically initialised mutex possible.
    pub const fn mutex() -> Self {
        Self {
            count: 0,
            owner: sync_owner::MUTEX_FREE,
        }
    }

    /// A condition variable bound to the given kernel clock.
    ///
    /// A condvar is identified by its owner sentinel with the clock id in the
    /// count field, so this too needs no kernel call to initialise.
    pub const fn condvar(clock_id: c_int) -> Self {
        Self {
            count: clock_id as c_uint,
            owner: sync_owner::COND,
        }
    }
}

/// `sync_t::__count` flag bits (`_NTO_SYNC_*`), which share the field with a
/// 24-bit recursion count.
#[allow(dead_code)]
pub mod sync_count {
    use crate::platform::types::c_uint;

    pub const NONRECURSIVE: c_uint = 0x8000_0000;
    pub const PRIOCEILING: c_uint = 0x2000_0000;
    pub const PRIONONE: c_uint = 0x1000_0000;
    pub const WAKEUP: c_uint = 0x0800_0000;
    pub const KFORCE: c_uint = 0x0400_0000;
    pub const COUNTMASK: c_uint = 0x00ff_ffff;
}

/// `sync_t::__owner` values (`_NTO_SYNC_*`).
///
/// A mutex stores its owning thread here, optionally with [`WAITING`] set. The
/// other primitives are identified by a sentinel owner, with `__count` carrying
/// the type-specific payload: a clock id for a condition variable, the current
/// value for a semaphore.
///
/// [`WAITING`]: sync_owner::WAITING
#[allow(dead_code)]
pub mod sync_owner {
    use crate::platform::types::c_uint;

    pub const MUTEX_FREE: c_uint = 0x0000_0000;
    /// Set on a mutex's owner field while a thread is blocked on it, which is
    /// what tells the unlocking thread it must enter the kernel.
    pub const WAITING: c_uint = 0x8000_0000;
    pub const OWNER_MASK: c_uint = 0x7fff_ffff;
    pub const INITIALIZER: c_uint = 0xffff_ffff;
    pub const DESTROYED: c_uint = 0xffff_fffe;
    pub const NAMED_SEM: c_uint = 0xffff_fffd;
    pub const SEM: c_uint = 0xffff_fffc;
    pub const COND: c_uint = 0xffff_fffb;
    pub const SPIN: c_uint = 0xffff_fffa;
    pub const JOB: c_uint = 0xffff_fff9;
    pub const BARRIER: c_uint = 0xffff_fff8;
}

/// `ThreadCtl` commands (`_NTO_TCTL_*`).
#[allow(dead_code)]
pub mod tctl {
    use crate::platform::types::c_int;

    pub const IO_PRIV: c_int = 1;
    pub const THREADS_HOLD: c_int = 2;
    pub const THREADS_CONT: c_int = 3;
    pub const RUNMASK: c_int = 4;
    pub const ALIGN_FAULT: c_int = 5;
    pub const RUNMASK_GET_AND_SET: c_int = 6;
    pub const ONE_THREAD_HOLD: c_int = 8;
    pub const ONE_THREAD_CONT: c_int = 9;
    pub const RUNMASK_GET_AND_SET_INHERIT: c_int = 10;
    pub const NAME: c_int = 11;
    pub const IO: c_int = 14;
    pub const ADD_EXIT_EVENT: c_int = 17;
    pub const DEL_EXIT_EVENT: c_int = 18;
    pub const IO_LEVEL: c_int = 19;
    /// The alternate signal stack; see `version::HAS_TCTL_SIGSTACK`.
    pub const SIGSTACK: c_int = 23;
}

/// Thread creation attributes, `struct _thread_attr` (104 bytes).
///
/// Shared by 7.1 and 8.0. 8.0 renames `__exitfunc` to `__reserved0` and
/// `__spare` to `__reserved1`; both must be zero there, which is what this
/// backend always passes. `param` sits at offset 40 because `sched_param`
/// contains `struct timespec` and is 8-byte aligned.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThreadAttr {
    pub flags: c_int,
    pub stacksize: usize,
    pub stackaddr: *mut c_void,
    pub exitfunc: Option<unsafe extern "C" fn(*mut c_void)>,
    pub policy: c_int,
    pub param: SchedParam,
    pub guardsize: u32,
    pub prealloc: u32,
    pub reserved: [u32; 2],
}

/// Scheduling parameters, `struct sched_param` as the kernel sees it
/// (48 bytes). The union after the two priorities holds `struct timespec`s,
/// hence the 8-byte alignment.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
pub struct SchedParam {
    pub sched_priority: c_int,
    pub sched_curpriority: c_int,
    pub reserved: [c_int; 10],
}

/// QNX's `struct sigaction` and its conversion to and from relibc's.
pub mod sigaction {
    use crate::{header::signal::sigaction as re_sigaction, platform::types::c_int};

    /// `struct sigaction` as the kernel sees it. The mask moved from offset 12
    /// to 16 in 8.0, so the layout comes from the version module, which also
    /// provides the constructor and accessors used below.
    pub use crate::platform::sys::version::Sigaction;

    /// QNX `SA_*` flags, from `<signal.h>`.
    ///
    /// QNX has no `SA_RESTART`: an interrupted kernel call is not restarted.
    /// The flag is accepted and dropped (see [`from_relibc`]).
    #[allow(dead_code)]
    pub mod sa {
        use crate::platform::types::c_int;

        pub const NOCLDSTOP: c_int = 0x0001;
        pub const SIGINFO: c_int = 0x0002;
        pub const RESETHAND: c_int = 0x0004;
        pub const ONSTACK: c_int = 0x0008;
        pub const NODEFER: c_int = 0x0010;
        pub const NOCLDWAIT: c_int = 0x0020;
        pub const MASK: c_int = 0x00ff;
    }

    /// Converts relibc's `sigaction` into the kernel's.
    pub fn from_relibc(act: &re_sigaction) -> Sigaction {
        use crate::header::signal as re;

        let mut flags = 0;
        // SA_RESTART has no QNX counterpart and is dropped.
        let pairs = [
            (re::SA_NOCLDSTOP, sa::NOCLDSTOP),
            (re::SA_NOCLDWAIT, sa::NOCLDWAIT),
            (re::SA_SIGINFO, sa::SIGINFO),
            (re::SA_ONSTACK, sa::ONSTACK),
            (re::SA_NODEFER, sa::NODEFER),
            (re::SA_RESETHAND, sa::RESETHAND),
        ];
        for (relibc_bit, qnx_bit) in pairs {
            if act.sa_flags & (relibc_bit as c_int) != 0 {
                flags |= qnx_bit;
            }
        }

        Sigaction::new(
            act.sa_handler.map_or(0, |handler| handler as usize),
            flags,
            super::super::signal::set_to_kernel(act.sa_mask),
        )
    }

    /// Converts the kernel's `sigaction` back into relibc's.
    pub fn to_relibc(act: &Sigaction) -> re_sigaction {
        use crate::header::signal as re;

        let mut sa_flags: c_int = 0;
        let pairs = [
            (sa::NOCLDSTOP, re::SA_NOCLDSTOP),
            (sa::NOCLDWAIT, re::SA_NOCLDWAIT),
            (sa::SIGINFO, re::SA_SIGINFO),
            (sa::ONSTACK, re::SA_ONSTACK),
            (sa::NODEFER, re::SA_NODEFER),
            (sa::RESETHAND, re::SA_RESETHAND),
        ];
        for (qnx_bit, relibc_bit) in pairs {
            if act.flags() & qnx_bit != 0 {
                sa_flags |= relibc_bit as c_int;
            }
        }

        let mut out = re_sigaction {
            sa_handler: None,
            sa_flags,
            sa_restorer: None,
            sa_mask: super::super::signal::set_from_kernel(act.mask()),
        };
        let handler = act.handler();
        if handler != 0 {
            // SAFETY: the kernel returns the pointer this process installed.
            out.sa_handler = Some(unsafe { core::mem::transmute::<usize, extern "C" fn(c_int)>(handler) });
        }
        out
    }
}

/// A message's origin, filled in by `MsgReceive`, `MsgInfo` and
/// `ConnectServerInfo`. 48 bytes on 7.1 and 64 on 8.0 (`_msg_info64`), so the
/// kernel-facing layout comes from the version module.
pub use crate::platform::sys::version::MsgInfo;

/// `struct _itimer`: `{ nsec, interval_nsec }`, expiration first (DWARF).
///
/// The vendor `timer_settime` loads `it_value` into the first word and
/// `it_interval` into the second. QNX's `itimerspec` puts `it_value` first,
/// where relibc's (like Linux's) puts `it_interval` first, which is how the two
/// words were once read the wrong way round here: every timer reached the
/// kernel with value and interval swapped, and a one-shot timer (interval 0)
/// was disarmed. Callers set the fields by name, so only this order matters.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Itimer {
    pub expire_nsec: u64,
    pub interval_nsec: u64,
}

/// `struct sigevent` as a 64-bit process sees it (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Sigevent {
    pub notify: i32,
    pub pad: i32,
    pub un1: u64,
    pub value: u64,
    pub un2: u64,
}

/// `struct _timer_info` without relying on the trailing event union's size.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TimerInfo {
    pub itime: Itimer,
    pub otime: Itimer,
    pub flags: u32,
    pub tid: i32,
    pub notify: i32,
    pub clockid: i32,
    pub overruns: u32,
    pub event: Sigevent,
}

/// `timer_settime` flags (`<time.h>`); relibc's `TIMER_ABSTIME` is Linux's 1.
pub mod timer_flags {
    pub const ABSTIME: i32 = 0x8000_0000_u32 as i32;
}

/// `_NTO_TIMER_*` flags for `TimerInfo`.
#[allow(dead_code)]
pub mod timer_info {
    pub const SEARCH: i32 = 0x0000_0001;
    pub const RESET_OVERRUNS: i32 = 0x0000_0002;
}

/// Fixed-size `_client_info` used when the group list is not required.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ClientInfo {
    pub nd: u32,
    pub pid: pid_t,
    pub sid: pid_t,
    pub flags: u32,
    pub ruid: uid_t,
    pub euid: uid_t,
    pub suid: uid_t,
    pub rgid: gid_t,
    pub egid: gid_t,
    pub sgid: gid_t,
    pub ngroups: u32,
}

/// How many supplementary groups `ConnectClientInfo` will fill at most.
pub const NGROUPS_MAX: usize = 384;
