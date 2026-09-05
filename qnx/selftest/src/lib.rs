//! Bring-up self-test for the QNX backend.
//!
//! Each step prints `==>` before the call and `ok` / `FAIL` / `SKIP` / `FAULT`
//! after it, so a hard crash still names the step it died in. A logical
//! failure never stops the run, and a memory fault is caught, named, counted,
//! and the run continues from the next step.
//!
//! Fault recovery uses `setjmp`/`longjmp`. Both live in `main`'s frame — the
//! jump buffer is filled by the `step!` macro, which expands at the call site,
//! so the frame `longjmp` returns into is still alive. Everything a later step
//! needs is kept in a `static`, because locals written between `setjmp` and
//! `longjmp` have indeterminate values afterwards.

#![no_std]

use core::{
    ffi::{c_char, c_int, c_void},
    ptr,
    sync::atomic::{AtomicI64, AtomicPtr, AtomicU32, AtomicUsize, Ordering},
};

mod proof;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    write_str(2, "\nPANIC: the test binary itself panicked during ");
    write_current(2);
    write_str(2, "\n");
    unsafe { _exit(70) }
}

unsafe extern "C" {
    fn write(fd: c_int, buf: *const u8, n: usize) -> isize;
    fn read(fd: c_int, buf: *mut u8, n: usize) -> isize;
    fn open(path: *const c_char, oflag: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn lseek(fd: c_int, offset: i64, whence: c_int) -> i64;
    fn _exit(status: c_int) -> !;
    fn __errno_location() -> *mut c_int;

    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;

    fn getpid() -> c_int;
    fn getppid() -> c_int;
    fn sched_yield() -> c_int;
    fn clock_gettime(clk: c_int, tp: *mut Timespec) -> c_int;
    fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> c_int;

    fn pthread_create(
        thread: *mut usize,
        attr: *const c_void,
        start: extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
    ) -> c_int;
    fn pthread_join(thread: usize, retval: *mut *mut c_void) -> c_int;

    fn fork() -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn raise(sig: c_int) -> c_int;
    fn signal(sig: c_int, handler: extern "C" fn(c_int)) -> usize;
    fn sigprocmask(how: c_int, set: *const u64, old: *mut u64) -> c_int;

    fn dl_iterate_phdr(
        callback: extern "C" fn(*mut c_void, usize, *mut c_void) -> c_int,
        data: *mut c_void,
    ) -> c_int;

    fn setjmp(env: *mut u64) -> c_int;
    fn longjmp(env: *mut u64, val: c_int) -> !;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

const O_RDONLY: c_int = 0x0000;
const O_WRONLY: c_int = 0x0001;
const CLOCK_REALTIME: c_int = 0;
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_PRIVATE: c_int = 0x0002;
const MAP_ANONYMOUS: c_int = 0x0020;
const SEEK_SET: c_int = 0;
const SIG_UNBLOCK: c_int = 1;
const SIGILL: c_int = 4;
const SIGBUS: c_int = 7;
const SIGUSR1: c_int = 10;
const SIGSEGV: c_int = 11;

fn errno() -> c_int {
    unsafe { *__errno_location() }
}

fn write_str(fd: c_int, s: &str) -> isize {
    unsafe { write(fd, s.as_ptr(), s.len()) }
}

fn write_num(fd: c_int, value: i64) {
    let mut buf = [0u8; 24];
    let mut n = buf.len();
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    if magnitude == 0 {
        n -= 1;
        buf[n] = b'0';
    }
    while magnitude > 0 {
        n -= 1;
        buf[n] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
    }
    if negative {
        n -= 1;
        buf[n] = b'-';
    }
    unsafe { write(fd, buf.as_ptr().add(n), buf.len() - n) };
}

fn write_hex(fd: c_int, value: u64) {
    write_str(fd, "0x");
    if value == 0 {
        write_str(fd, "0");
        return;
    }
    let mut started = false;
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as u8;
        if nibble != 0 {
            started = true;
        }
        if started {
            let ch = if nibble < 10 {
                b'0' + nibble
            } else {
                b'a' + (nibble - 10)
            };
            unsafe { write(fd, &ch, 1) };
        }
    }
}

fn errno_name(err: c_int) -> &'static str {
    match err {
        1 => "EPERM",
        2 => "ENOENT",
        4 => "EINTR",
        5 => "EIO",
        9 => "EBADF",
        11 => "EAGAIN",
        12 => "ENOMEM",
        13 => "EACCES",
        14 => "EFAULT",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        24 => "EMFILE",
        34 => "ERANGE",
        38 => "ENOSYS",
        _ => "",
    }
}

