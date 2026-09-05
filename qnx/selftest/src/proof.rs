//! HARNESS=6 proof steps.
//!
//! Each step checks the meaning of a result, not only that the call
//! returned: the signal number a handler sees, the exit code inside a wait
//! status, the family inside a returned address. They exercise exactly the
//! places where relibc's numbering and layouts (Linux's) are translated into
//! QNX's, so a missing or wrong translation fails a step by name.

use core::{
    ffi::{c_char, c_int, c_ulong, c_void},
    ptr,
    sync::atomic::{AtomicI64, AtomicU32, Ordering},
};

use super::{
    _exit, ARMED, CLOCK_REALTIME, MAP_ANONYMOUS, MAP_PRIVATE, O_RDONLY, PROT_READ, PROT_WRITE,
    SEEK_SET, SIGUSR1, Timespec, begin, clock_gettime, close, errno, fork, getpid,
    install_fault_handlers, jmp_buf, lseek, malloc, mmap, munmap, nanosleep, open, pthread_create,
    pthread_join, raise, read, report, report_fault, report_i, setjmp, sigprocmask, skip, waitpid,
    write, write_num, write_str,
};

// relibc's ABI, which is what a program built against it speaks.
const SIGKILL: c_int = 9;
const SIGUSR2: c_int = 12;
const SIGPIPE: c_int = 13;
const SIGALRM: c_int = 14;
const SA_SIGINFO: c_int = 4;
const SIG_BLOCK: c_int = 0;
const SIG_UNBLOCK: c_int = 1;
const SI_USER: c_int = 0;
const SI_QUEUE: c_int = -1;
const ECHILD: c_int = 10;
const EAGAIN: c_int = 11;
const ENOTTY: c_int = 25;
const EPIPE: c_int = 32;
const ENAMETOOLONG: c_int = 36;
const ENOSYS: c_int = 38;
const ETIMEDOUT: c_int = 110;
const ECONNREFUSED: c_int = 111;
const CLOCK_MONOTONIC: c_int = 1;
const CLOCK_PROCESS_CPUTIME_ID: c_int = 2;
const TIMER_ABSTIME: c_int = 1;
const SIGEV_NONE: c_int = 1;
const ITIMER_REAL: c_int = 0;
const WNOHANG: c_int = 1;
const O_RDWR: c_int = 0x0002;
const O_CREAT: c_int = 0x0040;
const O_EXCL: c_int = 0x0080;
const O_TRUNC: c_int = 0x0200;
const SEEK_END: c_int = 2;
const F_GETLK: c_int = 5;
const F_SETLK: c_int = 6;
const F_WRLCK: i16 = 1;
const F_UNLCK: i16 = 2;
const LOCK_EX: c_int = 2;
const LOCK_NB: c_int = 4;
const LOCK_UN: c_int = 8;
const SS_DISABLE: c_int = 2;
const AF_INET: u16 = 2;
const AF_INET6: c_int = 10;
const SOCK_STREAM: c_int = 1;
const SOCK_DGRAM: c_int = 2;
const SOL_SOCKET: c_int = 1;
const SO_REUSEADDR: c_int = 2;
const SO_TYPE: c_int = 3;
const SO_ERROR: c_int = 4;
const SO_RCVTIMEO: c_int = 20;
const IPPROTO_TCP: c_int = 6;
const TCP_NODELAY: c_int = 1;
const MSG_DONTWAIT: c_int = 0x40;
const MSG_NOSIGNAL: c_int = 0x4000;
const TCSANOW: c_int = 0;
const ICANON: u32 = 0o2;
const TIOCGWINSZ: c_ulong = 0x5413;

/// relibc's `struct sigaction`.
#[repr(C)]
struct Sigaction {
    handler: usize,
    flags: c_int,
    restorer: usize,
    mask: u64,
}

/// relibc's `siginfo_t`.
#[repr(C)]
#[derive(Default)]
struct Siginfo {
    signo: c_int,
    errno: c_int,
    code: c_int,
    pid: c_int,
    uid: c_int,
    addr: usize,
    status: c_int,
    value: usize,
}

#[repr(C)]
struct Flock {
    l_type: i16,
    l_whence: i16,
    l_start: i64,
    l_len: i64,
    l_pid: c_int,
}

#[repr(C)]
#[derive(Default)]
struct Timeval {
    sec: i64,
    usec: i64,
}

#[repr(C)]
#[derive(Default)]
struct Itimerspec {
    interval: Timespec,
    value: Timespec,
}

#[repr(C)]
struct Itimerval {
    interval: Timeval,
    value: Timeval,
}

