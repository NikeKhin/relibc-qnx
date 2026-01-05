//! Futex emulation over Neutrino synchronisation objects.
//!
//! relibc builds all of its own locks — mutexes, `once`, rwlocks, semaphores,
//! the allocator's guards — on `futex_wait`/`futex_wake`. Neutrino has no
//! futex: it has whole synchronisation objects that cannot be attached to an
//! arbitrary address.
//!
//! The classic answer is a fixed table of condition variables, each covering
//! the futex addresses that hash to it. Waiting takes the bucket's mutex,
//! re-checks the word, and blocks on the condvar; waking takes the mutex and
//! signals. Sharing a bucket between unrelated addresses only causes spurious
//! wakeups, which the futex contract explicitly permits.
//!
//! Two details from the SDP headers make this work without any setup call:
//!
//! * `sync_t` is plain user-space memory — `{ __count, __owner }` — and QNX's
//!   own `_mutex_init` initialises a mutex by writing `{0, _NTO_SYNC_MUTEX_FREE}`
//!   precisely so the first lock need not enter the kernel.
//! * A condition variable is just as cheap: owner `_NTO_SYNC_COND` with the
//!   clock id in the count field.
//!
//! So the whole table is a `static` with no initialisation path and no lock
//! ordering problem at startup, which matters because relibc's allocator needs
//! futexes before anything has had a chance to run.
//!
//! The uncontended mutex fast path is the one from `sys/inline_mutex.h`:
//! compare-exchange the thread's TLS owner cookie into `__owner`, and only call
//! `SyncMutexLock` when that fails. Unlocking takes the reverse path and only
//! enters the kernel when `_NTO_SYNC_WAITING` says someone is blocked.
//!
//! The buckets belong to this process, so a wake issued by another process
//! never reaches them. For a futex word in memory registered with [`shared`]
//! (the named semaphores), a waiter therefore blocks for at most
//! [`SHARED_SLICE_NS`] at a time and re-checks the word in between: a wake
//! from this process still arrives at once through the bucket, and one from
//! another process is seen within a slice.

use core::{
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

use super::{
    abi::neutrino::{Sync, clock, sync_owner, timeout, timer_flags},
    kernel::{
        clock_time, sync_condvar_signal, sync_condvar_wait, sync_mutex_lock, sync_mutex_unlock,
        timer_timeout,
    },
    tls,
};
use crate::{
    error::{Errno, Result},
    header::{
        errno::{EAGAIN, ETIMEDOUT},
        time::timespec,
    },
    platform::types::c_uint,
};

/// Number of buckets. A power of two so the hash is a mask, and large enough
/// that unrelated locks rarely collide in a process with a few hundred threads.
const BUCKETS: usize = 64;

/// One bucket: a mutex guarding the wait decision, and a condvar to block on.
struct Bucket {
    mutex: SyncUnsafeCell<Sync>,
    condvar: SyncUnsafeCell<Sync>,
}

impl Bucket {
    const fn new() -> Self {
        Self {
            mutex: SyncUnsafeCell::new(Sync::mutex()),
            condvar: SyncUnsafeCell::new(Sync::condvar(clock::MONOTONIC)),
        }
    }
}

static BUCKETS_TABLE: [Bucket; BUCKETS] = [const { Bucket::new() }; BUCKETS];

/// Picks the bucket for a futex address.
///
/// Futex words are at least four-byte aligned, so the low bits carry no
/// information; the shift and multiply spread the remaining ones.
fn bucket_for(addr: *const u32) -> &'static Bucket {
    let index = (addr as usize >> 2).wrapping_mul(0x9E37_79B9) % BUCKETS;
    &BUCKETS_TABLE[index]
}

/// Takes a bucket mutex, using the vendor's user-space fast path.
fn lock(mutex: *mut Sync) -> Result<()> {
    let owner = tls::owner();
    // SAFETY: `mutex` points into the static table and stays valid forever.
    // The owner word is only ever accessed atomically, here and by the kernel.
    let owner_word = unsafe { AtomicU32::from_ptr(&raw mut (*mutex).owner) };

    if owner_word
        .compare_exchange(
            sync_owner::MUTEX_FREE,
            owner,
            Ordering::Acquire,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        return Ok(());
    }
    unsafe { sync_mutex_lock(mutex) }
}

/// Releases a bucket mutex, entering the kernel only if a thread is blocked.
fn unlock(mutex: *mut Sync) -> Result<()> {
    let owner = tls::owner();
    // SAFETY: as in `lock`.
    let owner_word = unsafe { AtomicU32::from_ptr(&raw mut (*mutex).owner) };

    if owner_word
        .compare_exchange(
            owner,
            sync_owner::MUTEX_FREE,
            Ordering::Release,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        return Ok(());
    }
    unsafe { sync_mutex_unlock(mutex) }
}

const NSEC_PER_SEC: i64 = 1_000_000_000;

/// How long a waiter on a word in shared memory blocks before re-checking it.
pub const SHARED_SLICE_NS: u64 = 2_000_000;

/// A futex deadline — an absolute `CLOCK_MONOTONIC` time, as for Linux's
/// `FUTEX_WAIT_BITSET` — in nanoseconds. A time before the epoch has passed,
/// and one too large for the clock never comes.
fn deadline_nanos(deadline: &timespec) -> u64 {
    if deadline.tv_sec < 0 {
        return 0;
    }
    deadline
        .tv_sec
        .checked_mul(NSEC_PER_SEC)
        .and_then(|secs| secs.checked_add(deadline.tv_nsec.clamp(0, NSEC_PER_SEC - 1)))
        .map_or(u64::MAX, |nanos| nanos as u64)
}

