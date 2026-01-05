//! Signals.
//!
//! Delivery is a kernel call, and so is installing a disposition — but the
//! latter needs somewhere for the kernel to *enter* the process. QNX passes
//! that address to `SignalAction` as a "stub", and the vendor `sigaction` is
//! nothing more than
//!
//! ```text
//! sigaction:
//!     ldr x1, [x5, #3592]      ; __signalstub
//!     mov w0, #0               ; pid 0 = this process
//!     b   SignalAction         ; (pid, stub, signo, act, oact)
//! ```
//!
//! The vendor stub is equally small:
//!
//! ```text
//! __signalstub:
//!     mov x19, x0              ; x0 = the kernel's signal frame
//!     ldr x3, [x19, #48]       ; handler
//!     ldr x2, [x19, #56]       ; context
//!     add x1, x19, #0          ; siginfo is the frame itself
//!     ldr x0, [x19]            ; signo, the frame's first word
//!     blr x3
//!     mov x0, x19
//!     b   SignalReturn
//! ```
//!
//! `__relibc_qnx_signalstub` keeps that shape but hands the handler relibc's
//! signal number and a relibc `siginfo_t`, built by [`translate_frame`] in a
//! slot on the signal stack: the frame holds QNX's numbering and layout.
//!
//! Calling the handler with three arguments is correct whether or not it was
//! registered with `SA_SIGINFO`: a one-argument handler simply ignores `x1`
//! and `x2`, which the AArch64 calling convention permits. The call is made
//! from assembly because Rust has no such permission.

use core::{arch::global_asm, mem::size_of};

use super::{
    Sys,
    abi::{
        neutrino::{Itimer, clock, sigaction as qnx_sa, tctl, timeout},
        signal::{
            Siginfo, Stack, from_kernel, how, info_from_kernel, set_from_kernel, set_to_kernel,
            si_code, ss, to_kernel,
        },
    },
    kernel::{
        signal_action, signal_kill, signal_procmask, signal_suspend, signal_waitinfo,
        thread_ctl, timer_timeout,
    },
    proc, tls, version,
};
#[expect(deprecated)]
use crate::header::sys_time::{ITIMER_REAL, itimerval};
use crate::{
    error::{Errno, Result},
    header::{
        bits_sigset_t::sigset_t,
        errno::{EAGAIN, EINTR, EINVAL, ENOSYS, ETIMEDOUT},
        signal::{
            SIG_BLOCK, SIG_DFL, SIG_IGN, SIG_SETMASK, SIG_UNBLOCK, SS_DISABLE, SS_ONSTACK,
            sigaction, siginfo_t, sigval, stack_t,
        },
        sys_select::timeval,
        time::timespec,
    },
    platform::{PalSignal, types::*},
};

/// The local node, as opposed to a networked one.
const ND_LOCAL_NODE: u32 = 0;

/// Stack reserved by the stub for the translated `siginfo_t`, kept 16-byte
/// aligned as AAPCS64 requires of `sp`.
const INFO_SLOT: usize = size_of::<siginfo_t>().next_multiple_of(16);

fn timeval_to_nsec(tv: &timeval) -> Result<u64> {
    if tv.tv_sec < 0 || tv.tv_usec < 0 {
        return Err(Errno(EINVAL));
    }
    (tv.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|s| s.checked_add((tv.tv_usec as u64).saturating_mul(1_000)))
        .ok_or(Errno(EINVAL))
}

fn nsec_to_timeval(nsec: u64) -> timeval {
    timeval {
        tv_sec: (nsec / 1_000_000_000) as time_t,
        tv_usec: ((nsec % 1_000_000_000) / 1_000) as suseconds_t,
    }
}

#[expect(deprecated)]
fn itimerval_to_kernel(v: &itimerval) -> Result<Itimer> {
    Ok(Itimer {
        interval_nsec: timeval_to_nsec(&v.it_interval)?,
        expire_nsec: timeval_to_nsec(&v.it_value)?,
    })
}

#[expect(deprecated)]
fn kernel_to_itimerval(v: &Itimer) -> itimerval {
    itimerval {
        it_interval: nsec_to_timeval(v.interval_nsec),
        it_value: nsec_to_timeval(v.expire_nsec),
    }
}

fn timespec_to_nsec(ts: &timespec) -> Result<u64> {
    if ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) {
        return Err(Errno(EINVAL));
    }
    (ts.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|s| s.checked_add(ts.tv_nsec as u64))
        .ok_or(Errno(EINVAL))
}

