//! Thread creation and teardown.
//!
//! relibc's `pthread_create` allocates a stack, pushes a fixed argument block
//! onto it, and hands the resulting pointer to [`clone`]. The new thread is
//! expected to pop that block and tail-call `new_thread_shim`:
//!
//! ```text
//! [sp +  0]  new_thread_shim
//! [sp +  8]  start_routine
//! [sp + 16]  arg
//! [sp + 24]  tcb
//! [sp + 32]  synchronization mutex
//! ```
//!
//! Neutrino's `ThreadCreate` starts a thread at `func(arg)` on a stack
//! described by its attribute block, so the argument block is passed through
//! `arg` and read back by a small assembly trampoline. The stack it is given
//! is relibc's own, everything below the block, so the thread gets the size
//! it was created with and `pthread_getattr_np` reports the stack it runs on.
//!
//! Unlike Linux, the kernel supplies the thread's control block and publishes
//! it through `tpidrro_el0`, so there is no TLS register to set up here — the
//! shim's call to `Tcb::activate` handles relibc's own ELF thread-local
//! storage through `tpidr_el0`, which QNX leaves alone.

use core::{
    arch::{asm, global_asm},
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use super::{
    abi::{
        neutrino::{SchedParam, ThreadAttr, tctl},
        signal::how,
    },
    kernel::{signal_procmask, thread_create, thread_ctl, thread_detach},
    mem, tls,
};
use crate::{
    error::{Errno, Result},
    header::errno::{EINVAL, ERANGE},
    ld_so::tcb::Tcb,
    platform::types::*,
    pthread::OsTid,
};

// The argument block relibc prepared is passed in x0. Load the entry point and
// its four arguments straight out of it and jump; `new_thread_shim` is
// `-> !`, so the `brk` is only there to fault loudly if that ever changes.
global_asm!(
    ".globl __relibc_qnx_thread_start",
    ".hidden __relibc_qnx_thread_start",
    ".type __relibc_qnx_thread_start, @function",
    "__relibc_qnx_thread_start:",
    "mov x9, x0",
    "ldr x8, [x9]",
    "ldp x0, x1, [x9, #8]",
    "ldp x2, x3, [x9, #24]",
    "blr x8",
    "brk #0",
    ".size __relibc_qnx_thread_start, . - __relibc_qnx_thread_start",
);

unsafe extern "C" {
    fn __relibc_qnx_thread_start(arg: *mut c_void) -> *mut c_void;
}

/// Starts a thread on the argument block relibc prepared, on relibc's stack.
///
/// relibc records the stack's bounds in the new thread's TCB, the block's
/// fourth word, before calling this. The kernel is given the part of that
/// stack below the block, so the block stays intact above the thread's stack
/// pointer until the trampoline has read it.
///
/// # Safety
///
/// `stack` must point at an argument block laid out as described in the module
/// documentation, inside the stack recorded in its TCB.
pub unsafe fn clone(stack: *mut usize) -> Result<OsTid> {
    // SAFETY: the caller guarantees the block, and its TCB word points at the
    // new thread's live TCB.
    let (base, size) = unsafe {
        let tcb = stack.add(3).read() as *const Tcb;
        (
            (*tcb).pthread.stack_base as usize,
            (*tcb).pthread.stack_size,
        )
    };
    let below = (stack as usize)
        .checked_sub(base)
        .filter(|&below| base != 0 && below < size);
    // The kernel starts the thread with its stack pointer at the top of what
    // it is given, which AArch64 requires to be 16-byte aligned.
    let Some(stacksize) = below.map(|below| below & !15).filter(|&len| len != 0) else {
        return Err(Errno(EINVAL));
    };
    let attr = ThreadAttr {
        flags: 0,
        stacksize,
        stackaddr: base as *mut c_void,
        exitfunc: None,
        // Inherit the creating thread's policy and priority.
        policy: -1,
        param: SchedParam::default(),
        guardsize: 0,
        prealloc: 0,
        reserved: [0; 2],
    };
    let tid = unsafe {
        thread_create(
            tls::pid(),
            __relibc_qnx_thread_start,
            stack.cast::<c_void>(),
            &attr,
        )
    }?;
    // relibc joins through its own `Pthread` state and never calls the
    // kernel's ThreadJoin, so a joinable kernel thread would stay a zombie
    // after it exits. Detaching reaps it then (or now, if it already has).
    let _ = thread_detach(tid);
    Ok(OsTid {
        thread_id: tid as usize,
    })
}

/// The calling thread's identifier as relibc spells it.
pub fn current() -> OsTid {
    OsTid {
        thread_id: tls::tid() as usize,
    }
}

/// `_NTO_THREAD_NAME_MAX`: the longest thread name, without the terminator.
pub const THREAD_NAME_MAX: usize = 100;

/// `struct _thread_name`, the argument of `ThreadCtl(_NTO_TCTL_NAME)`.
///
/// A non-negative `new_name_len` replaces the name with the first
/// `new_name_len` bytes of `name_buf` (0 deletes it); a negative one leaves it
/// alone. Either way the kernel writes the previous name, terminated, into
/// `name_buf`, of which `name_buf_len` bytes are available.
#[repr(C)]
struct ThreadName {
    new_name_len: c_int,
    name_buf_len: c_int,
    name_buf: [u8; THREAD_NAME_MAX + 1],
}

/// Names the calling thread. The kernel only names its caller this way, so
/// `pthread_setname_np` handles only the calling thread.
pub fn set_name(name: &[u8]) -> Result<()> {
    if name.len() > THREAD_NAME_MAX {
        return Err(Errno(ERANGE));
    }
    let mut tn = ThreadName {
        new_name_len: name.len() as c_int,
        name_buf_len: (THREAD_NAME_MAX + 1) as c_int,
        name_buf: [0; THREAD_NAME_MAX + 1],
    };
    tn.name_buf[..name.len()].copy_from_slice(name);
    unsafe { thread_ctl(tctl::NAME, (&raw mut tn).cast()) }.map(|_| ())
}

/// Copies the calling thread's name, terminated, into `buf`, and returns its
/// length without the terminator.
pub fn get_name(buf: &mut [u8]) -> Result<usize> {
    let mut tn = ThreadName {
        new_name_len: -1,
        name_buf_len: (THREAD_NAME_MAX + 1) as c_int,
        name_buf: [0; THREAD_NAME_MAX + 1],
    };
    unsafe { thread_ctl(tctl::NAME, (&raw mut tn).cast()) }?;
    let len = tn.name_buf.iter().position(|&b| b == 0).unwrap_or(THREAD_NAME_MAX);
    if len >= buf.len() {
        return Err(Errno(ERANGE));
    }
    buf[..len].copy_from_slice(&tn.name_buf[..len]);
    buf[len] = 0;
    Ok(len)
}

/// The main thread's stack, `[base, base + size)`, as [`record_main_thread_stack`]
/// found it; zero until then.
static MAIN_STACK: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// Records the main thread's stack, for `pthread_getattr_np`. Called by the
/// main thread while relibc starts, near the top of its stack.
///
/// The kernel publishes the stack's lowest address in its thread block
/// (`__stackaddr`) but not its top, which is taken as the end of the page
/// holding the current stack pointer. Only the frames of the start-up code
/// lie above that, so the size is a slight underestimate, never an overestimate.
///
/// The bounds are kept here rather than in the main thread's `Pthread`, whose
/// stack fields mean "unmap this on exit", which the kernel's stack must not be.
pub fn record_main_thread_stack() {
    let base = tls::stack_addr() as usize;
    let sp: usize;
    // SAFETY: reading sp has no side effects.
    unsafe { asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags)) };
    let top = (sp & !(super::PAGE_SIZE - 1)) + super::PAGE_SIZE;
    if base != 0 && base < top {
        MAIN_STACK[0].store(base, Ordering::Relaxed);
        MAIN_STACK[1].store(top - base, Ordering::Relaxed);
    }
}

