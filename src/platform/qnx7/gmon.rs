//! gprof support, which programs built with `-p` or `-pg` need, as the QNX
//! SDP's libc provides it: `monstartup`, `moncontrol`, `_mcleanup`, `profil`
//! and the `_mcount` every profiled function calls. The start object of a
//! profiled program (`gcrt1.o`, `mcrt1.o`) calls `monstartup` before `main`
//! and `_mcleanup` at exit, which writes GNU gprof's `gmon.out`.
//!
//! The call graph comes from `_mcount`, which counts each caller-callee arc
//! with glibc's tables. The flat profile comes from `profil`, which samples
//! the program counter: a timer raises `SIGPROF`, and the handler reads the
//! interrupted program counter from the kernel's context. Where the context
//! keeps it is found once, by signalling the process from a kernel call
//! whose return address is known (see [`calibrate`]); if it cannot be found,
//! `profil` fails and `gmon.out` holds the call graph only, which is what
//! the vendor's `profil` stub gives.
//!
//! QNX has no `SA_RESTART`, so a `SIGPROF` that arrives during a blocking
//! call makes it fail with `EINTR`. `RELIBC_GMON_HZ` sets the sampling rate
//! of a profiled program (100 by default; 0 samples nothing).

use alloc::{
    alloc::{Layout, alloc_zeroed},
    vec::Vec,
};
use core::{
    ffi::{CStr as RustCStr, c_void},
    mem::size_of,
    ptr,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering},
};

use super::{abi::signal::to_kernel, proc, tls};
use crate::{
    c_str::CStr,
    header::bits_sigset_t::sigset_t,
    header::{
        errno::ENOSYS,
        fcntl::{O_CLOEXEC, O_CREAT, O_TRUNC, O_WRONLY},
        signal::{SA_SIGINFO, SIG_BLOCK, SIGEV_SIGNAL, SIGPROF, sigaction, sigevent, siginfo_t},
        stdlib::getenv,
        time::{CLOCK_MONOTONIC, itimerspec, timespec},
    },
    platform::{self, Pal, PalSignal, Sys, types::*},
};

// glibc's sizing: two bytes of text per histogram counter and per arc hash
// slot, and three arcs for every hundred bytes of text.
const HISTFRACTION: usize = 2;
const HASHFRACTION: usize = 2;
const ARCDENSITY: usize = 3;
const MINARCS: usize = 50;
const MAXARCS: usize = 1 << 20;
const SCALE_1_TO_1: usize = 0x10000;

const STATE_ON: u32 = 0;
const STATE_BUSY: u32 = 1;
const STATE_ERROR: u32 = 2;
const STATE_OFF: u32 = 3;
/// `_mcleanup` has written `gmon.out`.
const STATE_DONE: u32 = 4;

#[repr(C)]
struct ToStruct {
    selfpc: usize,
    count: usize,
    link: usize,
}

/// The arc tables and histogram, set up by `monstartup`. Only whoever moves
/// [`STATE`] from on to busy touches the arc tables.
struct Gmon {
    lowpc: usize,
    highpc: usize,
    textsize: usize,
    kcount: *mut u16,
    kcountsize: usize,
    froms: *mut usize,
    fromssize: usize,
    tos: *mut ToStruct,
    tolimit: usize,
    scale: usize,
}

static mut GMON: Gmon = Gmon {
    lowpc: 0,
    highpc: 0,
    textsize: 0,
    kcount: ptr::null_mut(),
    kcountsize: 0,
    froms: ptr::null_mut(),
    fromssize: 0,
    tos: ptr::null_mut(),
    tolimit: 0,
    scale: 0,
};
static STATE: AtomicU32 = AtomicU32::new(STATE_OFF);

/// Where the kernel's signal context keeps the interrupted program counter,
/// plus one; zero until [`calibrate`] has found it.
static PC_SLOT: AtomicUsize = AtomicUsize::new(0);
/// The histogram `profil` fills, its counter count, the address of its first
/// counter and the scale, as `profil`'s caller gave them.
static SAMPLES: AtomicUsize = AtomicUsize::new(0);
static NSAMPLES: AtomicUsize = AtomicUsize::new(0);
static SAMPLE_OFFSET: AtomicUsize = AtomicUsize::new(0);
static SAMPLE_SCALE: AtomicUsize = AtomicUsize::new(0);
static TIMER: AtomicUsize = AtomicUsize::new(0);
static RATE: AtomicUsize = AtomicUsize::new(DEFAULT_RATE);
const DEFAULT_RATE: usize = 100;