/// Builds relibc's view of the signal the kernel's frame describes.
///
/// # Safety
///
/// `frame` must be the kernel's signal frame, whose first bytes are the QNX
/// `siginfo_t`, and `out` must be valid for writing a `siginfo_t`.
unsafe extern "C" fn translate_frame(frame: *const Siginfo, out: *mut siginfo_t) -> c_int {
    // SAFETY: guaranteed by the caller, the stub below.
    let info = unsafe { frame.read_unaligned() };
    let relibc = info_from_kernel(&info);
    let signo = relibc.si_signo;
    unsafe { out.write(relibc) };
    signo
}

// See the module documentation. x19 is callee-saved, so it carries the frame
// across both calls; the kernel restores every register on SignalReturn.
global_asm!(
    ".globl __relibc_qnx_signalstub",
    ".hidden __relibc_qnx_signalstub",
    ".type __relibc_qnx_signalstub, @function",
    "__relibc_qnx_signalstub:",
    "mov x19, x0",
    "sub sp, sp, #{slot}",
    "mov x1, sp",
    "bl {translate}",
    "mov x1, sp",
    "ldr x2, [x19, #56]",
    "ldr x3, [x19, #48]",
    "blr x3",
    "add sp, sp, #{slot}",
    "mov x0, x19",
    "b {signal_return}",
    ".size __relibc_qnx_signalstub, . - __relibc_qnx_signalstub",
    slot = const INFO_SLOT,
    translate = sym translate_frame,
    signal_return = sym super::kernel::signal_return,
);

unsafe extern "C" {
    fn __relibc_qnx_signalstub(frame: *mut c_void);
}

impl PalSignal for Sys {
    #[expect(deprecated)]
    fn getitimer(which: c_int, out: &mut itimerval) -> Result<()> {
        if which != ITIMER_REAL {
            return Err(Errno(EINVAL));
        }
        let mut old = Itimer::default();
        super::time::itimer_alarm(None, Some(&mut old))?;
        *out = kernel_to_itimerval(&old);
        Ok(())
    }

    fn kill(pid: pid_t, sig: c_int) -> Result<()> {
        let sig = to_kernel(sig).ok_or(Errno(EINVAL))?;
        signal_kill(ND_LOCAL_NODE, pid, 0, sig, si_code::USER, 0)
    }

    fn sigqueue(pid: pid_t, sig: c_int, val: sigval) -> Result<()> {
        // SAFETY: `sigval` is a union of an int and a pointer; the int arm is
        // what the kernel carries in the signal's value field.
        let value = unsafe { val.sival_int };
        let sig = to_kernel(sig).ok_or(Errno(EINVAL))?;
        signal_kill(ND_LOCAL_NODE, pid, 0, sig, si_code::QUEUE, value)
    }

    fn killpg(pgrp: pid_t, sig: c_int) -> Result<()> {
        let sig = to_kernel(sig).ok_or(Errno(EINVAL))?;
        signal_kill(ND_LOCAL_NODE, -pgrp, 0, sig, si_code::USER, 0)
    }

    fn raise(sig: c_int) -> Result<()> {
        let sig = to_kernel(sig).ok_or(Errno(EINVAL))?;
        signal_kill(ND_LOCAL_NODE, proc::getpid(), tls::tid(), sig, si_code::USER, 0)
    }

    #[expect(deprecated)]
    fn setitimer(which: c_int, new: &itimerval, old: Option<&mut itimerval>) -> Result<()> {
        if which != ITIMER_REAL {
            return Err(Errno(EINVAL));
        }
        let itime = itimerval_to_kernel(new)?;
        let mut prev = Itimer::default();
        super::time::itimer_alarm(Some(&itime), old.as_ref().map(|_| &mut prev))?;
        if let Some(old) = old {
            *old = kernel_to_itimerval(&prev);
        }
        Ok(())
    }

    fn sigaction(sig: c_int, act: Option<&sigaction>, oact: Option<&mut sigaction>) -> Result<()> {
        let signo = to_kernel(sig).ok_or(Errno(EINVAL))?;

        let converted = act.map(qnx_sa::from_relibc);
        let mut old = qnx_sa::Sigaction::default();

        unsafe {
            signal_action(
                0,
                __relibc_qnx_signalstub as *const () as usize,
                signo,
                converted
                    .as_ref()
                    .map_or(core::ptr::null(), core::ptr::from_ref),
                oact.as_ref()
                    .map_or(core::ptr::null_mut(), |_| core::ptr::from_mut(&mut old)),
            )
        }?;

        if let Some(oact) = oact {
            *oact = qnx_sa::to_relibc(&old);
        }
        Ok(())
    }

