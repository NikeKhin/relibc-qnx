//! Typed wrappers over the Neutrino kernel calls.
//!
//! These correspond one-to-one with the `*_r` entry points in the vendor libc
//! (the variants that report failure by return value instead of by setting
//! `errno`), which is what relibc's `Result`-based platform layer wants.

use core::ptr;

use super::{
    abi::neutrino::{IoVec, MsgInfo, SchedParam, Sync, ThreadAttr},
    kercall::{kercall1, kercall2, kercall3, kercall4, kercall5, kercall6},
    version::ker,
};
use crate::{error::Result, platform::types::*};

/// `MsgSendv`: sends gathered data and scatters the reply.
///
/// # Safety
///
/// The iovec arrays must be valid for the given part counts, and the buffers
/// they describe must remain valid for the duration of the call.
#[inline]
pub unsafe fn msg_sendv(
    coid: c_int,
    siov: *const IoVec,
    sparts: usize,
    riov: *mut IoVec,
    rparts: usize,
) -> Result<isize> {
    unsafe {
        kercall5(
            ker::MSG_SENDV,
            coid as usize,
            siov as usize,
            sparts,
            riov as usize,
            rparts,
        )
    }
    .map(|ret| ret as isize)
}

/// `MsgSend`: the scalar form of [`msg_sendv`].
///
/// Neutrino has no separate kernel call for this. The scalar form is encoded by
/// negating the part counts, which the vendor `MsgSend_r` veneer does with
/// `neg x2, x2; neg x4, x4` before entering the kernel.
///
/// # Safety
///
/// Both buffers must be valid for the given lengths.
#[inline]
pub unsafe fn msg_send(
    coid: c_int,
    smsg: *const u8,
    sbytes: usize,
    rmsg: *mut u8,
    rbytes: usize,
) -> Result<isize> {
    unsafe {
        kercall5(
            ker::MSG_SENDV,
            coid as usize,
            smsg as usize,
            (sbytes as isize).wrapping_neg() as usize,
            rmsg as usize,
            (rbytes as isize).wrapping_neg() as usize,
        )
    }
    .map(|ret| ret as isize)
}

/// `MsgSendnc`: like [`msg_send`] but not a cancellation point.
///
/// # Safety
///
/// Both buffers must be valid for the given lengths.
#[inline]
pub unsafe fn msg_send_nc(
    coid: c_int,
    smsg: *const u8,
    sbytes: usize,
    rmsg: *mut u8,
    rbytes: usize,
) -> Result<isize> {
    unsafe {
        kercall5(
            ker::MSG_SENDVNC,
            coid as usize,
            smsg as usize,
            (sbytes as isize).wrapping_neg() as usize,
            rmsg as usize,
            (rbytes as isize).wrapping_neg() as usize,
        )
    }
    .map(|ret| ret as isize)
}

/// `MsgSendvnc`: like [`msg_sendv`] but not a cancellation point.
///
/// # Safety
///
/// The iovec arrays must be valid for the given part counts.
#[inline]
pub unsafe fn msg_sendvnc(
    coid: c_int,
    siov: *const IoVec,
    sparts: usize,
    riov: *mut IoVec,
    rparts: usize,
) -> Result<isize> {
    unsafe {
        kercall5(
            ker::MSG_SENDVNC,
            coid as usize,
            siov as usize,
            sparts,
            riov as usize,
            rparts,
        )
    }
    .map(|ret| ret as isize)
}

/// Sends a fixed message struct and receives a fixed reply struct.
///
/// # Safety
///
/// `S` and `R` must be `#[repr(C)]` types whose layouts match what the server
/// expects.
#[inline]
pub unsafe fn msg_send_struct<S, R>(coid: c_int, send: &S, reply: &mut R) -> Result<isize> {
    unsafe {
        msg_send(
            coid,
            ptr::from_ref(send).cast::<u8>(),
            size_of::<S>(),
            ptr::from_mut(reply).cast::<u8>(),
            size_of::<R>(),
        )
    }
}

/// `MsgInfo`: retrieves information about a message.
///
/// # Safety
///
/// `info` must be a valid, writable `MsgInfo`.
#[inline]
pub unsafe fn msg_info(rcvid: isize, info: *mut MsgInfo) -> Result<()> {
    unsafe { kercall2(ker::MSG_INFO, rcvid as usize, info as usize) }.map(|_| ())
}

