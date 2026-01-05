//! Clocks and sleeping.
//!
//! Neutrino's `ClockTime` works in whole nanoseconds rather than a `timespec`,
//! and `nanosleep` is not a kernel call at all: the vendor implementation is a
//! tail call to `timer_timeout(CLOCK_REALTIME, _NTO_TIMEOUT_NANOSLEEP, NULL,
//! rqtp, rmtp)`, where the timeout flag is `1 << STATE_NANOSLEEP`.

use super::{
    abi::neutrino::{clock, timeout, timer_flags, Itimer, Sigevent, TimerInfo},
    version::sigev,
    kernel::{
        ClockPeriod, clock_period, clock_time, timer_alarm as ker_timer_alarm,
        timer_create as ker_timer_create, timer_destroy, timer_info as ker_timer_info,
        timer_settime as ker_timer_settime, timer_timeout,
    },
};
use crate::{
    error::{Errno, Result},
    header::{
        errno::{EINVAL, ENOTSUP},
        signal::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, sigevent},
        time::{
            CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
            TIMER_ABSTIME, itimerspec, timespec,
        },
    },
    out::Out,
    platform::types::*,
};

const NSEC_PER_SEC: i64 = 1_000_000_000;

/// Maps a relibc `clockid_t` to the kernel's clock numbering.
pub(crate) fn to_kernel_clock(clk: clockid_t) -> Result<c_int> {
    Ok(match clk {
        CLOCK_REALTIME => clock::REALTIME,
        CLOCK_MONOTONIC => clock::MONOTONIC,
        CLOCK_PROCESS_CPUTIME_ID => clock::PROCESS_CPUTIME_ID,
        CLOCK_THREAD_CPUTIME_ID => clock::THREAD_CPUTIME_ID,
        _ => return Err(Errno(EINVAL)),
    })
}

pub(crate) fn to_nanos(ts: &timespec) -> Result<u64> {
    if ts.tv_nsec < 0 || ts.tv_nsec >= NSEC_PER_SEC || ts.tv_sec < 0 {
        return Err(Errno(EINVAL));
    }
    ts.tv_sec
        .checked_mul(NSEC_PER_SEC)
        .and_then(|secs| secs.checked_add(ts.tv_nsec))
        .map(|nanos| nanos as u64)
        .ok_or(Errno(EINVAL))
}

pub(crate) fn from_nanos(nanos: u64) -> timespec {
    timespec {
        tv_sec: (nanos / NSEC_PER_SEC as u64) as time_t,
        tv_nsec: (nanos % NSEC_PER_SEC as u64) as c_long,
    }
}

/// `clock_gettime()`.
pub fn clock_gettime(clk: clockid_t, mut tp: Out<timespec>) -> Result<()> {
    let id = to_kernel_clock(clk)?;
    let mut nanos: u64 = 0;
    unsafe { clock_time(id, core::ptr::null(), &mut nanos) }?;
    tp.write(from_nanos(nanos));
    Ok(())
}

/// `clock_settime()`.
///
/// # Safety
///
/// `tp` must be a valid `timespec`.
pub unsafe fn clock_settime(clk: clockid_t, tp: *const timespec) -> Result<()> {
    let id = to_kernel_clock(clk)?;
    let nanos = to_nanos(&unsafe { tp.read() })?;
    unsafe { clock_time(id, &nanos, core::ptr::null_mut()) }
}

/// `clock_getres()`, which reports the clock's tick period.
pub fn clock_getres(clk: clockid_t, res: Option<Out<timespec>>) -> Result<()> {
    let id = to_kernel_clock(clk)?;
    let mut period = ClockPeriod::default();
    unsafe { clock_period(id, core::ptr::null(), &mut period) }?;
    if let Some(mut res) = res {
        res.write(from_nanos(u64::from(period.nsec)));
    }
    Ok(())
}

/// `nanosleep()`.
///
/// # Safety
///
/// `rqtp` must be valid, and `rmtp` must be null or writable.
pub unsafe fn nanosleep(rqtp: *const timespec, rmtp: *mut timespec) -> Result<()> {
    let nanos = to_nanos(&unsafe { rqtp.read() })?;
    let mut remaining: u64 = 0;
    let result = unsafe {
        timer_timeout(
            clock::REALTIME,
            timeout::NANOSLEEP,
            core::ptr::null(),
            &nanos,
            &mut remaining,
        )
    };
    if !rmtp.is_null() {
        unsafe { rmtp.write(from_nanos(remaining)) };
    }
    result.map(|_| ())
}

/// `clock_nanosleep()`, which relibc declares but neither implements nor
/// exports; the backend provides it, and `tools/qnx-headers/time.h` its
/// declaration.
///
/// The vendor implementation is `nanosleep`'s timeout with the caller's clock
/// and, for `TIMER_ABSTIME`, the kernel's absolute flag; the remaining time is
/// only reported for a relative sleep. It returns the error number rather than
/// setting `errno`.
///
/// # Safety
///
/// `rqtp` must be null or valid, and `rmtp` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_nanosleep(
    clock_id: clockid_t,
    flags: c_int,
    rqtp: *const timespec,
    rmtp: *mut timespec,
) -> c_int {
    match unsafe { clock_sleep(clock_id, flags, rqtp, rmtp) } {
        Ok(()) => 0,
        Err(Errno(err)) => err,
    }
}