    unsafe fn sigaltstack(ss: Option<&stack_t>, old_ss: Option<&mut stack_t>) -> Result<()> {
        // In and out through one buffer, as the vendor sigaltstack does: a
        // null stack with no flags only reads the current one back.
        let mut stack = Stack::default();
        if let Some(ss) = ss {
            stack = match ss.ss_flags {
                0 => Stack {
                    ss_sp: ss.ss_sp as usize,
                    ss_size: ss.ss_size,
                    ..Stack::default()
                },
                flags if flags == SS_DISABLE as c_int => Stack {
                    ss_flags: ss::DISABLE,
                    ..Stack::default()
                },
                _ => return Err(Errno(EINVAL)),
            };
        }
        if !version::HAS_TCTL_SIGSTACK {
            if stack.ss_sp != 0 {
                return Err(Errno(ENOSYS));
            }
            stack = Stack {
                ss_flags: ss::DISABLE,
                ..Stack::default()
            };
        } else {
            unsafe { thread_ctl(tctl::SIGSTACK, core::ptr::from_mut(&mut stack).cast()) }?;
        }
        if let Some(old_ss) = old_ss {
            let mut flags = 0;
            if stack.ss_flags & ss::ONSTACK != 0 {
                flags |= SS_ONSTACK as c_int;
            }
            if stack.ss_flags & ss::DISABLE != 0 {
                flags |= SS_DISABLE as c_int;
            }
            *old_ss = stack_t {
                ss_sp: core::ptr::with_exposed_provenance_mut(stack.ss_sp),
                ss_flags: flags,
                ss_size: stack.ss_size,
            };
        }
        Ok(())
    }

    fn sigpending(set: &mut sigset_t) -> Result<()> {
        let mut pending = [0u32; 2];
        unsafe {
            signal_procmask(
                0,
                0,
                how::PENDING,
                core::ptr::null(),
                pending.as_mut_ptr().cast(),
            )
        }?;
        *set = set_from_kernel(pending);
        Ok(())
    }

    fn sigprocmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> Result<()> {
        // `how` only matters when there is a set to apply.
        let qnx_how = match how {
            _ if set.is_none() => how::BLOCK,
            SIG_BLOCK => how::BLOCK,
            SIG_UNBLOCK => how::UNBLOCK,
            SIG_SETMASK => how::SETMASK,
            _ => return Err(Errno(EINVAL)),
        };
        let new = set.map(|s| set_to_kernel(*s));
        let mut old = [0u32; 2];
        unsafe {
            signal_procmask(
                proc::getpid(),
                tls::tid(),
                qnx_how,
                new.as_ref().map_or(core::ptr::null(), |w| w.as_ptr().cast()),
                if oset.is_some() {
                    old.as_mut_ptr().cast()
                } else {
                    core::ptr::null_mut()
                },
            )
        }?;
        if let Some(oset) = oset {
            *oset = set_from_kernel(old);
        }
        Ok(())
    }

    fn sigsuspend(mask: &sigset_t) -> Errno {
        let words = set_to_kernel(*mask);
        match unsafe { signal_suspend(words.as_ptr().cast()) } {
            Ok(()) => Errno(EINTR),
            Err(err) => err,
        }
    }

    fn sigtimedwait(
        set: &sigset_t,
        sig: Option<&mut siginfo_t>,
        tp: Option<&timespec>,
    ) -> Result<c_int> {
        let words = set_to_kernel(*set);
        if let Some(tp) = tp {
            // Arms a one-shot timeout for the wait below, as the vendor
            // sigtimedwait does.
            let nsec = timespec_to_nsec(tp)?;
            unsafe {
                timer_timeout(
                    clock::MONOTONIC,
                    timeout::SIGWAITINFO,
                    core::ptr::null(),
                    &nsec,
                    core::ptr::null_mut(),
                )
            }?;
        }
        let mut info = Siginfo::default();
        let signo = match unsafe {
            signal_waitinfo(words.as_ptr().cast(), core::ptr::from_mut(&mut info).cast())
        } {
            Err(Errno(ETIMEDOUT)) => return Err(Errno(EAGAIN)),
            other => other?,
        };
        if let Some(sig) = sig {
            *sig = info_from_kernel(&info);
        }
        Ok(from_kernel(signo).unwrap_or(signo))
    }
}

/// Keeps the disposition constants referenced so that a future conversion can
/// find them next to their only users.
const _: () = {
    let _ = SIG_DFL;
    let _ = SIG_IGN;
};