/// `ConnectAttach`: creates a connection to a channel.
///
/// `index` is the lowest connection id to allocate; passing zero yields the
/// lowest free file descriptor, which is what `open()` needs.
#[inline]
pub fn connect_attach(
    nd: u32,
    pid: pid_t,
    chid: c_int,
    index: c_int,
    flags: c_uint,
) -> Result<c_int> {
    unsafe {
        kercall5(
            ker::CONNECT_ATTACH,
            nd as usize,
            pid as usize,
            chid as usize,
            index as usize,
            flags as usize,
        )
    }
    .map(|coid| coid as c_int)
}

/// `ConnectDetach`: tears down a connection.
#[inline]
pub fn connect_detach(coid: c_int) -> Result<()> {
    unsafe { kercall1(ker::CONNECT_DETACH, coid as usize) }.map(|_| ())
}

/// `ConnectFlags`: reads and modifies the per-connection flags, which is how
/// `FD_CLOEXEC` is stored on QNX.
#[inline]
pub fn connect_flags(pid: pid_t, coid: c_int, mask: c_uint, bits: c_uint) -> Result<c_uint> {
    unsafe {
        kercall4(
            ker::CONNECT_FLAGS,
            pid as usize,
            coid as usize,
            mask as usize,
            bits as usize,
        )
    }
    .map(|flags| flags as c_uint)
}

/// `ConnectServerInfo`: describes the server behind a connection.
///
/// # Safety
///
/// `info` must be a valid, writable `MsgInfo`.
#[inline]
pub unsafe fn connect_server_info(pid: pid_t, coid: c_int, info: *mut MsgInfo) -> Result<c_int> {
    unsafe {
        kercall3(
            ker::CONNECT_SERVER_INFO,
            pid as usize,
            coid as usize,
            info as usize,
        )
    }
    .map(|coid| coid as c_int)
}

