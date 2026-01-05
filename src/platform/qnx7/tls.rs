//! The kernel-supplied thread control block.
//!
//! Neutrino hands every thread a `struct _thread_local_storage` and puts its
//! address in `tpidrro_el0`, which is read-only at EL0. That block is where the
//! kernel publishes the thread's pid and tid, so `getpid()` and `gettid()` are
//! plain loads rather than kernel calls — exactly what the vendor libc does:
//!
//! ```text
//! getpid:  mrs x0, tpidrro_el0 ; ldr w0, [x0, #32] ; ret
//! gettid:  mrs x0, tpidrro_el0 ; ldr w0, [x0, #36] ; ret
//! ```
//!
//! `tpidr_el0` is *not* used by QNX and is writable at EL0, which is what makes
//! it available for relibc's own ELF thread-local storage.

use core::{
    arch::asm,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use super::{
    kernel,
    version::{cpupage, tls as offset},
};
use crate::platform::types::*;

/// Fallback for the rare case the per-tid table is full: last ABI pointer
/// written by any thread. Fine for the single-threaded CRT path.
static LAST_ABI_PTR: AtomicUsize = AtomicUsize::new(0);

const ABI_SLOTS: usize = 128;

struct AbiSlot {
    tid: AtomicU32,
    ptr: AtomicUsize,
}

static ABI_BY_TID: [AbiSlot; ABI_SLOTS] = [const {
    AbiSlot {
        tid: AtomicU32::new(0),
        ptr: AtomicUsize::new(0),
    }
}; ABI_SLOTS];

/// Kernel tid from `tpidrro_el0` only — never a kercall, because
/// [`restore_tpidr`] runs on the return path of every kercall.
fn tid_noker() -> u32 {
    let p = tpidrro_raw();
    if p == 0 {
        return 0;
    }
    unsafe {
        core::ptr::with_exposed_provenance::<u8>(p)
            .add(offset::TID)
            .cast::<c_int>()
            .read() as u32
    }
}

fn lookup_abi_ptr(tid: u32) -> usize {
    for slot in &ABI_BY_TID {
        if slot.tid.load(Ordering::Acquire) == tid {
            let ptr = slot.ptr.load(Ordering::Acquire);
            if ptr != 0 {
                return ptr;
            }
        }
    }
    LAST_ABI_PTR.load(Ordering::Acquire)
}

/// Records the ABI pointer installed by `Tcb::os_arch_activate`.
///
/// Neutrino's own TLS lives in `tpidrro_el0`. `tpidr_el0` is ours, but a
/// kernel call can put a stale value back — `svc` is how we print startup
/// traces, so without restoring this the first `Tcb::current()` after a
/// `write` walks a wild pointer and SIGSEGVs.
pub fn note_abi_ptr(ptr: usize) {
    LAST_ABI_PTR.store(ptr, Ordering::Release);
    let tid = tid_noker();
    for slot in &ABI_BY_TID {
        let existing = slot.tid.load(Ordering::Acquire);
        if existing == tid {
            slot.ptr.store(ptr, Ordering::Release);
            return;
        }
        if existing == 0
            && slot
                .tid
                .compare_exchange(0, tid, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            slot.ptr.store(ptr, Ordering::Release);
            return;
        }
    }
}

/// Called by `Tcb::os_arch_activate` just before `tpidr_el0` is pointed at
/// relibc's ABI page. 7.1 publishes its block through `tpidrro_el0`, so
/// relibc's thread pointer needs nothing from the kernel here. (8.0 finds its
/// block through `[tpidr_el0 + 8]` and seeds that slot; see `../qnx8/tls.rs`.)
#[inline]
pub fn prepare_abi_ptr(_abi_ptr: usize) {}

/// Reinstalls this thread's ELF thread pointer if we have one.
#[inline]
pub fn restore_tpidr() {
    let abi = lookup_abi_ptr(tid_noker());
    if abi == 0 {
        return;
    }
    if tpidr_raw() == abi {
        return;
    }
    unsafe {
        asm!(
            "msr tpidr_el0, {tp}",
            "isb",
            tp = in(reg) abi,
            options(nostack, preserves_flags),
        );
    }
}

/// The raw `tpidr_el0` value.
#[inline]
pub fn tpidr_raw() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mrs {}, tpidr_el0", out(reg) ptr, options(nomem, nostack, preserves_flags));
    }
    ptr
}

/// The raw `tpidrro_el0` value, which may be zero.
#[inline]
pub fn tpidrro_raw() -> usize {
    let ptr: usize;
    // SAFETY: reading tpidrro_el0 has no side effects.
    unsafe {
        asm!("mrs {}, tpidrro_el0", out(reg) ptr, options(nomem, nostack, preserves_flags));
    }
    ptr
}

/// Returns the address of this thread's kernel TLS block.
#[inline]
pub fn block() -> *mut u8 {
    let mut ptr = tpidrro_raw();
    if ptr == 0 {
        // The register is the fast path the vendor libc uses, but a process
        // whose CRT never ran that libc can still ask the kernel for the
        // same pointer. A null here used to become `ldr [x0, #36]` and
        // SIGSEGV the first time anyone asked for a tid — which is
        // `pthread::init`, immediately after argv/environ are copied.
        if let Ok(from_kernel) = kernel::sys_cpupage_get(cpupage::TLS) {
            ptr = from_kernel;
        }
    }
    core::ptr::with_exposed_provenance_mut(ptr)
}

#[inline]
unsafe fn field<T: Copy>(byte_offset: usize) -> T {
    let base = block();
    if base.is_null() {
        // SAFETY: integer 0 is a valid bit pattern for pid/tid/owner/pointer.
        return unsafe { core::mem::zeroed() };
    }
    // SAFETY: the caller names an offset within the kernel's TLS block, whose
    // layout is fixed by the ABI.
    unsafe { base.add(byte_offset).cast::<T>().read() }
}

/// The calling thread's process id.
#[inline]
pub fn pid() -> pid_t {
    if block().is_null() {
        1
    } else {
        unsafe { field::<c_int>(offset::PID) }
    }
}

/// The calling thread's thread id.
#[inline]
pub fn tid() -> pid_t {
    if block().is_null() {
        1
    } else {
        unsafe { field::<c_int>(offset::TID) }
    }
}

/// The owner cookie the kernel uses to identify this thread in synchronisation
/// objects.
#[inline]
pub fn owner() -> c_uint {
    if block().is_null() {
        // 0 is `_NTO_SYNC_MUTEX_FREE`. Inventing that as our identity makes
        // every uncontended lock look like it is still free.
        1
    } else {
        unsafe { field::<c_uint>(offset::OWNER) }
    }
}

/// The base of this thread's stack, as recorded by the kernel.
#[inline]
pub fn stack_addr() -> *mut c_void {
    unsafe { field::<*mut c_void>(offset::STACKADDR) }
}