#[repr(C)]
struct Sigevent {
    value: usize,
    signo: c_int,
    notify: c_int,
    function: usize,
    attributes: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SockaddrIn {
    family: u16,
    port: u16,
    addr: u32,
    zero: [u8; 8],
}

#[repr(C)]
struct StackT {
    sp: *mut c_void,
    flags: c_int,
    size: usize,
}

#[repr(C)]
struct Termios {
    iflag: u32,
    oflag: u32,
    cflag: u32,
    lflag: u32,
    line: u8,
    cc: [u8; 32],
    ispeed: u32,
    ospeed: u32,
}

#[repr(C)]
#[derive(Default)]
struct Winsize {
    row: u16,
    col: u16,
    x: u16,
    y: u16,
}

unsafe extern "C" {
    fn sigaction(sig: c_int, act: *const Sigaction, old: *mut Sigaction) -> c_int;
    fn sigpending(set: *mut u64) -> c_int;
    fn sigtimedwait(set: *const u64, info: *mut Siginfo, timeout: *const Timespec) -> c_int;
    fn sigqueue(pid: c_int, sig: c_int, value: usize) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn sigaltstack(ss: *const StackT, old: *mut StackT) -> c_int;
    fn timer_create(clock: c_int, ev: *mut Sigevent, id: *mut usize) -> c_int;
    fn timer_settime(id: usize, flags: c_int, new: *const Itimerspec, old: *mut Itimerspec) -> c_int;
    fn timer_gettime(id: usize, cur: *mut Itimerspec) -> c_int;
    fn timer_delete(id: usize) -> c_int;
    fn setitimer(which: c_int, new: *const Itimerval, old: *mut Itimerval) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn flock(fd: c_int, op: c_int) -> c_int;
    fn ftruncate(fd: c_int, len: i64) -> c_int;
    fn unlink(path: *const c_char) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const SockaddrIn, len: u32) -> c_int;
    fn listen(fd: c_int, backlog: c_int) -> c_int;
    fn connect(fd: c_int, addr: *const SockaddrIn, len: u32) -> c_int;
    fn accept(fd: c_int, addr: *mut SockaddrIn, len: *mut u32) -> c_int;
    fn getsockname(fd: c_int, addr: *mut SockaddrIn, len: *mut u32) -> c_int;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, val: *const c_void, len: u32) -> c_int;
    fn getsockopt(fd: c_int, level: c_int, name: c_int, val: *mut c_void, len: *mut u32) -> c_int;
    fn send(fd: c_int, buf: *const u8, n: usize, flags: c_int) -> isize;
    fn recv(fd: c_int, buf: *mut u8, n: usize, flags: c_int) -> isize;
    fn sendto(fd: c_int, buf: *const u8, n: usize, flags: c_int, to: *const SockaddrIn, len: u32) -> isize;
    fn recvfrom(fd: c_int, buf: *mut u8, n: usize, flags: c_int, from: *mut SockaddrIn, len: *mut u32) -> isize;
    fn isatty(fd: c_int) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, arg: *mut c_void) -> c_int;
    fn tcgetattr(fd: c_int, t: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, action: c_int, t: *const Termios) -> c_int;
    fn posix_spawn(
        pid: *mut c_int,
        path: *const c_char,
        actions: *const c_void,
        attr: *const c_void,
        argv: *const *const c_char,
        envp: *const *const c_char,
    ) -> c_int;
    static environ: *const *const c_char;

    fn sem_init(sem: *mut SemT, pshared: c_int, value: u32) -> c_int;
    fn sem_destroy(sem: *mut SemT) -> c_int;
    fn sem_timedwait(sem: *mut SemT, deadline: *const Timespec) -> c_int;
    fn sem_post(sem: *mut SemT) -> c_int;
    fn sem_open(name: *const c_char, oflag: c_int, ...) -> *mut SemT;
    fn sem_close(sem: *mut SemT) -> c_int;
    fn sem_unlink(name: *const c_char) -> c_int;
    fn pthread_mutex_init(m: *mut PthreadMutex, attr: *const c_void) -> c_int;
    fn pthread_mutex_lock(m: *mut PthreadMutex) -> c_int;
    fn pthread_mutex_unlock(m: *mut PthreadMutex) -> c_int;
    fn pthread_mutex_destroy(m: *mut PthreadMutex) -> c_int;
    fn pthread_cond_init(c: *mut PthreadCond, attr: *const c_void) -> c_int;
    fn pthread_cond_timedwait(
        c: *mut PthreadCond,
        m: *mut PthreadMutex,
        deadline: *const Timespec,
    ) -> c_int;
    fn pthread_cond_destroy(c: *mut PthreadCond) -> c_int;
    fn pthread_attr_init(a: *mut PthreadAttr) -> c_int;
    fn pthread_attr_setstacksize(a: *mut PthreadAttr, size: usize) -> c_int;
    fn pthread_attr_getstack(
        a: *const PthreadAttr,
        addr: *mut *mut c_void,
        size: *mut usize,
    ) -> c_int;
    fn pthread_attr_destroy(a: *mut PthreadAttr) -> c_int;
    fn pthread_getattr_np(thread: usize, a: *mut PthreadAttr) -> c_int;
    fn pthread_self() -> usize;
    fn clock_nanosleep(clock: c_int, flags: c_int, rq: *const Timespec, rm: *mut Timespec)
    -> c_int;
    fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
    fn shm_open(name: *const c_char, oflag: c_int, mode: u32) -> c_int;
    fn shm_unlink(name: *const c_char) -> c_int;
}

// relibc's semaphore and pthread types, opaque, at its sizes and alignments.
#[repr(C, align(8))]
struct SemT([u8; 8]);
#[repr(C, align(4))]
struct PthreadMutex([u8; 12]);
#[repr(C, align(4))]
struct PthreadCond([u8; 8]);
#[repr(C, align(8))]
struct PthreadAttr([u8; 32]);

static R: AtomicI64 = AtomicI64::new(0);
static R2: AtomicI64 = AtomicI64::new(0);
static SIG_SEEN: AtomicI64 = AtomicI64::new(0);
static SI_SIGNO: AtomicI64 = AtomicI64::new(0);
static SI_CODE: AtomicI64 = AtomicI64::new(0);
static SI_PID: AtomicI64 = AtomicI64::new(0);
static SI_VALUE: AtomicI64 = AtomicI64::new(0);
static ALARMS: AtomicU32 = AtomicU32::new(0);
static PIPES: AtomicU32 = AtomicU32::new(0);
static USR1: AtomicU32 = AtomicU32::new(0);
static ARGV0: AtomicI64 = AtomicI64::new(0);
static STACK_SEEN: AtomicI64 = AtomicI64::new(0);
static STACK_INSIDE: AtomicI64 = AtomicI64::new(0);

/// The child side of the spawn steps; `None` when not started as a child.
pub fn child_mode(argc: c_int, argv: *const *const c_char) -> Option<c_int> {
    if argc < 2 || argv.is_null() {
        if argc >= 1 && !argv.is_null() {
            ARGV0.store(unsafe { *argv } as i64, Ordering::SeqCst);
        }
        return None;
    }
    let arg = unsafe { core::ffi::CStr::from_ptr(*argv.add(1)) }.to_bytes();
    match arg {
        b"--child-exit" => Some(7),
        b"--child-mask" => {
            let mut cur = 0u64;
            unsafe { sigprocmask(SIG_BLOCK, ptr::null(), &mut cur) };
            Some(if cur & bit(SIGUSR2) != 0 { 0 } else { 3 })
        }
        _ => None,
    }
}

