//! The kernel-supplied thread control block, QNX 8.0.
//!
//! 8.0 no longer uses `tpidrro_el0`. The kernel points `tpidr_el0` at an
//! AArch64 variant-I thread control block whose second word holds the
//! thread's `struct _thread_local_storage *`, and the vendor libc reaches
//! everything through it:
//!
//! ```text
//! getpid:  mrs x0, tpidr_el0 ; ldr x1, [x0, #8] ; ldr w0, [x1, #32] ; ret
//! gettid:  mrs x0, tpidr_el0 ; ldr x1, [x0, #8] ; ldr w0, [x1, #36] ; ret
//! ```
//!
//! No user-space code in the 8.0 libc, `ldqnx-64.so.2` or `crt1.o` ever
//! writes `tpidr_el0`, so the kernel sets it for every thread. The block's
//! own layout is 7.1's.
//!
//! relibc still installs its own thread pointer for its ELF thread-local
//! storage, exactly as on 7.1 and Linux. Word 1 of relibc's AArch64 ABI page
//! (`[tp + 8]`) is unused by relibc, so [`prepare_abi_ptr`] copies the kernel
//! block pointer into it before the switch, and `[tpidr_el0 + 8]` stays valid
//! on both sides of it. [`restore_tpidr`] is kept from 7.1 as a safety net in
//! case the kernel restores its own thread pointer across a kernel call; it
//! costs a compare when it does not.
//!
//! Not yet validated on hardware. The `start:` trace lines print `tpidr_el0`
//! before and after the first kernel call, which shows whether the kernel
//! preserves a thread pointer relibc has written.

use core::{
    arch::asm,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use super::{
    kernel,
    version::{cpupage, tls as offset},
};
use crate::platform::types::*;

/// Where the `_thread_local_storage *` sits in the thread control block.
const TCB_BLOCK: usize = 8;

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

/// `[tpidr_el0 + 8]` without a kernel call, or zero.
///
/// Valid whether the register holds the kernel's thread pointer or relibc's,
/// since [`prepare_abi_ptr`] mirrors the slot.
fn block_noker() -> usize {
    let tp = tpidr_raw();
    if tp == 0 {
        return 0;
    }
    unsafe {
        core::ptr::with_exposed_provenance::<usize>(tp + TCB_BLOCK).read()
    }
}

/// Kernel tid without a kernel call, because [`restore_tpidr`] runs on the
/// return path of every kernel call.
fn tid_noker() -> u32 {
    let p = block_noker();
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

/// Called by `Tcb::os_arch_activate` just before `tpidr_el0` is pointed at
/// relibc's ABI page at `abi_ptr`: seeds `[abi_ptr + 8]` with this thread's
/// kernel block, taken from the thread pointer that is about to be replaced.
pub fn prepare_abi_ptr(abi_ptr: usize) {
    let mut block = block_noker();
    if block == 0 {
        if let Ok(from_kernel) = kernel::sys_cpupage_get(cpupage::TLS) {
            block = from_kernel;
        }
    }
    unsafe {
        core::ptr::with_exposed_provenance_mut::<usize>(abi_ptr + TCB_BLOCK).write(block);
    }
}

/// Records the ABI pointer installed by `Tcb::os_arch_activate`.
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

/// The raw `tpidrro_el0` value. 8.0 leaves it unused; kept for the startup
/// trace, which prints both registers.
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
    let mut ptr = block_noker();
    if ptr == 0 {
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