fn sig_name(sig: c_int) -> &'static str {
    match sig {
        4 => "SIGILL",
        7 => "SIGBUS",
        11 => "SIGSEGV",
        _ => "",
    }
}

static STEP: AtomicU32 = AtomicU32::new(0);
static FAILURES: AtomicU32 = AtomicU32::new(0);
static FAULTS: AtomicU32 = AtomicU32::new(0);
static SKIPS: AtomicU32 = AtomicU32::new(0);

static CURRENT_PTR: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static CURRENT_LEN: AtomicUsize = AtomicUsize::new(0);
static LAST_FAULT: AtomicU32 = AtomicU32::new(0);
static ARMED: AtomicU32 = AtomicU32::new(0);

/// `jmp_buf` for aarch64 is 22 `u64`s (see `setjmp.s`).
static mut JMP: [u64; 22] = [0; 22];

fn jmp_buf() -> *mut u64 {
    ptr::addr_of_mut!(JMP).cast::<u64>()
}

fn write_current(fd: c_int) {
    let p = CURRENT_PTR.load(Ordering::SeqCst);
    let n = CURRENT_LEN.load(Ordering::SeqCst);
    if p.is_null() || n == 0 {
        write_str(fd, "<no step>");
        return;
    }
    unsafe { write(fd, p, n) };
}

fn begin(name: &'static str) {
    CURRENT_PTR.store(name.as_ptr().cast_mut(), Ordering::SeqCst);
    CURRENT_LEN.store(name.len(), Ordering::SeqCst);
    write_str(1, "==> ");
    write_str(1, name);
    write_str(1, "\n");
}

fn write_step_prefix(kind: &str, name: &str) -> u32 {
    let step = STEP.fetch_add(1, Ordering::Relaxed) + 1;
    write_str(1, kind);
    write_num(1, i64::from(step));
    write_str(1, "  ");
    write_str(1, name);
    step
}

fn report(name: &str, ok: bool) {
    report_note(name, ok, "", 0, false);
}

fn report_i(name: &str, ok: bool, label: &str, value: i64) {
    report_note(name, ok, label, value as u64, false);
}

fn report_hex(name: &str, ok: bool, label: &str, value: u64) {
    report_note(name, ok, label, value, true);
}

fn report_note(name: &str, ok: bool, label: &str, value: u64, hex: bool) {
    write_step_prefix(if ok { "ok    " } else { "FAIL  " }, name);
    if !label.is_empty() {
        write_str(1, "   ");
        write_str(1, label);
        write_str(1, "=");
        if hex {
            write_hex(1, value);
        } else {
            write_num(1, value as i64);
        }
    }
    if !ok {
        FAILURES.fetch_add(1, Ordering::Relaxed);
        let err = errno();
        write_str(1, "   errno=");
        write_num(1, i64::from(err));
        let named = errno_name(err);
        if !named.is_empty() {
            write_str(1, " ");
            write_str(1, named);
        }
    }
    write_str(1, "\n");
}

fn report_fault(name: &str) {
    let sig = LAST_FAULT.load(Ordering::SeqCst) as c_int;
    FAILURES.fetch_add(1, Ordering::Relaxed);
    FAULTS.fetch_add(1, Ordering::Relaxed);
    write_step_prefix("FAULT ", name);
    write_str(1, "   sig=");
    write_num(1, i64::from(sig));
    let named = sig_name(sig);
    if !named.is_empty() {
        write_str(1, " ");
        write_str(1, named);
    }
    write_str(1, "   recovered, continuing\n");
}