// `_mcount(frompc)`: the profiled function passes its caller's return
// address, and its own is the return address of this call.
core::arch::global_asm!(
    ".globl _mcount",
    ".type _mcount, %function",
    "_mcount:",
    "mov x1, x30",
    "b {internal}",
    ".size _mcount, . - _mcount",
    internal = sym mcount_internal,
);

// (pid, tid, signo): sends the signal to the calling thread with
// `SignalKill`, which the kernel delivers as the call returns, to
// `__relibc_qnx_gmon_probe_return`; the error return is one instruction on.
core::arch::global_asm!(
    ".globl __relibc_qnx_gmon_probe",
    ".hidden __relibc_qnx_gmon_probe",
    ".type __relibc_qnx_gmon_probe, %function",
    "__relibc_qnx_gmon_probe:",
    "mov x3, x2",
    "mov x2, x1",
    "mov x1, x0",
    "mov x0, #0",
    "mov x4, #0",
    "mov x5, #0",
    "mov w8, #{kill}",
    "svc #{svc}",
    ".globl __relibc_qnx_gmon_probe_return",
    ".hidden __relibc_qnx_gmon_probe_return",
    "__relibc_qnx_gmon_probe_return:",
    "ret",
    "ret",
    ".size __relibc_qnx_gmon_probe, . - __relibc_qnx_gmon_probe",
    kill = const super::version::ker::SIGNAL_KILL,
    svc = const super::version::SVC_IMM,
);

unsafe extern "C" {
    fn __relibc_qnx_gmon_probe(pid: c_int, tid: c_int, signo: c_int);
    static __relibc_qnx_gmon_probe_return: u8;
}

/// How far into the kernel's signal context the program counter is looked
/// for: past the CPU registers, which come first.
const CONTEXT_SCAN: usize = 512;

/// Whether the calibration signal has been handled.
static CALIBRATED: AtomicBool = AtomicBool::new(false);

extern "C" fn calibrate_handler(_signo: c_int, _info: *mut siginfo_t, context: *mut c_void) {
    CALIBRATED.store(true, Ordering::Release);
    let want = &raw const __relibc_qnx_gmon_probe_return as usize;
    let mut offset = 0;
    while offset < CONTEXT_SCAN {
        // SAFETY: the kernel's context is larger than the scanned range.
        let word = unsafe { context.cast::<u8>().add(offset).cast::<usize>().read() };
        if word == want || word == want + 4 {
            PC_SLOT.store(offset + 1, Ordering::Relaxed);
            return;
        }
        offset += size_of::<usize>();
    }
}

