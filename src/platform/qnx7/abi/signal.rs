//! QNX Neutrino signal numbers and their translation from relibc's numbering.
//!
//! Neutrino follows the BSD signal numbering, which disagrees with the Linux
//! numbering relibc uses from `SIGUSR1` upwards — `SIGUSR1` is 16 on QNX and 10
//! on Linux, `SIGCHLD` is 18 rather than 17, and so on. Sending a signal
//! without translating would deliver the wrong one.
//!
//! The numbering was recovered from the vendor `libc.so`: `sys_siglist` is an
//! array of pointers whose `R_AARCH64_RELATIVE` addends point at the per-signal
//! description strings, so resolving the relocations gives the table by index.
//! Slot 7 is QNX's `SIGDEADLK` ("mutex deadlock"), which has no Linux
//! counterpart.
//!
//! What the kernel hands back is numbered and laid out the QNX way as well:
//! the `siginfo_t` of a delivered signal, of `sigtimedwait` and of
//! `_PROC_WAIT`, and the user `si_code` values inside it.

use core::ptr;

use crate::{header::signal as re, platform::types::c_int};

/// Signal numbers as the Neutrino kernel understands them.
#[allow(dead_code)]
pub mod qnx {
    use crate::platform::types::c_int;

    pub const SIGHUP: c_int = 1;
    pub const SIGINT: c_int = 2;
    pub const SIGQUIT: c_int = 3;
    pub const SIGILL: c_int = 4;
    pub const SIGTRAP: c_int = 5;
    pub const SIGABRT: c_int = 6;
    /// QNX-specific: a mutex was left in a deadlocked state.
    pub const SIGDEADLK: c_int = 7;
    pub const SIGFPE: c_int = 8;
    pub const SIGKILL: c_int = 9;
    pub const SIGBUS: c_int = 10;
    pub const SIGSEGV: c_int = 11;
    pub const SIGSYS: c_int = 12;
    pub const SIGPIPE: c_int = 13;
    pub const SIGALRM: c_int = 14;
    pub const SIGTERM: c_int = 15;
    pub const SIGUSR1: c_int = 16;
    pub const SIGUSR2: c_int = 17;
    pub const SIGCHLD: c_int = 18;
    pub const SIGPWR: c_int = 19;
    pub const SIGWINCH: c_int = 20;
    pub const SIGURG: c_int = 21;
    pub const SIGIO: c_int = 22;
    pub const SIGSTOP: c_int = 23;
    pub const SIGTSTP: c_int = 24;
    pub const SIGCONT: c_int = 25;
    pub const SIGTTIN: c_int = 26;
    pub const SIGTTOU: c_int = 27;
    pub const SIGVTALRM: c_int = 28;
    pub const SIGPROF: c_int = 29;
    pub const SIGXCPU: c_int = 30;
    pub const SIGXFSZ: c_int = 31;
    /// First real-time signal.
    pub const SIGRTMIN: c_int = 41;
    /// Last real-time signal.
    pub const SIGRTMAX: c_int = 56;
}