fn bit(sig: c_int) -> u64 {
    1u64 << (sig - 1)
}

fn monotonic_ns() -> i64 {
    let mut t = Timespec::default();
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut t) };
    t.tv_sec * 1_000_000_000 + t.tv_nsec
}

fn sleep_ms(ms: i64) {
    let t = Timespec {
        tv_sec: ms / 1000,
        tv_nsec: (ms % 1000) * 1_000_000,
    };
    unsafe { nanosleep(&t, ptr::null_mut()) };
}

extern "C" fn on_siginfo(sig: c_int, info: *mut Siginfo, _ctx: *mut c_void) {
    SIG_SEEN.store(i64::from(sig), Ordering::SeqCst);
    if let Some(info) = unsafe { info.as_ref() } {
        SI_SIGNO.store(i64::from(info.signo), Ordering::SeqCst);
        SI_CODE.store(i64::from(info.code), Ordering::SeqCst);
        SI_PID.store(i64::from(info.pid), Ordering::SeqCst);
        SI_VALUE.store(info.value as i64, Ordering::SeqCst);
    }
}

extern "C" fn on_alarm(_sig: c_int) {
    ALARMS.fetch_add(1, Ordering::SeqCst);
}

extern "C" fn on_pipe(_sig: c_int) {
    PIPES.fetch_add(1, Ordering::SeqCst);
}

extern "C" fn on_usr1(_sig: c_int) {
    USR1.fetch_add(1, Ordering::SeqCst);
}

fn handle(sig: c_int, handler: usize, flags: c_int) -> c_int {
    let act = Sigaction {
        handler,
        flags,
        restorer: 0,
        mask: 0,
    };
    unsafe { sigaction(sig, &act, ptr::null_mut()) }
}

/// Forks a child that runs `body` and exits with its result; returns the
/// raw wait status, or -1.
fn in_child(body: fn() -> c_int) -> i64 {
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe { _exit(body()) };
    }
    if pid < 0 {
        return -1;
    }
    let mut status = 0;
    if unsafe { waitpid(pid, &mut status, 0) } != pid {
        return -1;
    }
    i64::from(status)
}

fn realtime_in(ms: i64) -> Timespec {
    let mut t = Timespec::default();
    unsafe { clock_gettime(CLOCK_REALTIME, &mut t) };
    let nsec = t.tv_nsec + ms * 1_000_000;
    Timespec {
        tv_sec: t.tv_sec + nsec / 1_000_000_000,
        tv_nsec: nsec % 1_000_000_000,
    }
}

fn exited(status: i64, code: i64) -> bool {
    status >= 0 && status & 0x7f == 0 && (status >> 8) & 0xff == code
}

/// Guarded step whose body stores its verdict inputs into `R`/`R2`.
macro_rules! proof {
    ($name:expr, $body:block, |$a:ident, $b:ident| $ok:expr, $label:expr) => {{
        begin($name);
        R.store(0, Ordering::SeqCst);
        R2.store(0, Ordering::SeqCst);
        if unsafe { setjmp(jmp_buf()) } == 0 {
            ARMED.store(1, Ordering::SeqCst);
            $body
            ARMED.store(0, Ordering::SeqCst);
            let $a = R.load(Ordering::SeqCst);
            let $b = R2.load(Ordering::SeqCst);
            report_i($name, $ok, $label, $a);
        } else {
            ARMED.store(0, Ordering::SeqCst);
            install_fault_handlers();
            report_fault($name);
        }
    }};
}

fn set(a: i64, b: i64) {
    R.store(a, Ordering::SeqCst);
    R2.store(b, Ordering::SeqCst);
}

pub fn run() {
    write_str(1, "\n-- proof steps (HARNESS=6) --\n");
    errno_and_clocks();
    signals();
    waits();
    timers();
    files();
    alt_stack();
    sockets();
    terminal();
    spawn();
    sync_and_memory();
}

fn errno_and_clocks() {
    proof!("errno is renumbered (ENAMETOOLONG)", {
        let mut path = [b'a'; 320];
        path[0] = b'/';
        path[319] = 0;
        let rc = unsafe { open(path.as_ptr().cast(), O_RDONLY) };
        set(i64::from(errno()), i64::from(rc));
    }, |e, rc| rc == -1 && e == i64::from(ENAMETOOLONG), "errno");

    proof!("CLOCK_MONOTONIC is renumbered", {
        let a = monotonic_ns();
        sleep_ms(2);
        let b = monotonic_ns();
        set(b - a, 0);
    }, |d, _x| d >= 1_000_000 && d < 1_000_000_000, "elapsed_ns");

    proof!("CLOCK_PROCESS_CPUTIME_ID", {
        let mut t = Timespec::default();
        set(i64::from(unsafe { clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &mut t) }), 0);
    }, |rc, _x| rc == 0, "rc");
}