extern "C" fn sample_handler(_signo: c_int, _info: *mut siginfo_t, context: *mut c_void) {
    let slot = PC_SLOT.load(Ordering::Relaxed);
    let samples = SAMPLES.load(Ordering::Relaxed);
    if slot == 0 || samples == 0 {
        return;
    }
    // SAFETY: `slot` was found inside this kind of context by `calibrate`.
    let pc = unsafe { context.cast::<u8>().add(slot - 1).cast::<usize>().read() };
    let Some(offset) = pc.checked_sub(SAMPLE_OFFSET.load(Ordering::Relaxed)) else {
        return;
    };
    let index = ((offset / 2) as u128 * SAMPLE_SCALE.load(Ordering::Relaxed) as u128) >> 16;
    if index < NSAMPLES.load(Ordering::Relaxed) as u128 {
        // SAFETY: `profil`'s caller gave `NSAMPLES` counters at `samples`;
        // two handlers can overlap on different threads, hence the atomic.
        let counter = unsafe { AtomicU16::from_ptr((samples as *mut u16).add(index as usize)) };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

fn set_handler(
    handler: extern "C" fn(c_int, *mut siginfo_t, *mut c_void),
    old: Option<&mut sigaction>,
) -> bool {
    // SAFETY: a `sigaction` of zeroes is a valid empty disposition.
    let mut act: sigaction = unsafe { core::mem::zeroed() };
    // SAFETY: with SA_SIGINFO the handler is called with three arguments.
    act.sa_handler = Some(unsafe {
        core::mem::transmute::<
            extern "C" fn(c_int, *mut siginfo_t, *mut c_void),
            extern "C" fn(c_int),
        >(handler)
    });
    act.sa_flags = SA_SIGINFO as c_int;
    Sys::sigaction(SIGPROF as c_int, Some(&act), old).is_ok()
}

/// Finds where the kernel's signal context keeps the program counter, by
/// sending this thread `SIGPROF` from a kernel call whose return address is
/// `__relibc_qnx_gmon_probe_return` and looking for that address.
///
/// The previous disposition comes back only once the signal has been
/// handled, so that a late delivery cannot meet `SIG_DFL`, which would end
/// the process.
fn calibrate() -> bool {
    if PC_SLOT.load(Ordering::Relaxed) != 0 {
        return true;
    }
    let Some(signo) = to_kernel(SIGPROF as c_int) else {
        return false;
    };
    let mut mask: sigset_t = 0;
    if Sys::sigprocmask(SIG_BLOCK, None, Some(&mut mask)).is_err()
        || mask & (1 << (SIGPROF - 1)) != 0
    {
        return false;
    }
    // SAFETY: zeroes are a valid disposition to restore into.
    let mut old: sigaction = unsafe { core::mem::zeroed() };
    if !set_handler(calibrate_handler, Some(&mut old)) {
        return false;
    }
    CALIBRATED.store(false, Ordering::Release);
    unsafe { __relibc_qnx_gmon_probe(proc::getpid(), tls::tid(), signo) };
    // Delivery normally happens as the probe returns; a kernel call is
    // another chance, should it not.
    for _ in 0..100 {
        if CALIBRATED.load(Ordering::Acquire) {
            break;
        }
        let _ = Sys::sched_yield();
    }
    if CALIBRATED.load(Ordering::Acquire) {
        let _ = Sys::sigaction(SIGPROF as c_int, Some(&old), None);
    }
    PC_SLOT.load(Ordering::Relaxed) != 0
}

fn stop_sampling() {
    let timer = TIMER.swap(0, Ordering::AcqRel);
    if timer != 0 {
        let _ = Sys::timer_delete(timer as timer_t);
    }
    SAMPLES.store(0, Ordering::Release);
}

/// `profil()`: samples the program counter into `buf`, `bufsiz` bytes of
/// 16-bit counters, counter `((pc - offset) / 2 * scale) >> 16` for each
/// sample; a scale below 2 or a null buffer stops sampling.
///
/// # Safety
///
/// `buf` must stay valid for `bufsiz` bytes until sampling is stopped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn profil(
    buf: *mut c_ushort,
    bufsiz: size_t,
    offset: size_t,
    scale: c_uint,
) -> c_int {
    stop_sampling();
    if buf.is_null() || scale < 2 {
        return 0;
    }
    let rate = RATE.load(Ordering::Relaxed);
    if rate == 0 || !calibrate() || !set_handler(sample_handler, None) {
        platform::ERRNO.set(ENOSYS);
        return -1;
    }
    NSAMPLES.store(bufsiz / size_of::<u16>(), Ordering::Relaxed);
    SAMPLE_OFFSET.store(offset, Ordering::Relaxed);
    SAMPLE_SCALE.store(scale as usize, Ordering::Relaxed);
    SAMPLES.store(buf as usize, Ordering::Release);

    // SAFETY: a zeroed `sigevent` is valid; the fields that matter are set.
    let mut event: sigevent = unsafe { core::mem::zeroed() };
    event.sigev_notify = SIGEV_SIGNAL;
    event.sigev_signo = SIGPROF as c_int;
    let period = timespec {
        tv_sec: 0,
        tv_nsec: (1_000_000_000 / rate.clamp(1, 1_000_000_000)) as c_long,
    };
    let spec = itimerspec {
        it_interval: period.clone(),
        it_value: period,
    };
    match Sys::timer_create(CLOCK_MONOTONIC, &event) {
        Ok(timer) if Sys::timer_settime(timer, 0, &spec, None).is_ok() => {
            TIMER.store(timer as usize, Ordering::Release);
            0
        }
        Ok(timer) => {
            let _ = Sys::timer_delete(timer);
            SAMPLES.store(0, Ordering::Release);
            -1
        }
        Err(err) => {
            SAMPLES.store(0, Ordering::Release);
            platform::ERRNO.set(err.0);
            -1
        }
    }
}

/// `__mcount_internal` from glibc: counts the arc from `frompc`, in a
/// profiled function's caller, to `selfpc`, in the function.
unsafe extern "C" fn mcount_internal(frompc: usize, selfpc: usize) {
    if STATE
        .compare_exchange(STATE_ON, STATE_BUSY, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    // SAFETY: moving the state to busy makes this the tables' only user.
    let p = unsafe { &*(&raw const GMON) };
    let state = unsafe { count_arc(p, frompc, selfpc) };
    STATE.store(state, Ordering::Release);
}

/// The table update, with the arc tables held; returns the new state.
unsafe fn count_arc(p: &Gmon, frompc: usize, selfpc: usize) -> u32 {
    let from = frompc.wrapping_sub(p.lowpc);
    if from >= p.textsize {
        return STATE_ON;
    }
    let slot = from / (HASHFRACTION * size_of::<usize>());
    if slot >= p.fromssize / size_of::<usize>() {
        return STATE_ON;
    }
    unsafe {
        let frompcindex = p.froms.add(slot);
        let mut toindex = *frompcindex;
        let new_arc = |link: usize| -> Option<usize> {
            let index = (*p.tos).link + 1;
            if index >= p.tolimit {
                return None;
            }
            (*p.tos).link = index;
            let top = p.tos.add(index);
            (*top).selfpc = selfpc;
            (*top).count = 1;
            (*top).link = link;
            Some(index)
        };
        if toindex == 0 {
            return match new_arc(0) {
                Some(index) => {
                    *frompcindex = index;
                    STATE_ON
                }
                None => STATE_ERROR,
            };
        }
        let mut top = p.tos.add(toindex);
        if (*top).selfpc == selfpc {
            (*top).count += 1;
            return STATE_ON;
        }
        loop {
            if (*top).link == 0 {
                // A new callee for this caller goes to the front of its chain.
                return match new_arc(*frompcindex) {
                    Some(index) => {
                        *frompcindex = index;
                        STATE_ON
                    }
                    None => STATE_ERROR,
                };
            }
            let prev = top;
            toindex = (*top).link;
            top = p.tos.add(toindex);
            if (*top).selfpc == selfpc {
                // Found: count it and move it to the front.
                (*top).count += 1;
                (*prev).link = (*top).link;
                (*top).link = *frompcindex;
                *frompcindex = toindex;
                return STATE_ON;
            }
        }
    }
}

/// `monstartup()`: profiles the text from `lowpc` to `highpc`.
///
/// # Safety
///
/// Must be called once, before any profiled function runs on another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn monstartup(lowpc: usize, highpc: usize) {
    let round = HISTFRACTION * size_of::<u16>();
    let lowpc = lowpc / round * round;
    let highpc = highpc.next_multiple_of(round);
    let textsize = highpc.saturating_sub(lowpc);
    let kcountsize = (textsize / HISTFRACTION).next_multiple_of(size_of::<usize>());
    let fromssize = (textsize / HASHFRACTION).next_multiple_of(size_of::<usize>());
    let tolimit = (textsize * ARCDENSITY / 100).clamp(MINARCS, MAXARCS);
    let tossize = tolimit * size_of::<ToStruct>();

    let total = tossize + kcountsize + fromssize;
    // A zeroed allocation, kept for the life of the process.
    let buf = match Layout::from_size_align(total, 16) {
        Ok(layout) if textsize != 0 => unsafe { alloc_zeroed(layout) },
        _ => ptr::null_mut(),
    };
    if buf.is_null() {
        STATE.store(STATE_ERROR, Ordering::Release);
        complain(b"monstartup: out of memory\n");
        return;
    }
    if let Some(rate) = env_usize(c"RELIBC_GMON_HZ") {
        RATE.store(rate, Ordering::Relaxed);
    }
    let scale = if kcountsize < textsize {
        ((kcountsize as u128 * SCALE_1_TO_1 as u128) / textsize as u128) as usize
    } else {
        SCALE_1_TO_1
    };
    // SAFETY: as documented, nothing else uses the tables yet.
    unsafe {
        GMON = Gmon {
            lowpc,
            highpc,
            textsize,
            tos: buf.cast::<ToStruct>(),
            tolimit,
            kcount: buf.add(tossize).cast::<u16>(),
            kcountsize,
            froms: buf.add(tossize + kcountsize).cast::<usize>(),
            fromssize,
            scale,
        };
    }
    unsafe { moncontrol(1) };
}

/// `moncontrol()`: starts (non-zero) or stops (zero) profiling.
///
/// # Safety
///
/// `monstartup` must have run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moncontrol(mode: c_int) {
    if matches!(STATE.load(Ordering::Acquire), STATE_ERROR | STATE_DONE) {
        return;
    }
    // SAFETY: the fields read here are set once, by `monstartup`.
    let p = unsafe { &*(&raw const GMON) };
    if p.kcount.is_null() {
        return;
    }
    if mode != 0 {
        // Sampling is optional: without it the call graph is still counted.
        let _ = unsafe { profil(p.kcount, p.kcountsize, p.lowpc, p.scale as c_uint) };
        STATE.store(STATE_ON, Ordering::Release);
    } else {
        let _ = unsafe { profil(ptr::null_mut(), 0, 0, 0) };
        // Wait out an `_mcount` in progress.
        while STATE
            .compare_exchange(STATE_ON, STATE_OFF, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            match STATE.load(Ordering::Acquire) {
                STATE_BUSY => core::hint::spin_loop(),
                _ => break,
            }
        }
    }
}