/// Translates a relibc signal number into Neutrino's numbering.
///
/// Signals relibc defines but QNX does not (`SIGSTKFLT`) have no sensible
/// mapping and are rejected by the caller as invalid.
pub fn to_kernel(signo: c_int) -> Option<c_int> {
    // relibc spells its signal numbers as `usize`.
    let signo = usize::try_from(signo).ok()?;
    Some(match signo {
        re::SIGHUP => qnx::SIGHUP,
        re::SIGINT => qnx::SIGINT,
        re::SIGQUIT => qnx::SIGQUIT,
        re::SIGILL => qnx::SIGILL,
        re::SIGTRAP => qnx::SIGTRAP,
        re::SIGABRT => qnx::SIGABRT,
        re::SIGBUS => qnx::SIGBUS,
        re::SIGFPE => qnx::SIGFPE,
        re::SIGKILL => qnx::SIGKILL,
        re::SIGUSR1 => qnx::SIGUSR1,
        re::SIGSEGV => qnx::SIGSEGV,
        re::SIGUSR2 => qnx::SIGUSR2,
        re::SIGPIPE => qnx::SIGPIPE,
        re::SIGALRM => qnx::SIGALRM,
        re::SIGTERM => qnx::SIGTERM,
        re::SIGCHLD => qnx::SIGCHLD,
        re::SIGCONT => qnx::SIGCONT,
        re::SIGSTOP => qnx::SIGSTOP,
        re::SIGTSTP => qnx::SIGTSTP,
        re::SIGTTIN => qnx::SIGTTIN,
        re::SIGTTOU => qnx::SIGTTOU,
        re::SIGURG => qnx::SIGURG,
        re::SIGXCPU => qnx::SIGXCPU,
        re::SIGXFSZ => qnx::SIGXFSZ,
        re::SIGVTALRM => qnx::SIGVTALRM,
        re::SIGPROF => qnx::SIGPROF,
        re::SIGWINCH => qnx::SIGWINCH,
        re::SIGIO => qnx::SIGIO,
        re::SIGPWR => qnx::SIGPWR,
        re::SIGSYS => qnx::SIGSYS,
        // Real-time signals keep their offset from the lowest one.
        signo if (re::SIGRTMIN..=re::SIGRTMAX).contains(&signo) => {
            let offset = c_int::try_from(signo - re::SIGRTMIN).ok()?;
            let rt = qnx::SIGRTMIN + offset;
            if rt > qnx::SIGRTMAX {
                return None;
            }
            rt
        }
        // Zero is the "does this process exist" probe and passes through.
        0 => 0,
        _ => return None,
    })
}

/// Translates a Neutrino signal number back into relibc's numbering, for
/// everything the kernel hands back: the signal a handler runs for,
/// `sigtimedwait`'s result, and the signal inside a wait status.
///
/// `SIGDEADLK` has no relibc number and the kernel only raises it for a mutex
/// deadlock that relibc's own mutexes cannot produce; it and anything else
/// unknown map to `None`.
pub fn from_kernel(signo: c_int) -> Option<c_int> {
    let relibc = match signo {
        0 => 0,
        qnx::SIGHUP => re::SIGHUP,
        qnx::SIGINT => re::SIGINT,
        qnx::SIGQUIT => re::SIGQUIT,
        qnx::SIGILL => re::SIGILL,
        qnx::SIGTRAP => re::SIGTRAP,
        qnx::SIGABRT => re::SIGABRT,
        qnx::SIGFPE => re::SIGFPE,
        qnx::SIGKILL => re::SIGKILL,
        qnx::SIGBUS => re::SIGBUS,
        qnx::SIGSEGV => re::SIGSEGV,
        qnx::SIGSYS => re::SIGSYS,
        qnx::SIGPIPE => re::SIGPIPE,
        qnx::SIGALRM => re::SIGALRM,
        qnx::SIGTERM => re::SIGTERM,
        qnx::SIGUSR1 => re::SIGUSR1,
        qnx::SIGUSR2 => re::SIGUSR2,
        qnx::SIGCHLD => re::SIGCHLD,
        qnx::SIGPWR => re::SIGPWR,
        qnx::SIGWINCH => re::SIGWINCH,
        qnx::SIGURG => re::SIGURG,
        qnx::SIGIO => re::SIGIO,
        qnx::SIGSTOP => re::SIGSTOP,
        qnx::SIGTSTP => re::SIGTSTP,
        qnx::SIGCONT => re::SIGCONT,
        qnx::SIGTTIN => re::SIGTTIN,
        qnx::SIGTTOU => re::SIGTTOU,
        qnx::SIGVTALRM => re::SIGVTALRM,
        qnx::SIGPROF => re::SIGPROF,
        qnx::SIGXCPU => re::SIGXCPU,
        qnx::SIGXFSZ => re::SIGXFSZ,
        rt if (qnx::SIGRTMIN..=qnx::SIGRTMAX).contains(&rt) => {
            let relibc = re::SIGRTMIN + usize::try_from(rt - qnx::SIGRTMIN).ok()?;
            if relibc > re::SIGRTMAX {
                return None;
            }
            relibc
        }
        _ => return None,
    };
    c_int::try_from(relibc).ok()
}