fn signals() {
    proof!("SA_SIGINFO handler gets relibc signo and siginfo", {
        SIG_SEEN.store(0, Ordering::SeqCst);
        let rc = handle(SIGUSR2, on_siginfo as *const () as usize, SA_SIGINFO);
        unsafe { raise(SIGUSR2) };
        let pid = i64::from(unsafe { getpid() });
        let ok = rc == 0
            && SIG_SEEN.load(Ordering::SeqCst) == i64::from(SIGUSR2)
            && SI_SIGNO.load(Ordering::SeqCst) == i64::from(SIGUSR2)
            && SI_CODE.load(Ordering::SeqCst) == i64::from(SI_USER)
            && SI_PID.load(Ordering::SeqCst) == pid;
        set(SIG_SEEN.load(Ordering::SeqCst), i64::from(ok));
    }, |_sig, ok| ok == 1, "signo");

    proof!("sigqueue carries SI_QUEUE and the value", {
        SIG_SEEN.store(0, Ordering::SeqCst);
        let rc = unsafe { sigqueue(getpid(), SIGUSR2, 77) };
        let ok = rc == 0
            && SI_CODE.load(Ordering::SeqCst) == i64::from(SI_QUEUE)
            && SI_VALUE.load(Ordering::SeqCst) & 0xffff_ffff == 77;
        set(SI_CODE.load(Ordering::SeqCst), i64::from(ok));
    }, |_code, ok| ok == 1, "si_code");

    proof!("blocked signal stays pending, runs on unblock", {
        USR1.store(0, Ordering::SeqCst);
        handle(SIGUSR1, on_usr1 as *const () as usize, 0);
        let set1 = bit(SIGUSR1);
        unsafe { sigprocmask(SIG_BLOCK, &set1, ptr::null_mut()) };
        unsafe { raise(SIGUSR1) };
        let before = USR1.load(Ordering::SeqCst);
        let mut pending = 0u64;
        unsafe { sigpending(&mut pending) };
        let mut cur = 0u64;
        unsafe { sigprocmask(SIG_BLOCK, ptr::null(), &mut cur) };
        unsafe { sigprocmask(SIG_UNBLOCK, &set1, ptr::null_mut()) };
        let after = USR1.load(Ordering::SeqCst);
        let ok = before == 0 && pending & set1 != 0 && cur & set1 != 0 && after == 1;
        set(pending as i64, i64::from(ok));
    }, |_p, ok| ok == 1, "pending");

    proof!("sigtimedwait honours its timeout", {
        let s = bit(SIGUSR2);
        unsafe { sigprocmask(SIG_BLOCK, &s, ptr::null_mut()) };
        let t0 = monotonic_ns();
        let wait = Timespec {
            tv_sec: 0,
            tv_nsec: 50_000_000,
        };
        let mut info = Siginfo::default();
        let rc = unsafe { sigtimedwait(&s, &mut info, &wait) };
        let e = errno();
        let dt = monotonic_ns() - t0;
        set(dt, i64::from(rc == -1 && e == EAGAIN && dt >= 40_000_000 && dt < 1_000_000_000));
    }, |_dt, ok| ok == 1, "elapsed_ns");

    proof!("sigtimedwait returns relibc signo and siginfo", {
        let s = bit(SIGUSR2);
        unsafe { raise(SIGUSR2) };
        let wait = Timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        let mut info = Siginfo::default();
        let rc = unsafe { sigtimedwait(&s, &mut info, &wait) };
        unsafe { sigprocmask(SIG_UNBLOCK, &s, ptr::null_mut()) };
        set(i64::from(rc), i64::from(info.signo == SIGUSR2));
    }, |rc, ok| rc == i64::from(SIGUSR2) && ok == 1, "rc");
}

fn waits() {
    proof!("waitpid: exit status 42", {
        set(in_child(|| 42), 0);
    }, |st, _x| exited(st, 42), "status");

    proof!("waitpid: killed by SIGKILL", {
        set(in_child(|| {
            unsafe { kill(getpid(), SIGKILL) };
            1
        }), 0);
    }, |st, _x| st & 0x7f == i64::from(SIGKILL) && (st >> 8) & 0xff == 0, "status");

    proof!("waitpid: WNOHANG before the child exits", {
        let pid = unsafe { fork() };
        if pid == 0 {
            sleep_ms(300);
            unsafe { _exit(0) };
        }
        let mut st = 0;
        let early = unsafe { waitpid(pid, &mut st, WNOHANG) };
        let late = unsafe { waitpid(pid, &mut st, 0) };
        set(i64::from(early), i64::from(pid > 0 && late == pid && exited(i64::from(st), 0)));
    }, |early, ok| early == 0 && ok == 1, "early");

    proof!("waitpid: ECHILD without children", {
        let mut st = 0;
        let rc = unsafe { waitpid(-1, &mut st, 0) };
        set(i64::from(errno()), i64::from(rc));
    }, |e, rc| rc == -1 && e == i64::from(ECHILD), "errno");
}

fn timers() {
    proof!("timer_settime TIMER_ABSTIME", {
        let mut ev = Sigevent {
            value: 0,
            signo: 0,
            notify: SIGEV_NONE,
            function: 0,
            attributes: 0,
        };
        let mut id = 0usize;
        let rc = unsafe { timer_create(CLOCK_MONOTONIC, &mut ev, &mut id) };
        let mut now = Timespec::default();
        unsafe { clock_gettime(CLOCK_MONOTONIC, &mut now) };
        let mut its = Itimerspec::default();
        its.value.tv_sec = now.tv_sec;
        its.value.tv_nsec = now.tv_nsec + 200_000_000;
        if its.value.tv_nsec >= 1_000_000_000 {
            its.value.tv_sec += 1;
            its.value.tv_nsec -= 1_000_000_000;
        }
        let rc2 = unsafe { timer_settime(id, TIMER_ABSTIME, &its, ptr::null_mut()) };
        let mut cur = Itimerspec::default();
        let rc3 = unsafe { timer_gettime(id, &mut cur) };
        unsafe { timer_delete(id) };
        let left = cur.value.tv_sec * 1_000_000_000 + cur.value.tv_nsec;
        set(left, i64::from(rc == 0 && rc2 == 0 && rc3 == 0));
    }, |left, ok| ok == 1 && left > 0 && left <= 250_000_000, "left_ns");

    proof!("setitimer delivers SIGALRM", {
        ALARMS.store(0, Ordering::SeqCst);
        handle(SIGALRM, on_alarm as *const () as usize, 0);
        let it = Itimerval {
            interval: Timeval::default(),
            value: Timeval { sec: 0, usec: 30_000 },
        };
        let rc = unsafe { setitimer(ITIMER_REAL, &it, ptr::null_mut()) };
        let mut waited = 0;
        while ALARMS.load(Ordering::SeqCst) == 0 && waited < 1000 {
            sleep_ms(10);
            waited += 10;
        }
        set(i64::from(ALARMS.load(Ordering::SeqCst)), i64::from(rc));
    }, |n, rc| rc == 0 && n == 1, "alarms");
}