/// `_mcleanup()`: stops profiling and writes `gmon.out`, or
/// `$GMON_OUT_PREFIX.<pid>`, in GNU gprof's format.
///
/// # Safety
///
/// `monstartup` must have run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _mcleanup() {
    match STATE.load(Ordering::Acquire) {
        STATE_DONE => return,
        STATE_ERROR => {
            complain(b"_mcleanup: the arc table overflowed; the call graph is incomplete\n")
        }
        _ => unsafe { moncontrol(0) },
    }
    STATE.store(STATE_DONE, Ordering::Release);
    // SAFETY: profiling is stopped, so nothing else uses the tables.
    let p = unsafe { &*(&raw const GMON) };
    if p.kcount.is_null() {
        return;
    }
    let mut path = Vec::new();
    match unsafe { getenv(c"GMON_OUT_PREFIX".as_ptr()) } {
        prefix if !prefix.is_null() => {
            path.extend_from_slice(unsafe { CStr::from_ptr(prefix) }.to_bytes());
            path.push(b'.');
            path.extend_from_slice(alloc::format!("{}", proc::getpid()).as_bytes());
        }
        _ => path.extend_from_slice(b"gmon.out"),
    }
    path.push(0);
    let Ok(path) = CStr::from_bytes_with_nul(&path) else {
        return;
    };
    let Ok(fd) = Sys::open(path, O_CREAT | O_TRUNC | O_WRONLY | O_CLOEXEC, 0o666) else {
        complain(b"_mcleanup: cannot create gmon.out\n");
        return;
    };
    let mut out = Writer {
        fd,
        buf: Vec::with_capacity(64 * 1024),
        ok: true,
    };
    unsafe { write_gmon(&mut out, p) };
    out.flush();
    let _ = Sys::close(fd);
    if !out.ok {
        complain(b"_mcleanup: writing gmon.out failed\n");
    }
}