/// A relibc `sigset_t` (bit `n - 1` is relibc signal `n`) as the two mask
/// words the kernel expects (bit `n - 1` is QNX signal `n`).
///
/// Signal sets are numbered too: passing relibc's through unchanged would,
/// for instance, block QNX signal 10 (`SIGBUS`) when asked to block relibc's
/// 10 (`SIGUSR1`). Signals QNX does not have are dropped.
pub fn set_to_kernel(set: u64) -> [u32; 2] {
    let mut out = 0u64;
    for relibc in 1..=64usize {
        if set & (1u64 << (relibc - 1)) == 0 {
            continue;
        }
        if let Some(qnx) = c_int::try_from(relibc).ok().and_then(to_kernel) {
            if (1..=64).contains(&qnx) {
                out |= 1u64 << (qnx - 1);
            }
        }
    }
    [out as u32, (out >> 32) as u32]
}

/// The inverse of [`set_to_kernel`].
pub fn set_from_kernel(words: [u32; 2]) -> u64 {
    let set = u64::from(words[0]) | (u64::from(words[1]) << 32);
    let mut out = 0u64;
    for qnx in 1..=64 {
        if set & (1u64 << (qnx - 1)) == 0 {
            continue;
        }
        if let Some(relibc) = from_kernel(qnx) {
            if (1..=64).contains(&relibc) {
                out |= 1u64 << (relibc - 1);
            }
        }
    }
    out
}

/// `SignalProcmask` how-values (`<signal.h>`), including the kernel's own
/// query for the pending set.
pub mod how {
    use crate::platform::types::c_int;

    pub const BLOCK: c_int = 0;
    pub const UNBLOCK: c_int = 1;
    pub const SETMASK: c_int = 2;
    pub const PENDING: c_int = 5;
}

/// `si_code` values of signals sent by a process or raised on its behalf.
/// relibc numbers these differently; per-signal codes (`SEGV_MAPERR`,
/// `CLD_EXITED`, ...) are numbered alike and pass through.
pub mod si_code {
    use crate::platform::types::c_int;

    pub const USER: c_int = 0;
    pub const QUEUE: c_int = -2;
    pub const TIMER: c_int = -3;
    pub const ASYNCIO: c_int = -4;
    pub const MESGQ: c_int = -5;
}

/// `si_code` values of `SIGCHLD`.
pub mod cld {
    use crate::platform::types::c_int;

    pub const EXITED: c_int = 1;
    pub const KILLED: c_int = 2;
    pub const DUMPED: c_int = 3;
    pub const TRAPPED: c_int = 4;
    pub const STOPPED: c_int = 5;
    pub const CONTINUED: c_int = 6;
}

/// Offsets of the `siginfo_t` union members that `<sys/siginfo.h>` names
/// through its `si_*` macros.
pub mod si {
    pub const PID: usize = 16;
    pub const UID: usize = 24;
    pub const STATUS: usize = 28;
    pub const VALUE: usize = 32;
    pub const ADDR: usize = 32;
}

/// `siginfo_t` as the kernel and procnto fill it in: three words, then a
/// union read through the [`si`] offsets.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
pub struct Siginfo {
    pub si_signo: c_int,
    pub si_code: c_int,
    pub si_errno: c_int,
    pub data: [u32; 9],
}

impl Siginfo {
    const DATA: usize = 12;