static LOCK_FD: AtomicI64 = AtomicI64::new(-1);
static PARENT: AtomicI64 = AtomicI64::new(0);

/// A scratch file, in /tmp or else /dev/shmem.
fn scratch(name: &[u8]) -> (c_int, [u8; 64]) {
    for dir in [&b"/tmp/"[..], &b"/dev/shmem/"[..]] {
        let mut path = [0u8; 64];
        path[..dir.len()].copy_from_slice(dir);
        path[dir.len()..dir.len() + name.len()].copy_from_slice(name);
        let fd = unsafe { open(path.as_ptr().cast(), O_RDWR | O_CREAT | O_TRUNC, 0o600) };
        if fd >= 0 {
            return (fd, path);
        }
    }
    (-1, [0; 64])
}

fn files() {
    proof!("pipe carries data", {
        let mut fds = [-1; 2];
        let rc = unsafe { pipe(fds.as_mut_ptr()) };
        let w = unsafe { write(fds[1], b"abc".as_ptr(), 3) };
        let mut buf = [0u8; 8];
        let r = unsafe { read(fds[0], buf.as_mut_ptr(), buf.len()) };
        unsafe {
            close(fds[0]);
            close(fds[1]);
        }
        set(r as i64, i64::from(rc == 0 && w == 3 && &buf[..3] == b"abc"));
    }, |r, ok| r == 3 && ok == 1, "n");

    let (fd, path) = scratch(b"relibc-selftest.lock");
    if fd < 0 {
        skip("fcntl F_GETLK sees another process's lock", "no writable /tmp or /dev/shmem");
        skip("ftruncate grows and shrinks", "no writable /tmp or /dev/shmem");
        skip("flock excludes another open file", "no writable /tmp or /dev/shmem");
        return;
    }
    LOCK_FD.store(i64::from(fd), Ordering::SeqCst);
    PARENT.store(i64::from(unsafe { getpid() }), Ordering::SeqCst);

    proof!("fcntl F_GETLK sees another process's lock", {
        let lock = Flock {
            l_type: F_WRLCK,
            l_whence: SEEK_SET as i16,
            l_start: 0,
            l_len: 10,
            l_pid: 0,
        };
        let rc = unsafe { fcntl(fd, F_SETLK, ptr::from_ref(&lock)) };
        let st = in_child(|| {
            let mut probe = Flock {
                l_type: F_WRLCK,
                l_whence: SEEK_SET as i16,
                l_start: 0,
                l_len: 10,
                l_pid: 0,
            };
            let fd = LOCK_FD.load(Ordering::SeqCst) as c_int;
            if unsafe { fcntl(fd, F_GETLK, ptr::from_mut(&mut probe)) } != 0 {
                return 10;
            }
            if probe.l_type != F_WRLCK {
                return 20 + probe.l_type as c_int;
            }
            if i64::from(probe.l_pid) != PARENT.load(Ordering::SeqCst) {
                return 30;
            }
            0
        });
        let unlock = Flock {
            l_type: F_UNLCK,
            l_whence: SEEK_SET as i16,
            l_start: 0,
            l_len: 10,
            l_pid: 0,
        };
        unsafe { fcntl(fd, F_SETLK, ptr::from_ref(&unlock)) };
        set(st, i64::from(rc));
    }, |st, rc| rc == 0 && exited(st, 0), "status");

    proof!("ftruncate grows and shrinks", {
        let a = unsafe { ftruncate(fd, 4096) };
        let size_a = unsafe { lseek(fd, 0, SEEK_END) };
        let b = unsafe { ftruncate(fd, 100) };
        let size_b = unsafe { lseek(fd, 0, SEEK_END) };
        set(size_a, i64::from(a == 0 && b == 0 && size_b == 100));
    }, |size, ok| size == 4096 && ok == 1, "size");

    proof!("flock excludes another open file", {
        let rc = unsafe { flock(fd, LOCK_EX) };
        static PATH: AtomicI64 = AtomicI64::new(0);
        PATH.store(path.as_ptr() as i64, Ordering::SeqCst);
        let st = in_child(|| {
            let path = PATH.load(Ordering::SeqCst) as *const c_char;
            let other = unsafe { open(path, O_RDWR) };
            if other < 0 {
                return 10;
            }
            let rc = unsafe { flock(other, LOCK_EX | LOCK_NB) };
            if rc == 0 {
                return 20;
            }
            if errno() != EAGAIN {
                return 30;
            }
            0
        });
        unsafe { flock(fd, LOCK_UN) };
        set(st, i64::from(rc));
    }, |st, rc| rc == 0 && exited(st, 0), "status");

    unsafe {
        close(fd);
        unlink(path.as_ptr().cast());
    }
}

fn alt_stack() {
    proof!("sigaltstack", {
        let size = 65536;
        let sp = unsafe { malloc(size) };
        let ss = StackT { sp, flags: 0, size };
        let rc = unsafe { sigaltstack(&ss, ptr::null_mut()) };
        let e = errno();
        let mut cur = StackT {
            sp: ptr::null_mut(),
            flags: 0,
            size: 0,
        };
        let disable = StackT {
            sp: ptr::null_mut(),
            flags: SS_DISABLE,
            size: 0,
        };
        let rc_off = unsafe { sigaltstack(&disable, &mut cur) };
        let ok = if cfg!(target_env = "nto80") {
            rc == 0 && rc_off == 0 && cur.sp == sp && cur.size == size
        } else {
            // 7.1 refuses a stack it cannot prove the kernel will use.
            rc == -1 && e == ENOSYS && rc_off == 0
        };
        set(i64::from(rc), i64::from(ok));
    }, |_rc, ok| ok == 1, "rc");
}

fn loopback(port: u16) -> SockaddrIn {
    SockaddrIn {
        family: AF_INET,
        port,
        addr: u32::from_ne_bytes([127, 0, 0, 1]),
        zero: [0; 8],
    }
}