struct Writer {
    fd: c_int,
    buf: Vec<u8>,
    ok: bool,
}

impl Writer {
    fn put(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() >= 60 * 1024 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let mut done = 0;
        while self.ok && done < self.buf.len() {
            match Sys::write(self.fd, &self.buf[done..]) {
                Ok(0) | Err(_) => self.ok = false,
                Ok(n) => done += n,
            }
        }
        self.buf.clear();
    }
}

/// GNU gprof's format: a header, the histogram, then one record per arc.
unsafe fn write_gmon(out: &mut Writer, p: &Gmon) {
    out.put(b"gmon");
    out.put(&1u32.to_ne_bytes());
    out.put(&[0; 12]);

    out.put(&[0]); // GMON_TAG_TIME_HIST
    out.put(&p.lowpc.to_ne_bytes());
    out.put(&p.highpc.to_ne_bytes());
    let counters = p.kcountsize / size_of::<u16>();
    out.put(&(counters as u32).to_ne_bytes());
    out.put(&(RATE.load(Ordering::Relaxed) as u32).to_ne_bytes());
    let mut dimen = [0u8; 15];
    dimen[..7].copy_from_slice(b"seconds");
    out.put(&dimen);
    out.put(b"s");
    // SAFETY: `monstartup` allocated `kcountsize` bytes of counters.
    out.put(unsafe { core::slice::from_raw_parts(p.kcount.cast::<u8>(), p.kcountsize) });

    let slots = p.fromssize / size_of::<usize>();
    for slot in 0..slots {
        let frompc = p.lowpc + slot * HASHFRACTION * size_of::<usize>();
        let mut toindex = unsafe { *p.froms.add(slot) };
        while toindex != 0 && toindex < p.tolimit {
            let top = unsafe { &*p.tos.add(toindex) };
            out.put(&[1]); // GMON_TAG_CG_ARC
            out.put(&frompc.to_ne_bytes());
            out.put(&top.selfpc.to_ne_bytes());
            out.put(&(top.count.min(u32::MAX as usize) as u32).to_ne_bytes());
            toindex = top.link;
        }
    }
}

fn env_usize(name: &RustCStr) -> Option<usize> {
    let value = unsafe { getenv(name.as_ptr()) };
    if value.is_null() {
        return None;
    }
    core::str::from_utf8(unsafe { CStr::from_ptr(value) }.to_bytes())
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn complain(message: &[u8]) {
    let _ = Sys::write(2, message);
}