/// `ConnectClientInfo`: credentials of the process behind a connection.
///
/// Passing `scoid` of -1 asks about the caller. `ngroups` is the number of
/// supplementary-group slots the buffer can hold.
///
/// 8.0 replaced this kernel call with `ConnectClientInfoExtraArgs`, which also
/// reads an abilities buffer, its length, and an extra pointer from `x3`-`x5`;
/// the vendor `ConnectClientInfo` passes zeros for all three. 7.1 ignores
/// those registers, so passing the zeros explicitly serves both.
///
/// # Safety
///
/// `info` must be large enough for a `_client_info` with `ngroups` gids.
#[inline]
pub unsafe fn connect_client_info(scoid: c_int, info: *mut u8, ngroups: c_int) -> Result<()> {
    unsafe {
        kercall6(
            ker::CONNECT_CLIENT_INFO,
            scoid as usize,
            info as usize,
            ngroups as usize,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

/// `ChannelCreate`: creates a channel that this process can receive on.
#[inline]
pub fn channel_create(flags: c_uint) -> Result<c_int> {
    unsafe { kercall1(ker::CHANNEL_CREATE, flags as usize) }.map(|chid| chid as c_int)
}

/// `ChannelDestroy`.
#[inline]
pub fn channel_destroy(chid: c_int) -> Result<()> {
    unsafe { kercall1(ker::CHANNEL_DESTROY, chid as usize) }.map(|_| ())
}

/// `ClockTime`: reads or sets one of the kernel's clocks, in nanoseconds.
///
/// # Safety
///
/// The pointers must be null or valid for the direction they are used in.
#[inline]
pub unsafe fn clock_time(id: c_int, new: *const u64, old: *mut u64) -> Result<()> {
    unsafe {
        kercall3(
            ker::CLOCK_TIME,
            id as usize,
            new as usize,
            old as usize,
        )
    }
    .map(|_| ())
}

/// `ClockPeriod`: reads or sets a clock's tick period.
///
/// # Safety
///
/// The pointers must be null or valid `struct _clockperiod`s.
#[inline]
pub unsafe fn clock_period(id: c_int, new: *const ClockPeriod, old: *mut ClockPeriod) -> Result<()> {
    unsafe {
        kercall4(
            ker::CLOCK_PERIOD,
            id as usize,
            new as usize,
            old as usize,
            0,
        )
    }
    .map(|_| ())
}

/// `struct _clockperiod`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ClockPeriod {
    pub nsec: u32,
    pub fract: i32,
}

/// `TimerTimeout`: arms a timeout for the next blocking state the calling
/// thread enters.
///
/// With `_NTO_TIMEOUT_NANOSLEEP` this call blocks by itself, which is exactly
/// how the vendor `nanosleep` is implemented: it tail-calls
/// `timer_timeout(CLOCK_REALTIME, 0x1000, NULL, rqtp, rmtp)`.
///
/// # Safety
///
/// `notify`, `ntime` and `otime` must be null or valid.
#[inline]
pub unsafe fn timer_timeout(
    id: c_int,
    flags: c_uint,
    notify: *const c_void,
    ntime: *const u64,
    otime: *mut u64,
) -> Result<c_int> {
    unsafe {
        kercall5(
            ker::TIMER_TIMEOUT,
            id as usize,
            flags as usize,
            notify as usize,
            ntime as usize,
            otime as usize,
        )
    }
    .map(|prev| prev as c_int)
}

/// `SchedYield`.
#[inline]
pub fn sched_yield() -> Result<()> {
    unsafe { kercall1(ker::SCHED_YIELD, 0) }.map(|_| ())
}

/// `SchedGet`: reads a thread's scheduling policy and parameters.
///
/// # Safety
///
/// `param` must be null or a valid, writable `SchedParam`.
#[inline]
pub unsafe fn sched_get(pid: pid_t, tid: c_int, param: *mut SchedParam) -> Result<c_int> {
    unsafe {
        kercall3(
            ker::SCHED_GET,
            pid as usize,
            tid as usize,
            param as usize,
        )
    }
    .map(|policy| policy as c_int)
}

/// `SchedSet`: sets a thread's scheduling policy and parameters.
///
/// # Safety
///
/// `param` must be null or a valid `SchedParam`.
#[inline]
pub unsafe fn sched_set(
    pid: pid_t,
    tid: c_int,
    policy: c_int,
    param: *const SchedParam,
) -> Result<c_int> {
    unsafe {
        kercall4(
            ker::SCHED_SET,
            pid as usize,
            tid as usize,
            policy as usize,
            param as usize,
        )
    }
    .map(|policy| policy as c_int)
}

/// `ThreadCreate`: starts a thread in the given process.
///
/// # Safety
///
/// `attr` must describe a usable stack, and `func` must be a valid entry point
/// for the new thread.
#[inline]
pub unsafe fn thread_create(
    pid: pid_t,
    func: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
    attr: *const ThreadAttr,
) -> Result<c_int> {
    unsafe {
        kercall4(
            ker::THREAD_CREATE,
            pid as usize,
            func as usize,
            arg as usize,
            attr as usize,
        )
    }
    .map(|tid| tid as c_int)
}

/// `ThreadDestroy`: terminates threads with an exit status.
///
/// A `tid` of 0 destroys the caller alone; -1 destroys every thread in the
/// process, which ends it with `status`. The vendor `__pthread_exit` passes 0
/// and `_exit` passes -1.
#[inline]
pub fn thread_destroy(tid: c_int, priority: c_int, status: *mut c_void) -> Result<()> {
    unsafe {
        kercall3(
            ker::THREAD_DESTROY,
            tid as usize,
            priority as usize,
            status as usize,
        )
    }
    .map(|_| ())
}

/// `SysCpupageGet`: returns a cpupage slot, including the kernel TLS pointer.
#[inline]
pub fn sys_cpupage_get(index: usize) -> Result<usize> {
    unsafe { kercall1(ker::SYS_CPUPAGE_GET, index) }
}

/// `ThreadDetach`.
#[inline]
pub fn thread_detach(tid: c_int) -> Result<()> {
    unsafe { kercall1(ker::THREAD_DETACH, tid as usize) }.map(|_| ())
}

/// `ThreadJoin`.
///
/// # Safety
///
/// `status` must be null or a valid, writable pointer.
#[inline]
pub unsafe fn thread_join(pid: pid_t, tid: c_int, status: *mut *mut c_void) -> Result<()> {
    unsafe {
        kercall3(
            ker::THREAD_JOIN,
            pid as usize,
            tid as usize,
            status as usize,
        )
    }
    .map(|_| ())
}

/// `ThreadCtl`: per-thread control operations.
///
/// # Safety
///
/// `data` must match what `cmd` expects.
#[inline]
pub unsafe fn thread_ctl(cmd: c_int, data: *mut c_void) -> Result<c_int> {
    unsafe { kercall2(ker::THREAD_CTL, cmd as usize, data as usize) }.map(|ret| ret as c_int)
}

/// `SignalKill`: sends a signal to a process, process group or thread.
#[inline]
pub fn signal_kill(
    nd: u32,
    pid: pid_t,
    tid: c_int,
    signo: c_int,
    code: c_int,
    value: c_int,
) -> Result<()> {
    unsafe {
        kercall6(
            ker::SIGNAL_KILL,
            nd as usize,
            pid as usize,
            tid as usize,
            signo as usize,
            code as usize,
            value as usize,
        )
    }
    .map(|_| ())
}

/// `SignalAction`: installs a disposition for one signal.
///
/// `stub` is the address the kernel enters when the signal is delivered; it is
/// passed once per call rather than registered globally. See
/// `super::signal` for the trampoline itself.
///
/// # Safety
///
/// `act` and `oact` must be null or valid, and `stub` must point at a
/// trampoline that ends by calling [`signal_return`].
#[inline]
pub unsafe fn signal_action(
    pid: pid_t,
    stub: usize,
    signo: c_int,
    act: *const super::abi::neutrino::sigaction::Sigaction,
    oact: *mut super::abi::neutrino::sigaction::Sigaction,
) -> Result<()> {
    unsafe {
        kercall5(
            ker::SIGNAL_ACTION,
            pid as usize,
            stub,
            signo as usize,
            act as usize,
            oact as usize,
        )
    }
    .map(|_| ())
}

/// `SignalReturn`: resumes the interrupted context once a handler is done.
///
/// Declared `extern "C"` because the signal trampoline tail-calls it from
/// assembly. It does not return.
///
/// # Safety
///
/// `frame` must be the frame the kernel handed to the trampoline.
pub unsafe extern "C" fn signal_return(frame: *mut c_void) -> ! {
    unsafe {
        let _ = kercall1(ker::SIGNAL_RETURN, frame as usize);
    }
    // The kernel restores the interrupted context instead of returning here.
    loop {
        core::hint::spin_loop();
    }
}

/// `SignalProcmask`: examines and changes a thread's blocked-signal mask.
///
/// # Safety
///
/// The signal set pointers must be null or valid.
#[inline]
pub unsafe fn signal_procmask(
    pid: pid_t,
    tid: c_int,
    how: c_int,
    set: *const c_void,
    oldset: *mut c_void,
) -> Result<c_int> {
    unsafe {
        kercall5(
            ker::SIGNAL_PROCMASK,
            pid as usize,
            tid as usize,
            how as usize,
            set as usize,
            oldset as usize,
        )
    }
    .map(|ret| ret as c_int)
}

/// `SignalSuspend`: waits for a signal with a temporary mask.
///
/// # Safety
///
/// `set` must be a valid signal set.
#[inline]
pub unsafe fn signal_suspend(set: *const c_void) -> Result<()> {
    unsafe { kercall1(ker::SIGNAL_SUSPEND, set as usize) }.map(|_| ())
}

/// `SignalWaitinfo`: waits for one of a set of signals.
///
/// 8.0 turned this kernel call into `SignalWaitinfoMask`, whose third argument
/// is an optional mask to install while waiting; the vendor `SignalWaitinfo`
/// passes null. 7.1 ignores `x2`.
///
/// # Safety
///
/// Both pointers must be null or valid.
#[inline]
pub unsafe fn signal_waitinfo(set: *const c_void, info: *mut c_void) -> Result<c_int> {
    unsafe { kercall3(ker::SIGNAL_WAITINFO, set as usize, info as usize, 0) }
        .map(|signo| signo as c_int)
}

/// `SyncTypeCreate`: initialises a kernel synchronisation object.
///
/// # Safety
///
/// `sync` must point to writable storage for a `Sync`, and `attr` must be null
/// or a valid attribute block.
#[inline]
pub unsafe fn sync_type_create(type_: c_uint, sync: *mut Sync, attr: *const c_void) -> Result<()> {
    unsafe {
        kercall3(
            ker::SYNC_CREATE,
            type_ as usize,
            sync as usize,
            attr as usize,
        )
    }
    .map(|_| ())
}

/// `SyncDestroy`.
///
/// # Safety
///
/// `sync` must point to an initialised `Sync` that no thread is blocked on.
#[inline]
pub unsafe fn sync_destroy(sync: *mut Sync) -> Result<()> {
    unsafe { kercall1(ker::SYNC_DESTROY, sync as usize) }.map(|_| ())
}

/// `SyncMutexLock`.
///
/// # Safety
///
/// `sync` must point to an initialised mutex.
#[inline]
pub unsafe fn sync_mutex_lock(sync: *mut Sync) -> Result<()> {
    unsafe { kercall1(ker::SYNC_MUTEX_LOCK, sync as usize) }.map(|_| ())
}

/// `SyncMutexUnlock`.
///
/// # Safety
///
/// `sync` must point to a mutex held by the calling thread.
#[inline]
pub unsafe fn sync_mutex_unlock(sync: *mut Sync) -> Result<()> {
    unsafe { kercall1(ker::SYNC_MUTEX_UNLOCK, sync as usize) }.map(|_| ())
}

/// `SyncCondvarWait`: atomically releases `mutex` and blocks on `sync`.
///
/// # Safety
///
/// Both objects must be initialised and `mutex` must be held by the caller.
#[inline]
pub unsafe fn sync_condvar_wait(sync: *mut Sync, mutex: *mut Sync) -> Result<()> {
    unsafe { kercall2(ker::SYNC_CONDVAR_WAIT, sync as usize, mutex as usize) }.map(|_| ())
}

/// `SyncCondvarSignal`: wakes one or all waiters.
///
/// # Safety
///
/// `sync` must point to an initialised condition variable.
#[inline]
pub unsafe fn sync_condvar_signal(sync: *mut Sync, all: bool) -> Result<()> {
    unsafe { kercall2(ker::SYNC_CONDVAR_SIGNAL, sync as usize, usize::from(all)) }.map(|_| ())
}

/// `TimerCreate`.
///
/// # Safety
///
/// `event` must be a valid `sigevent` as the kernel sees it.
#[inline]
pub unsafe fn timer_create(clock_id: c_int, event: *const super::abi::neutrino::Sigevent) -> Result<c_int> {
    unsafe { kercall2(ker::TIMER_CREATE, clock_id as usize, event as usize) }.map(|id| id as c_int)
}

/// `TimerDestroy`.
#[inline]
pub fn timer_destroy(id: c_int) -> Result<()> {
    unsafe { kercall1(ker::TIMER_DESTROY, id as usize) }.map(|_| ())
}

/// `TimerSettime`.
///
/// # Safety
///
/// `itime` must be valid; `otime` must be null or writable.
#[inline]
pub unsafe fn timer_settime(
    id: c_int,
    flags: c_int,
    itime: *const super::abi::neutrino::Itimer,
    otime: *mut super::abi::neutrino::Itimer,
) -> Result<()> {
    unsafe {
        kercall4(
            ker::TIMER_SETTIME,
            id as usize,
            flags as usize,
            itime as usize,
            otime as usize,
        )
    }
    .map(|_| ())
}

/// `TimerInfo`.
///
/// # Safety
///
/// `info` must be null or a writable `_timer_info`.
#[inline]
pub unsafe fn timer_info(
    pid: pid_t,
    id: c_int,
    flags: c_int,
    info: *mut super::abi::neutrino::TimerInfo,
) -> Result<c_int> {
    unsafe {
        kercall4(
            ker::TIMER_INFO,
            pid as usize,
            id as usize,
            flags as usize,
            info as usize,
        )
    }
    .map(|id| id as c_int)
}

/// `TimerAlarm`.
///
/// # Safety
///
/// `itime` and `otime` must be null or valid `_itimer`s.
#[inline]
pub unsafe fn timer_alarm(
    clock_id: c_int,
    itime: *const super::abi::neutrino::Itimer,
    otime: *mut super::abi::neutrino::Itimer,
) -> Result<()> {
    unsafe {
        kercall3(
            ker::TIMER_ALARM,
            clock_id as usize,
            itime as usize,
            otime as usize,
        )
    }
    .map(|_| ())
}