fn name_of(fd: c_int) -> (SockaddrIn, u32) {
    let mut sa = SockaddrIn::default();
    let mut len = 16u32;
    unsafe { getsockname(fd, &mut sa, &mut len) };
    (sa, len)
}

fn int_opt(fd: c_int, level: c_int, name: c_int) -> c_int {
    let mut v: c_int = -1;
    let mut len = 4u32;
    unsafe { getsockopt(fd, level, name, ptr::from_mut(&mut v).cast(), &mut len) };
    v
}

fn sockets() {
    let s = unsafe { socket(c_int::from(AF_INET), SOCK_STREAM, 0) };
    if s < 0 {
        write_str(1, "          socket(AF_INET) errno=");
        write_num(1, i64::from(errno()));
        write_str(1, "\n");
        skip("sockets", "no network stack reachable");
        return;
    }
    let one: c_int = 1;
    proof!("SO_REUSEADDR set and read back", {
        let rc = unsafe { setsockopt(s, SOL_SOCKET, SO_REUSEADDR, ptr::from_ref(&one).cast(), 4) };
        set(i64::from(int_opt(s, SOL_SOCKET, SO_REUSEADDR)), i64::from(rc));
    }, |v, rc| rc == 0 && v != 0, "value");

    proof!("bind + getsockname: family and port", {
        let rc = unsafe { bind(s, &loopback(0), 16) };
        let (sa, len) = name_of(s);
        set(i64::from(sa.family), i64::from(rc == 0 && len == 16 && sa.port != 0));
    }, |family, ok| family == i64::from(AF_INET) && ok == 1, "family");

    let (addr, _) = name_of(s);
    unsafe { listen(s, 4) };
    let c = unsafe { socket(c_int::from(AF_INET), SOCK_STREAM, 0) };
    proof!("connect + accept: peer family", {
        let rc = unsafe { connect(c, &addr, 16) };
        let mut peer = SockaddrIn::default();
        let mut plen = 16u32;
        let a = unsafe { accept(s, &mut peer, &mut plen) };
        R2.store(i64::from(a), Ordering::SeqCst);
        set(i64::from(peer.family), i64::from(rc == 0 && a >= 0 && plen == 16));
        R.store(i64::from(peer.family) | (i64::from(a) << 16), Ordering::SeqCst);
    }, |v, ok| v & 0xffff == i64::from(AF_INET) && ok == 1, "family|fd<<16");
    let a = (R.load(Ordering::SeqCst) >> 16) as c_int;

    proof!("SO_TYPE and SO_ERROR", {
        set(i64::from(int_opt(c, SOL_SOCKET, SO_TYPE)), i64::from(int_opt(c, SOL_SOCKET, SO_ERROR)));
    }, |t, e| t == i64::from(SOCK_STREAM) && e == 0, "type");

    proof!("TCP_NODELAY", {
        let rc = unsafe { setsockopt(c, IPPROTO_TCP, TCP_NODELAY, ptr::from_ref(&one).cast(), 4) };
        set(i64::from(int_opt(c, IPPROTO_TCP, TCP_NODELAY)), i64::from(rc));
    }, |v, rc| rc == 0 && v != 0, "value");

    proof!("SO_RCVTIMEO round trip (timeval)", {
        let tv = Timeval { sec: 0, usec: 150_000 };
        let rc = unsafe { setsockopt(a, SOL_SOCKET, SO_RCVTIMEO, ptr::from_ref(&tv).cast(), 16) };
        let mut back = Timeval::default();
        let mut len = 16u32;
        unsafe { getsockopt(a, SOL_SOCKET, SO_RCVTIMEO, ptr::from_mut(&mut back).cast(), &mut len) };
        set(back.usec, i64::from(rc == 0 && back.sec == 0));
    }, |usec, ok| usec == 150_000 && ok == 1, "usec");

    proof!("send/recv over loopback", {
        let n = unsafe { send(c, b"ping".as_ptr(), 4, 0) };
        let mut buf = [0u8; 8];
        let m = unsafe { recv(a, buf.as_mut_ptr(), 4, 0) };
        set(m as i64, i64::from(n == 4 && &buf[..4] == b"ping"));
    }, |m, ok| m == 4 && ok == 1, "n");

    proof!("MSG_DONTWAIT on an empty socket", {
        let mut buf = [0u8; 8];
        let m = unsafe { recv(a, buf.as_mut_ptr(), 4, MSG_DONTWAIT) };
        set(i64::from(errno()), m as i64);
    }, |e, m| m == -1 && e == i64::from(EAGAIN), "errno");

    proof!("MSG_NOSIGNAL: EPIPE without SIGPIPE", {
        PIPES.store(0, Ordering::SeqCst);
        handle(SIGPIPE, on_pipe as *const () as usize, 0);
        unsafe { close(a) };
        sleep_ms(20);
        let mut e = 0;
        for _ in 0..8 {
            if unsafe { send(c, b"x".as_ptr(), 1, MSG_NOSIGNAL) } < 0 {
                e = errno();
                break;
            }
            sleep_ms(10);
        }
        set(i64::from(e), i64::from(PIPES.load(Ordering::SeqCst)));
    }, |e, pipes| e == i64::from(EPIPE) && pipes == 0, "errno");
    unsafe {
        close(c);
        close(s);
    }

    proof!("UDP sendto/recvfrom: sender address", {
        let u1 = unsafe { socket(c_int::from(AF_INET), SOCK_DGRAM, 0) };
        let u2 = unsafe { socket(c_int::from(AF_INET), SOCK_DGRAM, 0) };
        unsafe {
            bind(u1, &loopback(0), 16);
            bind(u2, &loopback(0), 16);
        }
        let (to, _) = name_of(u1);
        let (me, _) = name_of(u2);
        let n = unsafe { sendto(u2, b"dgram".as_ptr(), 5, 0, &to, 16) };
        let mut from = SockaddrIn::default();
        let mut flen = 16u32;
        let mut buf = [0u8; 16];
        let m = unsafe { recvfrom(u1, buf.as_mut_ptr(), buf.len(), 0, &mut from, &mut flen) };
        unsafe {
            close(u1);
            close(u2);
        }
        set(
            m as i64,
            i64::from(n == 5 && from.family == AF_INET && from.port == me.port && flen == 16),
        );
    }, |m, ok| m == 5 && ok == 1, "n");

    proof!("connect to a closed port: ECONNREFUSED", {
        let t = unsafe { socket(c_int::from(AF_INET), SOCK_STREAM, 0) };
        unsafe { bind(t, &loopback(0), 16) };
        let (dead, _) = name_of(t);
        unsafe { close(t) };
        let d = unsafe { socket(c_int::from(AF_INET), SOCK_STREAM, 0) };
        let rc = unsafe { connect(d, &dead, 16) };
        let e = errno();
        unsafe { close(d) };
        set(i64::from(e), i64::from(rc));
    }, |e, rc| rc == -1 && e == i64::from(ECONNREFUSED), "errno");

    // Proves the AF_INET6 number only where the stack has IPv6.
    let s6 = unsafe { socket(AF_INET6, SOCK_STREAM, 0) };
    if s6 >= 0 {
        report("socket(AF_INET6)", true);
        unsafe { close(s6) };
    } else {
        write_str(1, "          socket(AF_INET6) errno=");
        write_num(1, i64::from(errno()));
        write_str(1, "  (no IPv6 in this stack, or a wrong AF_INET6)\n");
    }
}