/// Arms an absolute timeout for the condvar wait the calling thread is about
/// to enter, the way the vendor `pthread_cond_timedwait` does.
///
/// `TimerTimeout` applies to the *next* blocking state the thread reaches, so
/// this has to be issued immediately before `SyncCondvarWait`.
fn arm_condvar_timeout(nanos: u64) -> Result<()> {
    // SAFETY: `nanos` is a local, and no notification event is requested.
    unsafe {
        timer_timeout(
            clock::MONOTONIC,
            timeout::CONDVAR | timer_flags::ABSTIME as c_uint,
            core::ptr::null(),
            &nanos,
            core::ptr::null_mut(),
        )
    }
    .map(|_| ())
}

fn monotonic_now() -> u64 {
    let mut now = 0;
    // SAFETY: `now` is a local; a null new value only reads the clock.
    let _ = unsafe { clock_time(clock::MONOTONIC, core::ptr::null(), &mut now) };
    now
}

/// Blocks while the word at `addr` still equals `val`.
///
/// # Safety
///
/// `addr` must point to a valid, aligned `u32` that remains live for the call.
pub unsafe fn futex_wait(addr: *mut u32, val: u32, deadline: Option<&timespec>) -> Result<()> {
    // SAFETY: the caller guarantees a live, aligned u32. Futex words are shared
    // by definition, so all access is atomic.
    let atom = unsafe { AtomicU32::from_ptr(addr) };
    let bucket = bucket_for(addr);
    let mutex = bucket.mutex.get();
    let condvar = bucket.condvar.get();

    let deadline = deadline.map(deadline_nanos);
    // Ends the wait early for a word another process may change; see the
    // module documentation.
    let slice = shared::contains(addr as usize)
        .then(|| monotonic_now().saturating_add(SHARED_SLICE_NS))
        .filter(|&end| deadline.is_none_or(|deadline| end < deadline));

    lock(mutex)?;

    // Re-check under the bucket lock. A waker that got here first has already
    // signalled, so without this check the wakeup could be missed.
    if atom.load(Ordering::Acquire) != val {
        let _ = unlock(mutex);
        return Err(Errno(EAGAIN));
    }

    if let Some(nanos) = slice.or(deadline)
        && arm_condvar_timeout(nanos).is_err()
    {
        let _ = unlock(mutex);
        return Err(Errno(EAGAIN));
    }

    // SyncCondvarWait releases the mutex and blocks atomically, and reacquires
    // it before returning — including on timeout or interruption.
    let result = unsafe { sync_condvar_wait(condvar, mutex) };
    let _ = unlock(mutex);

    match result {
        // The end of a slice is a spurious wakeup; the caller re-checks.
        Err(Errno(ETIMEDOUT)) if slice.is_none() && deadline.is_some() => Err(Errno(ETIMEDOUT)),
        // A signal, or any other early return, is a spurious wakeup as far as
        // the futex contract is concerned.
        _ => Ok(()),
    }
}

/// Wakes threads blocked on `addr`.
///
/// Buckets are shared between addresses, so a broadcast is issued whenever more
/// than one waiter is requested: waking a thread that was waiting on a
/// different address in the same bucket is harmless, whereas failing to wake
/// the right one is not.
///
/// # Safety
///
/// `addr` must point to a valid, aligned `u32`.
pub unsafe fn futex_wake(addr: *mut u32, num: u32) -> Result<u32> {
    let bucket = bucket_for(addr);
    let mutex = bucket.mutex.get();
    let condvar = bucket.condvar.get();

    lock(mutex)?;
    let result = unsafe { sync_condvar_signal(condvar, num > 1) };
    let _ = unlock(mutex);

    result.map(|()| num)
}

/// Address ranges shared with other processes, whose futex words are waited
/// on in slices.
///
/// A fixed table with lock-free lookup, since every wait consults it: a range
/// is published by storing its length after claiming a slot by its start, and
/// withdrawn in the reverse order. A range is registered before any pointer
/// into it is handed out and withdrawn after the last is given back, so a
/// lookup never races with a change to the range it is looking for.
pub mod shared {
    use core::sync::atomic::{AtomicUsize, Ordering};

    const SLOTS: usize = 64;

    struct Slot {
        start: AtomicUsize,
        len: AtomicUsize,
    }

    static SLOTS_TABLE: [Slot; SLOTS] = [const {
        Slot {
            start: AtomicUsize::new(0),
            len: AtomicUsize::new(0),
        }
    }; SLOTS];
    static IN_USE: AtomicUsize = AtomicUsize::new(0);

    /// Registers `[start, start + len)`; false if the table is full.
    pub fn register(start: usize, len: usize) -> bool {
        if start == 0 || len == 0 {
            return false;
        }
        for slot in &SLOTS_TABLE {
            if slot
                .start
                .compare_exchange(0, start, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                slot.len.store(len, Ordering::Release);
                IN_USE.fetch_add(1, Ordering::AcqRel);
                return true;
            }
        }
        false
    }

    /// Withdraws the range registered at `start`.
    pub fn unregister(start: usize) {
        for slot in &SLOTS_TABLE {
            if slot.start.load(Ordering::Acquire) == start && slot.len.load(Ordering::Acquire) != 0
            {
                slot.len.store(0, Ordering::Release);
                slot.start.store(0, Ordering::Release);
                IN_USE.fetch_sub(1, Ordering::AcqRel);
                return;
            }
        }
    }

    /// Whether `addr` lies in a registered range.
    pub fn contains(addr: usize) -> bool {
        if IN_USE.load(Ordering::Acquire) == 0 {
            return false;
        }
        SLOTS_TABLE.iter().any(|slot| {
            let start = slot.start.load(Ordering::Acquire);
            start != 0 && addr >= start && addr - start < slot.len.load(Ordering::Acquire)
        })
    }
}