fn skip(name: &str, why: &str) {
    SKIPS.fetch_add(1, Ordering::Relaxed);
    write_step_prefix("SKIP  ", name);
    write_str(1, "   ");
    write_str(1, why);
    write_str(1, "\n");
}

extern "C" fn on_fault(sig: c_int) {
    LAST_FAULT.store(sig as u32, Ordering::SeqCst);
    if ARMED.load(Ordering::SeqCst) != 0 {
        // Leaving a handler by longjmp skips the kernel's signal return, so
        // the signal would stay blocked and a second fault would kill the
        // process outright. Unblock it here.
        let set: u64 = 1u64 << (sig - 1);
        unsafe { sigprocmask(SIG_UNBLOCK, &set, ptr::null_mut()) };
        unsafe { longjmp(jmp_buf(), 1) };
    }
    write_str(2, "\nFAULT sig=");
    write_num(2, i64::from(sig));
    write_str(2, " ");
    write_str(2, sig_name(sig));
    write_str(2, " during ");
    write_current(2);
    write_str(2, "   (outside a guarded step, stopping)\n");
    unsafe { _exit(80) };
}

fn install_fault_handlers() {
    unsafe {
        signal(SIGSEGV, on_fault);
        signal(SIGBUS, on_fault);
        signal(SIGILL, on_fault);
    }
}

/// Runs a step with the fault guard armed, evaluating to `true` when it ran to
/// completion and `false` when it faulted (already reported).
///
/// This has to be a macro: `setjmp` must be executed in the frame that is
/// still live when the handler calls `longjmp`, which is `main`'s.
macro_rules! step {
    ($name:expr, $body:block) => {{
        begin($name);
        // SAFETY: the buffer is written here and only read by `on_fault`,
        // which runs on this thread while this frame is live.
        let completed = unsafe { setjmp(jmp_buf()) } == 0;
        if completed {
            ARMED.store(1, Ordering::SeqCst);
            $body
            ARMED.store(0, Ordering::SeqCst);
        } else {
            ARMED.store(0, Ordering::SeqCst);
            install_fault_handlers();
            report_fault($name);
        }
        completed
    }};
}

// Cross-step state. These are statics rather than locals because a local
// written between `setjmp` and `longjmp` has an indeterminate value after the
// jump, and later steps branch on these.
static PAGE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static HEAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ERRNO_PTR: AtomicPtr<c_int> = AtomicPtr::new(ptr::null_mut());
static NULL_FD: AtomicI64 = AtomicI64::new(-1);
static SELF_FD: AtomicI64 = AtomicI64::new(-1);
static THREAD: AtomicUsize = AtomicUsize::new(0);
static CREATED: AtomicI64 = AtomicI64::new(-1);
static PREV_HANDLER: AtomicUsize = AtomicUsize::new(usize::MAX);
static CHILD: AtomicI64 = AtomicI64::new(-1);
static SCRATCH: AtomicI64 = AtomicI64::new(0);
static OBJECTS: AtomicU32 = AtomicU32::new(0);

static HANDLER_RAN: AtomicU32 = AtomicU32::new(0);

extern "C" fn on_signal(_sig: c_int) {
    HANDLER_RAN.store(1, Ordering::SeqCst);
}

static THREAD_RAN: AtomicU32 = AtomicU32::new(0);

extern "C" fn thread_body(_arg: *mut c_void) -> *mut c_void {
    THREAD_RAN.store(1, Ordering::SeqCst);
    ptr::null_mut()
}