fn terminal() {
    let tty = unsafe { isatty(1) };
    if tty != 1 {
        let e = errno();
        report_i("isatty on a non-terminal: ENOTTY", e == ENOTTY, "errno", i64::from(e));
        return;
    }
    proof!("tcgetattr/tcsetattr round trip", {
        let mut t = Termios {
            iflag: 0,
            oflag: 0,
            cflag: 0,
            lflag: 0,
            line: 0,
            cc: [0; 32],
            ispeed: 0,
            ospeed: 0,
        };
        let rc = unsafe { tcgetattr(1, &mut t) };
        let rc2 = unsafe { tcsetattr(1, TCSANOW, &t) };
        set(i64::from(t.lflag & ICANON), i64::from(rc == 0 && rc2 == 0 && t.ospeed != 0));
    }, |_icanon, ok| ok == 1, "icanon");

    proof!("TIOCGWINSZ", {
        let mut ws = Winsize::default();
        let rc = unsafe { ioctl(1, TIOCGWINSZ, ptr::from_mut(&mut ws).cast()) };
        set(i64::from(ws.col), i64::from(rc));
    }, |_cols, rc| rc == 0, "cols");
}

fn spawn() {
    if cfg!(target_env = "nto80") {
        skip("posix_spawn", "8.0 spawn and exec are not implemented yet (ENOSYS)");
        return;
    }
    let argv0 = ARGV0.load(Ordering::SeqCst) as *const c_char;
    if argv0.is_null() {
        skip("posix_spawn", "no argv[0]");
        return;
    }
    proof!("posix_spawn: child exit status", {
        let argv = [argv0, c"--child-exit".as_ptr(), ptr::null()];
        let mut pid = 0;
        let rc = unsafe {
            posix_spawn(&mut pid, argv0, ptr::null(), ptr::null(), argv.as_ptr(), environ)
        };
        let mut st = 0;
        let w = unsafe { waitpid(pid, &mut st, 0) };
        set(i64::from(st), i64::from(rc == 0 && w == pid));
    }, |st, ok| ok == 1 && exited(st, 7), "status");

    proof!("posix_spawn: child inherits the signal mask", {
        let s = bit(SIGUSR2);
        unsafe { sigprocmask(SIG_BLOCK, &s, ptr::null_mut()) };
        let argv = [argv0, c"--child-mask".as_ptr(), ptr::null()];
        let mut pid = 0;
        let rc = unsafe {
            posix_spawn(&mut pid, argv0, ptr::null(), ptr::null(), argv.as_ptr(), environ)
        };
        let mut st = 0;
        let w = unsafe { waitpid(pid, &mut st, 0) };
        unsafe { sigprocmask(SIG_UNBLOCK, &s, ptr::null_mut()) };
        set(i64::from(st), i64::from(rc == 0 && w == pid));
    }, |st, ok| ok == 1 && exited(st, 0), "status");
}

/// Reports the stack the calling thread runs on, as `pthread_getattr_np`
/// describes it, and whether a local of the thread lies inside it.
extern "C" fn stack_probe(_: *mut c_void) -> *mut c_void {
    let local = 0u8;
    let here = ptr::from_ref(&local) as usize;
    let mut attr = PthreadAttr([0; 32]);
    let mut addr: *mut c_void = ptr::null_mut();
    let mut size = 0usize;
    if unsafe { pthread_getattr_np(pthread_self(), &mut attr) } == 0
        && unsafe { pthread_attr_getstack(&attr, &mut addr, &mut size) } == 0
    {
        let base = addr as usize;
        STACK_SEEN.store(size as i64, Ordering::SeqCst);
        STACK_INSIDE.store(
            i64::from(here >= base && here - base < size),
            Ordering::SeqCst,
        );
        unsafe { pthread_attr_destroy(&mut attr) };
    }
    ptr::null_mut()
}