/// The main thread's stack base and size, or `(null, 0)` if unknown.
pub fn main_thread_stack() -> (*mut c_void, usize) {
    (
        core::ptr::with_exposed_provenance_mut(MAIN_STACK[0].load(Ordering::Relaxed)),
        MAIN_STACK[1].load(Ordering::Relaxed),
    )
}

const EXIT_STACK_SIZE: usize = 16 * 1024;

#[repr(C, align(16))]
struct ExitStack([u8; EXIT_STACK_SIZE]);

/// Where a thread finishes exiting once its own stack is going away, one
/// thread at a time under [`EXIT_LOCK`]. Unmapping is a message to the
/// process manager, so it needs a stack of its own.
static EXIT_STACK: SyncUnsafeCell<ExitStack> = SyncUnsafeCell::new(ExitStack([0; EXIT_STACK_SIZE]));
static EXIT_LOCK: AtomicU32 = AtomicU32::new(0);

// (exit stack top, stack base, stack size, &EXIT_LOCK): switch to the exit
// stack, unmap the thread's stack, then release the exit stack and destroy
// the thread without touching memory in between. Thread 0 is the caller;
// priority -1 keeps it. Both return paths of the kernel call trap.
global_asm!(
    ".globl __relibc_qnx_exit_thread",
    ".hidden __relibc_qnx_exit_thread",
    ".type __relibc_qnx_exit_thread, @function",
    "__relibc_qnx_exit_thread:",
    "mov sp, x0",
    "mov x19, x3",
    "mov x0, x1",
    "mov x1, x2",
    "bl {release}",
    "stlr wzr, [x19]",
    "mov x0, #0",
    "mov x1, #-1",
    "mov x2, #0",
    "mov w8, #{destroy}",
    "svc #{svc}",
    "brk #0",
    "brk #0",
    ".size __relibc_qnx_exit_thread, . - __relibc_qnx_exit_thread",
    release = sym release_stack,
    destroy = const super::version::ker::THREAD_DESTROY,
    svc = const super::version::SVC_IMM,
);

unsafe extern "C" {
    fn __relibc_qnx_exit_thread(
        exit_stack_top: *mut u8,
        stack_base: *mut (),
        stack_size: usize,
        lock: *mut u32,
    ) -> !;
}

/// Unmaps an exiting thread's stack, running on the exit stack.
unsafe extern "C" fn release_stack(base: *mut (), size: usize) {
    if !base.is_null() && size != 0 {
        let _ = unsafe { mem::munmap(base.cast::<c_void>(), size) };
    }
}

/// Terminates the calling thread, releasing the stack relibc allocated for it.
///
/// # Safety
///
/// The calling thread must be one relibc started, running on that stack, and
/// nothing may refer to the stack once this is called.
pub unsafe fn exit_thread(stack_base: *mut (), stack_size: usize) -> ! {
    // No signal handler may run on the shared exit stack, and this thread is
    // ending, so block every signal that can be blocked.
    let all = [u32::MAX; 2];
    let _ =
        unsafe { signal_procmask(0, 0, how::BLOCK, all.as_ptr().cast(), core::ptr::null_mut()) };
    while EXIT_LOCK
        .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    // SAFETY: the lock makes this thread the exit stack's only user until the
    // assembly releases it, after which it no longer touches memory.
    unsafe {
        let top = EXIT_STACK.get().cast::<u8>().add(EXIT_STACK_SIZE);
        __relibc_qnx_exit_thread(top, stack_base, stack_size, EXIT_LOCK.as_ptr())
    }
}