extern "C" fn count_object(_info: *mut c_void, _size: usize, data: *mut c_void) -> c_int {
    // SAFETY: `data` is the counter passed to dl_iterate_phdr below.
    unsafe { *data.cast::<u32>() += 1 };
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: c_int, argv: *const *const c_char) -> c_int {
    if let Some(code) = proof::child_mode(argc, argv) {
        unsafe { _exit(code) };
    }
    write_str(1, "relibc QNX backend self-test  HARNESS=6\n");
    write_str(1, "every step is reported; failures and faults do not stop the run\n\n");

    install_fault_handlers();
    report("write to stdout", true);

    // Layer 1: kernel-call convention, thread control block, clocks.
    if step!("thread-local errno pointer", {
        ERRNO_PTR.store(unsafe { __errno_location() }, Ordering::SeqCst);
    }) {
        let p = ERRNO_PTR.load(Ordering::SeqCst);
        report_hex("thread-local errno pointer", !p.is_null(), "ptr", p as u64);
    }

    let errno_ptr = ERRNO_PTR.load(Ordering::SeqCst);
    if errno_ptr.is_null() {
        skip("write thread-local errno", "errno pointer was null");
    } else if step!("write thread-local errno", {
        unsafe { ptr::write_volatile(errno_ptr, 0) };
        SCRATCH.store(i64::from(unsafe { ptr::read_volatile(errno_ptr) }), Ordering::SeqCst);
    }) {
        let v = SCRATCH.load(Ordering::SeqCst);
        report_i("write thread-local errno", v == 0, "value", v);
    }

    if step!("getpid (tpidrro_el0)", {
        SCRATCH.store(i64::from(unsafe { getpid() }), Ordering::SeqCst);
    }) {
        let v = SCRATCH.load(Ordering::SeqCst);
        report_i("getpid (tpidrro_el0)", v > 0, "pid", v);
    }

    if step!("getppid (procnto message)", {
        SCRATCH.store(i64::from(unsafe { getppid() }), Ordering::SeqCst);
    }) {
        let v = SCRATCH.load(Ordering::SeqCst);
        report_i("getppid (procnto message)", v >= 0, "ppid", v);
    }

    if step!("sched_yield", {
        SCRATCH.store(i64::from(unsafe { sched_yield() }), Ordering::SeqCst);
    }) {
        let v = SCRATCH.load(Ordering::SeqCst);
        report_i("sched_yield", v == 0, "rc", v);
    }

    let mut now = Timespec::default();
    if step!("clock_gettime", {
        SCRATCH.store(
            i64::from(unsafe { clock_gettime(CLOCK_REALTIME, &raw mut now) }),
            Ordering::SeqCst,
        );
    }) {
        let rc = SCRATCH.load(Ordering::SeqCst);
        report_i("clock_gettime", rc == 0, "rc", rc);
        if rc == 0 {
            write_str(1, "          tv_sec=");
            write_num(1, now.tv_sec);
            write_str(1, " tv_nsec=");
            write_num(1, now.tv_nsec);
            write_str(1, "\n");
        }
    }

    let nap = Timespec {
        tv_sec: 0,
        tv_nsec: 1_000_000,
    };
    if step!("nanosleep 1ms", {
        SCRATCH.store(
            i64::from(unsafe { nanosleep(&nap, ptr::null_mut()) }),
            Ordering::SeqCst,
        );
    }) {
        let v = SCRATCH.load(Ordering::SeqCst);
        report_i("nanosleep 1ms", v == 0, "rc", v);
    }

    // Layer 2: memory.
    if step!("mmap anonymous page", {
        PAGE.store(
            unsafe {
                mmap(
                    ptr::null_mut(),
                    4096,
                    PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS,
                    -1,
                    0,
                )
            },
            Ordering::SeqCst,
        );
    }) {
        let p = PAGE.load(Ordering::SeqCst);
        report_hex("mmap anonymous page", p as isize > 0, "addr", p as u64);
    }

    let page = PAGE.load(Ordering::SeqCst);
    if page as isize <= 0 {
        skip("write and read the mapping", "mmap failed");
        skip("munmap", "mmap failed");
    } else {
        if step!("write and read the mapping", {
            unsafe { ptr::write_volatile(page.cast::<u64>(), 0x5A5A_5A5A) };
            SCRATCH.store(
                unsafe { ptr::read_volatile(page.cast::<u64>()) } as i64,
                Ordering::SeqCst,
            );
        }) {
            let v = SCRATCH.load(Ordering::SeqCst) as u64;
            report_hex("write and read the mapping", v == 0x5A5A_5A5A, "read", v);
        }
        if step!("munmap", {
            SCRATCH.store(i64::from(unsafe { munmap(page, 4096) }), Ordering::SeqCst);
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("munmap", v == 0, "rc", v);
        }
    }

    if step!("malloc (allocator + futex)", {
        HEAP.store(unsafe { malloc(1024) }, Ordering::SeqCst);
    }) {
        let p = HEAP.load(Ordering::SeqCst);
        report_hex("malloc (allocator + futex)", !p.is_null(), "ptr", p as u64);
    }

    let heap = HEAP.load(Ordering::SeqCst);
    if heap.is_null() {
        skip("write and read the allocation", "malloc returned NULL");
    } else if step!("write and read the allocation", {
        unsafe { ptr::write_volatile(heap.cast::<u64>(), 0xA5A5_A5A5) };
        SCRATCH.store(
            unsafe { ptr::read_volatile(heap.cast::<u64>()) } as i64,
            Ordering::SeqCst,
        );
    }) {
        let v = SCRATCH.load(Ordering::SeqCst) as u64;
        report_hex("write and read the allocation", v == 0xA5A5_A5A5, "read", v);
    }

    // Layer 3: the path manager. Run before free() so that a crash in the
    // allocator's teardown cannot hide whether _IO_CONNECT works.
    if step!("open /dev/null", {
        NULL_FD.store(
            i64::from(unsafe { open(c"/dev/null".as_ptr(), O_WRONLY) }),
            Ordering::SeqCst,
        );
    }) {
        let fd = NULL_FD.load(Ordering::SeqCst);
        report_i("open /dev/null", fd >= 0, "fd", fd);
    }

    let null_fd = NULL_FD.load(Ordering::SeqCst) as c_int;
    if null_fd < 0 {
        skip("write to /dev/null", "open failed");
        skip("close /dev/null", "open failed");
    } else {
        if step!("write to /dev/null", {
            SCRATCH.store(
                unsafe { write(null_fd, b"x".as_ptr(), 1) } as i64,
                Ordering::SeqCst,
            );
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("write to /dev/null", v == 1, "n", v);
        }
        if step!("close /dev/null", {
            SCRATCH.store(i64::from(unsafe { close(null_fd) }), Ordering::SeqCst);
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("close /dev/null", v == 0, "rc", v);
        }
    }

    if step!("open /proc/self/exefile", {
        SELF_FD.store(
            i64::from(unsafe { open(c"/proc/self/exefile".as_ptr(), O_RDONLY) }),
            Ordering::SeqCst,
        );
    }) {
        let fd = SELF_FD.load(Ordering::SeqCst);
        report_i("open /proc/self/exefile", fd >= 0, "fd", fd);
    }

    let self_fd = SELF_FD.load(Ordering::SeqCst) as c_int;
    if self_fd < 0 {
        skip("read from /proc", "open failed");
        skip("lseek", "open failed");
    } else {
        let mut buf = [0u8; 64];
        if step!("read from /proc", {
            SCRATCH.store(
                unsafe { read(self_fd, buf.as_mut_ptr(), buf.len()) } as i64,
                Ordering::SeqCst,
            );
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("read from /proc", v > 0, "n", v);
        }
        if step!("lseek", {
            SCRATCH.store(unsafe { lseek(self_fd, 0, SEEK_SET) }, Ordering::SeqCst);
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("lseek", v == 0, "off", v);
        }
        unsafe { close(self_fd) };
    }

    if heap.is_null() {
        skip("free", "malloc returned NULL");
    } else if step!("free", {
        unsafe { free(heap) };
    }) {
        report("free", true);
    }

    // Layer 4: threads, which exercise the futex and the TLS activation.
    if step!("pthread_create", {
        let mut tid: usize = 0;
        CREATED.store(
            i64::from(unsafe {
                pthread_create(&raw mut tid, ptr::null(), thread_body, ptr::null_mut())
            }),
            Ordering::SeqCst,
        );
        THREAD.store(tid, Ordering::SeqCst);
    }) {
        let rc = CREATED.load(Ordering::SeqCst);
        report_i("pthread_create", rc == 0, "rc", rc);
    }

    if CREATED.load(Ordering::SeqCst) != 0 {
        skip("pthread_join", "pthread_create failed");
        skip("thread actually ran", "pthread_create failed");
    } else {
        if step!("pthread_join", {
            SCRATCH.store(
                i64::from(unsafe { pthread_join(THREAD.load(Ordering::SeqCst), ptr::null_mut()) }),
                Ordering::SeqCst,
            );
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("pthread_join", v == 0, "rc", v);
        }
        let ran = THREAD_RAN.load(Ordering::SeqCst);
        report_i("thread actually ran", ran == 1, "ran", i64::from(ran));
    }

    // Layer 5: signals.
    if step!("signal (SignalAction + stub)", {
        PREV_HANDLER.store(unsafe { signal(SIGUSR1, on_signal) }, Ordering::SeqCst);
    }) {
        let prev = PREV_HANDLER.load(Ordering::SeqCst);
        report_hex(
            "signal (SignalAction + stub)",
            prev != usize::MAX,
            "prev",
            prev as u64,
        );
    }
    // Installing SIGUSR1 may have disturbed the fault handlers.
    install_fault_handlers();

    if PREV_HANDLER.load(Ordering::SeqCst) == usize::MAX {
        skip("raise SIGUSR1", "signal() failed");
        skip("handler ran", "signal() failed");
    } else {
        if step!("raise SIGUSR1", {
            SCRATCH.store(i64::from(unsafe { raise(SIGUSR1) }), Ordering::SeqCst);
        }) {
            let v = SCRATCH.load(Ordering::SeqCst);
            report_i("raise SIGUSR1", v == 0, "rc", v);
        }
        let ran = HANDLER_RAN.load(Ordering::SeqCst);
        report_i("handler ran", ran == 1, "ran", i64::from(ran));
    }

    // Layer 6: processes.
    if step!("fork", {
        let pid = unsafe { fork() };
        if pid == 0 {
            unsafe { _exit(42) };
        }
        CHILD.store(i64::from(pid), Ordering::SeqCst);
    }) {
        let pid = CHILD.load(Ordering::SeqCst);
        report_i("fork", pid > 0, "pid", pid);
    }

    let child = CHILD.load(Ordering::SeqCst) as c_int;
    if child <= 0 {
        skip("waitpid", "fork failed");
    } else {
        let mut status: c_int = 0;
        if step!("waitpid", {
            SCRATCH.store(
                i64::from(unsafe { waitpid(child, &raw mut status, 0) }),
                Ordering::SeqCst,
            );
        }) {
            let rc = SCRATCH.load(Ordering::SeqCst);
            report_i("waitpid", rc == i64::from(child), "rc", rc);
            write_str(1, "          status=");
            write_num(1, i64::from(status));
            write_str(1, "\n");
        }
    }

    // Layer 7: the unwinder's view of the process.
    if step!("dl_iterate_phdr sees objects", {
        let mut objects: u32 = 0;
        unsafe { dl_iterate_phdr(count_object, ptr::from_mut(&mut objects).cast()) };
        OBJECTS.store(objects, Ordering::SeqCst);
    }) {
        let n = OBJECTS.load(Ordering::SeqCst);
        report_i("dl_iterate_phdr sees objects", n > 0, "n", i64::from(n));
    }

    proof::run();

    let failures = FAILURES.load(Ordering::Relaxed);
    let faults = FAULTS.load(Ordering::Relaxed);
    let skips = SKIPS.load(Ordering::Relaxed);
    write_str(1, "\nsummary: ");
    write_num(1, i64::from(STEP.load(Ordering::Relaxed)));
    write_str(1, " steps, ");
    write_num(1, i64::from(failures));
    write_str(1, " failed (");
    write_num(1, i64::from(faults));
    write_str(1, " faults), ");
    write_num(1, i64::from(skips));
    write_str(1, " skipped\n");
    if failures == 0 {
        write_str(1, "all steps passed\n");
        0
    } else {
        1
    }
}