    fn word(&self, offset: usize) -> u32 {
        self.data[(offset - Self::DATA) / 4]
    }

    fn dword(&self, offset: usize) -> u64 {
        u64::from(self.word(offset)) | (u64::from(self.word(offset + 4)) << 32)
    }

    pub fn pid(&self) -> c_int {
        self.word(si::PID) as c_int
    }

    pub fn uid(&self) -> c_int {
        self.word(si::UID) as c_int
    }

    pub fn status(&self) -> c_int {
        self.word(si::STATUS) as c_int
    }

    pub fn value(&self) -> u64 {
        self.dword(si::VALUE)
    }

    pub fn addr(&self) -> u64 {
        self.dword(si::ADDR)
    }
}

/// `ss_flags` values (`<signal.h>`).
pub mod ss {
    use crate::platform::types::c_int;

    pub const ONSTACK: c_int = 1;
    pub const DISABLE: c_int = 2;
}

/// `stack_t`, which relibc lays out in Linux's member order instead.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Stack {
    pub ss_sp: usize,
    pub ss_size: usize,
    pub ss_flags: c_int,
    /// Explicit, so no stack bytes travel to the kernel.
    pub pad: u32,
}

/// Translates a QNX `si_code` into relibc's numbering.
pub fn code_from_kernel(code: c_int) -> c_int {
    match code {
        si_code::USER => re::SI_USER,
        si_code::QUEUE => re::SI_QUEUE,
        si_code::TIMER => re::SI_TIMER,
        si_code::ASYNCIO => re::SI_ASYNCIO,
        si_code::MESGQ => re::SI_MESGQ,
        other => other,
    }
}

/// `si_status` of a `SIGCHLD`: an exit code, or a signal to renumber.
fn child_status(code: c_int, status: c_int) -> c_int {
    if code == cld::EXITED {
        status
    } else {
        from_kernel(status).unwrap_or(status)
    }
}

/// Translates a QNX `siginfo_t` into relibc's, keeping only the members that
/// are meaningful for the signal: the union overlays the sender's pid with a
/// fault number and its value with a fault address.
pub fn info_from_kernel(info: &Siginfo) -> re::siginfo_t {
    let sent = info.si_code <= 0;
    let child = info.si_signo == qnx::SIGCHLD && info.si_code > 0;
    let fault = info.si_code > 0
        && matches!(
            info.si_signo,
            qnx::SIGILL | qnx::SIGTRAP | qnx::SIGFPE | qnx::SIGBUS | qnx::SIGSEGV
        );
    re::siginfo_t {
        si_signo: from_kernel(info.si_signo).unwrap_or(info.si_signo),
        si_errno: info.si_errno,
        si_code: code_from_kernel(info.si_code),
        si_pid: if sent || child { info.pid() } else { 0 },
        si_uid: if sent { info.uid() } else { 0 },
        si_addr: if fault {
            ptr::with_exposed_provenance_mut(info.addr() as usize)
        } else {
            ptr::null_mut()
        },
        si_status: if child {
            child_status(info.si_code, info.status())
        } else {
            0
        },
        si_value: re::sigval {
            sival_ptr: if sent {
                ptr::with_exposed_provenance_mut(info.value() as usize)
            } else {
                ptr::null_mut()
            },
        },
    }
}

/// The wait status a `SIGCHLD` `siginfo_t` describes, in the encoding
/// relibc's `W*` macros decode.
pub fn wait_status(info: &Siginfo) -> c_int {
    let status = info.status();
    let signal = || from_kernel(status).unwrap_or(status) & 0x7f;
    match info.si_code {
        cld::EXITED => (status & 0xff) << 8,
        cld::KILLED => signal(),
        cld::DUMPED => signal() | 0x80,
        cld::TRAPPED | cld::STOPPED => (signal() << 8) | 0x7f,
        cld::CONTINUED => 0xffff,
        _ => 0,
    }
}