/// Timed waits, sleeping until a time, named semaphores, thread stacks and
/// memory.
fn sync_and_memory() {
    proof!(
        "sem_timedwait times out at its deadline",
        {
            let mut sem = SemT([0; 8]);
            unsafe { sem_init(&mut sem, 0, 0) };
            let start = monotonic_ns();
            let deadline = realtime_in(50);
            let rc = unsafe { sem_timedwait(&mut sem, &deadline) };
            let err = errno();
            let ms = (monotonic_ns() - start) / 1_000_000;
            unsafe { sem_destroy(&mut sem) };
            set(ms, i64::from(rc == -1 && err == ETIMEDOUT));
        },
        |ms, ok| ok == 1 && (45..2000).contains(&ms),
        "ms"
    );

    proof!(
        "pthread_cond_timedwait times out at its deadline",
        {
            let mut m = PthreadMutex([0; 12]);
            let mut c = PthreadCond([0; 8]);
            unsafe { pthread_mutex_init(&mut m, ptr::null()) };
            unsafe { pthread_cond_init(&mut c, ptr::null()) };
            let start = monotonic_ns();
            let deadline = realtime_in(50);
            unsafe { pthread_mutex_lock(&mut m) };
            let rc = unsafe { pthread_cond_timedwait(&mut c, &mut m, &deadline) };
            unsafe { pthread_mutex_unlock(&mut m) };
            let ms = (monotonic_ns() - start) / 1_000_000;
            unsafe { pthread_cond_destroy(&mut c) };
            unsafe { pthread_mutex_destroy(&mut m) };
            set(ms, i64::from(rc == ETIMEDOUT));
        },
        |ms, ok| ok == 1 && (45..2000).contains(&ms),
        "ms"
    );

    proof!(
        "clock_nanosleep TIMER_ABSTIME on CLOCK_MONOTONIC",
        {
            let target = monotonic_ns() + 30_000_000;
            let until = Timespec {
                tv_sec: target / 1_000_000_000,
                tv_nsec: target % 1_000_000_000,
            };
            let rc =
                unsafe { clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &until, ptr::null_mut()) };
            set((monotonic_ns() - target) / 1000, i64::from(rc));
        },
        |late_us, rc| rc == 0 && (0..1_000_000).contains(&late_us),
        "us late"
    );

    proof!(
        "named semaphore: another process's post wakes a waiter",
        {
            let name = c"/relibc-selftest-sem";
            unsafe { sem_unlink(name.as_ptr()) };
            let sem = unsafe { sem_open(name.as_ptr(), O_CREAT | O_EXCL, 0o600, 0u32) };
            let mut ok = !sem.is_null();
            let mut ms = -1;
            if ok {
                let pid = unsafe { fork() };
                if pid == 0 {
                    let other = unsafe { sem_open(c"/relibc-selftest-sem".as_ptr(), 0) };
                    sleep_ms(100);
                    let posted = !other.is_null() && unsafe { sem_post(other) } == 0;
                    unsafe { _exit(if posted { 0 } else { 1 }) };
                }
                let start = monotonic_ns();
                let deadline = realtime_in(5000);
                let rc = unsafe { sem_timedwait(sem, &deadline) };
                ms = (monotonic_ns() - start) / 1_000_000;
                let mut status = 0;
                let waited = pid > 0 && unsafe { waitpid(pid, &mut status, 0) } == pid;
                ok = rc == 0 && waited && exited(i64::from(status), 0);
                ok &= unsafe { sem_close(sem) } == 0;
            }
            ok &= unsafe { sem_unlink(name.as_ptr()) } == 0;
            set(ms, i64::from(ok));
        },
        |ms, ok| ok == 1 && (0..2000).contains(&ms),
        "ms"
    );

    proof!(
        "a thread runs on the stack size it asked for",
        {
            const SIZE: usize = 4 << 20;
            let mut attr = PthreadAttr([0; 32]);
            unsafe { pthread_attr_init(&mut attr) };
            unsafe { pthread_attr_setstacksize(&mut attr, SIZE) };
            STACK_SEEN.store(0, Ordering::SeqCst);
            STACK_INSIDE.store(0, Ordering::SeqCst);
            let mut thread = 0usize;
            let rc = unsafe {
                pthread_create(
                    &mut thread,
                    ptr::from_ref(&attr).cast(),
                    stack_probe,
                    ptr::null_mut(),
                )
            };
            let joined = rc == 0 && unsafe { pthread_join(thread, ptr::null_mut()) } == 0;
            unsafe { pthread_attr_destroy(&mut attr) };
            let inside = STACK_INSIDE.load(Ordering::SeqCst) == 1;
            set(
                STACK_SEEN.load(Ordering::SeqCst),
                i64::from(joined && inside),
            );
        },
        |size, ok| ok == 1 && size >= 4 << 20,
        "size"
    );

    proof!(
        "8 GiB PROT_NONE reservation, partly made accessible",
        {
            let len = 8usize << 30;
            let p = unsafe { mmap(ptr::null_mut(), len, 0, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) };
            let mut ok = p as isize != -1 && !p.is_null();
            if ok {
                ok = unsafe { mprotect(p, 65536, PROT_READ | PROT_WRITE) } == 0;
                if ok {
                    let bytes = p.cast::<u8>();
                    unsafe { bytes.write_volatile(0x5a) };
                    unsafe { bytes.add(65535).write_volatile(0xa5) };
                    ok = unsafe { bytes.read_volatile() } == 0x5a;
                }
                ok &= unsafe { munmap(p, len) } == 0;
            }
            set(i64::from(if ok { 0 } else { errno() }), i64::from(ok));
        },
        |_err, ok| ok == 1,
        "errno"
    );

    proof!(
        "shm_open makes an object in /dev/shmem",
        {
            let name = c"/relibc-selftest-shm";
            unsafe { shm_unlink(name.as_ptr()) };
            let fd = unsafe { shm_open(name.as_ptr(), O_CREAT | O_EXCL | O_RDWR, 0o600) };
            let grown = fd >= 0 && unsafe { ftruncate(fd, 8192) } == 0;
            let size = if fd >= 0 {
                unsafe { lseek(fd, 0, SEEK_END) }
            } else {
                -1
            };
            let seen = unsafe { open(c"/dev/shmem/relibc-selftest-shm".as_ptr(), O_RDONLY) };
            if seen >= 0 {
                unsafe { close(seen) };
            }
            if fd >= 0 {
                unsafe { close(fd) };
            }
            let unlinked = unsafe { shm_unlink(name.as_ptr()) } == 0;
            set(size, i64::from(grown && seen >= 0 && unlinked));
        },
        |size, ok| ok == 1 && size == 8192,
        "size"
    );
}