unsafe fn clock_sleep(
    clock_id: clockid_t,
    flags: c_int,
    rqtp: *const timespec,
    rmtp: *mut timespec,
) -> Result<()> {
    let id = match clock_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC => to_kernel_clock(clock_id)?,
        // POSIX: sleeping on a CPU-time clock is unsupported, and on the
        // calling thread's own an error.
        CLOCK_PROCESS_CPUTIME_ID => return Err(Errno(ENOTSUP)),
        _ => return Err(Errno(EINVAL)),
    };
    if rqtp.is_null() {
        return Err(Errno(EINVAL));
    }
    let nanos = to_nanos(&unsafe { rqtp.read() })?;
    let absolute = flags & TIMER_ABSTIME != 0;
    let mode = timeout::NANOSLEEP
        | if absolute {
            timer_flags::ABSTIME as c_uint
        } else {
            0
        };
    let mut remaining: u64 = 0;
    let result = unsafe { timer_timeout(id, mode, core::ptr::null(), &nanos, &mut remaining) };
    if !absolute && !rmtp.is_null() {
        unsafe { rmtp.write(from_nanos(remaining)) };
    }
    result.map(|_| ())
}

/// Sleeps for a whole number of nanoseconds, ignoring interruptions.
pub fn sleep_nanos(nanos: u64) {
    let _ = unsafe {
        timer_timeout(
            clock::REALTIME,
            timeout::NANOSLEEP,
            core::ptr::null(),
            &nanos,
            core::ptr::null_mut(),
        )
    };
}

fn to_kernel_sigevent(ev: &sigevent) -> Result<Sigevent> {
    let notify = match ev.sigev_notify {
        SIGEV_SIGNAL => sigev::SIGNAL,
        SIGEV_NONE => sigev::NONE,
        SIGEV_THREAD => sigev::THREAD,
        _ => return Err(Errno(EINVAL)),
    };
    let un1 = if ev.sigev_notify == SIGEV_SIGNAL {
        u64::from(super::abi::signal::to_kernel(ev.sigev_signo).ok_or(Errno(EINVAL))? as u32)
    } else {
        0
    };
    Ok(Sigevent {
        notify,
        pad: 0,
        un1,
        value: unsafe { ev.sigev_value.sival_ptr as usize as u64 },
        un2: 0,
    })
}

fn to_itimer(spec: &itimerspec) -> Result<Itimer> {
    Ok(Itimer {
        interval_nsec: to_nanos(&spec.it_interval)?,
        expire_nsec: to_nanos(&spec.it_value)?,
    })
}

fn from_itimer(it: &Itimer) -> itimerspec {
    itimerspec {
        it_interval: from_nanos(it.interval_nsec),
        it_value: from_nanos(it.expire_nsec),
    }
}

/// `timer_create()`.
pub fn timer_create(clock_id: clockid_t, evp: &sigevent) -> Result<timer_t> {
    let id = to_kernel_clock(clock_id)?;
    let event = to_kernel_sigevent(evp)?;
    let timer = unsafe { ker_timer_create(id, &event) }?;
    Ok(timer as usize as timer_t)
}

/// `timer_delete()`.
pub fn timer_delete(timerid: timer_t) -> Result<()> {
    timer_destroy(timerid as usize as c_int)
}

/// `timer_gettime()`.
pub fn timer_gettime(timerid: timer_t) -> Result<itimerspec> {
    let mut info = TimerInfo::default();
    unsafe { ker_timer_info(0, timerid as usize as c_int, 0, &mut info) }?;
    Ok(from_itimer(&info.otime))
}

/// `timer_settime()`.
pub fn timer_settime(
    timerid: timer_t,
    flags: c_int,
    value: &itimerspec,
    ovalue: Option<Out<itimerspec>>,
) -> Result<()> {
    if flags & !TIMER_ABSTIME != 0 {
        return Err(Errno(EINVAL));
    }
    let qnx_flags = if flags & TIMER_ABSTIME != 0 {
        timer_flags::ABSTIME
    } else {
        0
    };
    let itime = to_itimer(value)?;
    let mut old = Itimer::default();
    unsafe {
        ker_timer_settime(
            timerid as usize as c_int,
            qnx_flags,
            &itime,
            if ovalue.is_some() {
                &mut old
            } else {
                core::ptr::null_mut()
            },
        )
    }?;
    if let Some(mut ovalue) = ovalue {
        ovalue.write(from_itimer(&old));
    }
    Ok(())
}

/// `setitimer(ITIMER_REAL)` / `getitimer(ITIMER_REAL)` via `TimerAlarm`.
pub fn itimer_alarm(new: Option<&Itimer>, old: Option<&mut Itimer>) -> Result<()> {
    unsafe {
        ker_timer_alarm(
            clock::REALTIME,
            new.map_or(core::ptr::null(), ptr::from_ref),
            old.map_or(core::ptr::null_mut(), ptr::from_mut),
        )
    }
}

use core::ptr;
