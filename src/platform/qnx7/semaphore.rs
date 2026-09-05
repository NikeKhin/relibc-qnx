//! Named semaphores: `sem_open()`, `sem_close()` and `sem_unlink()`.
//!
//! relibc implements unnamed semaphores but declares the named ones without
//! implementing or exporting them, so the backend provides them, and
//! `qnx/tools/qnx-headers/semaphore.h` their declarations.
//!
//! A named semaphore is relibc's own semaphore at the start of a one-page
//! shared memory object `/dev/shmem/sem.NAME` — QNX's shared memory namespace,
//! with the `sem.` prefix Linux uses in `/dev/shm` — so any process can open it
//! by name, and relibc's `sem_wait`, `sem_post` and the rest work on it
//! unchanged. Its range is registered with the futex, which then lets a
//! waiter see a post from another process (see `sync`).
//!
//! The creator makes the object with `O_EXCL`, sizes and initialises it, and
//! only then sets its ready word; a process opening an existing one waits for
//! that, so nobody ever uses a semaphore that is not initialised yet.

use alloc::vec::Vec;
use core::{
    mem::size_of,
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use super::{sync::shared, time::sleep_nanos};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        errno::{EEXIST, EINVAL, ENAMETOOLONG, ENFILE, ENOENT},
        fcntl::{O_CLOEXEC, O_CREAT, O_EXCL, O_RDWR},
        limits::NAME_MAX,
        semaphore::{RlctSempahore, sem_t},
        sys_mman::{MAP_SHARED, PROT_READ, PROT_WRITE},
        sys_stat::stat,
    },
    out::Out,
    platform::{self, Pal, Sys, types::*},
    sync::Mutex,
};

const PREFIX: &[u8] = b"/dev/shmem/sem.";

/// Set in [`Shared::ready`] once the semaphore is initialised.
const READY: u32 = 0x5345_4d31;

/// How long `sem_open` waits for another process to finish creating the
/// semaphore it is opening, in steps of a millisecond.
const READY_WAIT_STEPS: u32 = 2000;

/// SEM_VALUE_MAX, Linux's.
const SEM_VALUE_MAX: c_uint = i32::MAX as c_uint;

/// The start of the shared object. `sem_t *` points at `sem`.
#[repr(C)]
struct Shared {
    sem: RlctSempahore,
    ready: AtomicU32,
}

/// A semaphore this process has open. `name` is empty once it is unlinked,
/// so a later `sem_open` of the name makes a new one.
struct Open {
    name: Vec<u8>,
    addr: usize,
    len: usize,
    refs: usize,
}

static OPEN: Mutex<Vec<Open>> = Mutex::new(Vec::new());

/// The object's path for a semaphore name: leading slashes are dropped, and
/// what remains must be one non-empty path component.
fn object_path(name: *const c_char) -> Result<(Vec<u8>, Vec<u8>)> {
    if name.is_null() {
        return Err(Errno(EINVAL));
    }
    // SAFETY: the caller passes a NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    let base = &bytes[bytes.iter().take_while(|&&b| b == b'/').count()..];
    if base.is_empty() || base == b"." || base == b".." || base.contains(&b'/') {
        return Err(Errno(EINVAL));
    }
    if base.len() + b"sem.".len() > NAME_MAX {
        return Err(Errno(ENAMETOOLONG));
    }
    let mut path = Vec::with_capacity(PREFIX.len() + base.len() + 1);
    path.extend_from_slice(PREFIX);
    path.extend_from_slice(base);
    path.push(0);
    Ok((base.to_vec(), path))
}

fn c_path(path: &[u8]) -> CStr<'_> {
    CStr::from_bytes_with_nul(path).expect("path is NUL-terminated")
}

/// Maps an open object's first page.
fn map(fd: c_int, len: usize) -> Result<*mut Shared> {
    let addr = unsafe {
        Sys::mmap(
            ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            fd,
            0,
        )
    }?;
    Ok(addr.cast::<Shared>())
}

/// Makes the semaphore in an object this process just created.
fn create(fd: c_int, path: &[u8], len: usize, value: c_uint) -> Result<*mut Shared> {
    let made = Sys::ftruncate(fd, len as off_t).and_then(|()| map(fd, len));
    let shared = match made {
        Ok(shared) => shared,
        Err(err) => {
            let _ = Sys::unlink(c_path(path));
            return Err(err);
        }
    };
    // SAFETY: a fresh, page-aligned, writable mapping of `len` bytes, which no
    // other process uses before `ready` is set.
    unsafe {
        shared.write(Shared {
            sem: RlctSempahore::new(value),
            ready: AtomicU32::new(0),
        });
        (*shared).ready.store(READY, Ordering::Release);
    }
    Ok(shared)
}

/// Maps an existing semaphore once its creator has finished making it.
fn attach(fd: c_int, len: usize) -> Result<*mut Shared> {
    let mut steps = 0;
    loop {
        let mut st = stat::default();
        Sys::fstat(fd, Out::from_mut(&mut st))?;
        if st.st_size >= size_of::<Shared>() as off_t {
            break;
        }
        steps += 1;
        if steps > READY_WAIT_STEPS {
            return Err(Errno(EINVAL));
        }
        sleep_nanos(1_000_000);
    }
    let shared = map(fd, len)?;
    // SAFETY: the object is at least as large as `Shared`, and `ready` is only
    // ever accessed atomically.
    let ready = unsafe { &(*shared).ready };
    while ready.load(Ordering::Acquire) != READY {
        steps += 1;
        if steps > READY_WAIT_STEPS {
            let _ = unsafe { Sys::munmap(shared.cast(), len) };
            return Err(Errno(EINVAL));
        }
        sleep_nanos(1_000_000);
    }
    Ok(shared)
}

fn open(name: *const c_char, oflag: c_int, mode: mode_t, value: c_uint) -> Result<*mut sem_t> {
    let (base, path) = object_path(name)?;
    let create_excl = O_CREAT | O_EXCL;
    if oflag & O_CREAT != 0 && value > SEM_VALUE_MAX {
        return Err(Errno(EINVAL));
    }
    let mut open = OPEN.lock();
    if let Some(entry) = open.iter_mut().find(|entry| entry.name == base) {
        if oflag & create_excl == create_excl {
            return Err(Errno(EEXIST));
        }
        entry.refs += 1;
        return Ok(entry.addr as *mut sem_t);
    }

    let len = Sys::getpagesize();
    let mut shared = Err(Errno(ENOENT));
    // A semaphore unlinked between the two opens is simply made again.
    for _ in 0..4 {
        if oflag & O_CREAT != 0 {
            match Sys::open(c_path(&path), O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, mode) {
                Ok(fd) => {
                    shared = create(fd, &path, len, value);
                    let _ = Sys::close(fd);
                    break;
                }
                Err(Errno(EEXIST)) if oflag & O_EXCL == 0 => {}
                Err(err) => return Err(err),
            }
        }
        match Sys::open(c_path(&path), O_RDWR | O_CLOEXEC, 0) {
            Ok(fd) => {
                shared = attach(fd, len);
                let _ = Sys::close(fd);
                break;
            }
            Err(Errno(ENOENT)) if oflag & O_CREAT != 0 => {}
            Err(err) => return Err(err),
        }
    }
    let addr = shared? as usize;
    if !shared::register(addr, len) {
        let _ = unsafe { Sys::munmap(addr as *mut c_void, len) };
        return Err(Errno(ENFILE));
    }
    open.push(Open {
        name: base,
        addr,
        len,
        refs: 1,
    });
    Ok(addr as *mut sem_t)
}

fn close(sem: *mut sem_t) -> Result<()> {
    let mut open = OPEN.lock();
    let index = open
        .iter()
        .position(|entry| entry.addr == sem as usize)
        .ok_or(Errno(EINVAL))?;
    open[index].refs -= 1;
    if open[index].refs == 0 {
        let entry = open.swap_remove(index);
        shared::unregister(entry.addr);
        let _ = unsafe { Sys::munmap(entry.addr as *mut c_void, entry.len) };
    }
    Ok(())
}

fn unlink(name: *const c_char) -> Result<()> {
    let (base, path) = object_path(name)?;
    Sys::unlink(c_path(&path))?;
    for entry in OPEN.lock().iter_mut().filter(|entry| entry.name == base) {
        entry.name.clear();
    }
    Ok(())
}

fn fail<T>(err: Errno, value: T) -> T {
    platform::ERRNO.set(err.0);
    value
}

/// `sem_open()`. With `O_CREAT`, the variadic arguments are the object's
/// `mode_t` and the semaphore's initial value.
///
/// # Safety
///
/// `name` must be a NUL-terminated string, and with `O_CREAT` both variadic
/// arguments must be passed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_open(name: *const c_char, oflag: c_int, mut args: ...) -> *mut sem_t {
    let (mode, value) = if oflag & O_CREAT != 0 {
        // SAFETY: the caller passes both with O_CREAT.
        unsafe { (args.next_arg::<mode_t>(), args.next_arg::<c_uint>()) }
    } else {
        (0, 0)
    };
    open(name, oflag, mode, value).unwrap_or_else(|err| fail(err, ptr::null_mut()))
}

/// `sem_close()`.
///
/// # Safety
///
/// `sem` must not be used once its last open is closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_close(sem: *mut sem_t) -> c_int {
    close(sem).map_or_else(|err| fail(err, -1), |()| 0)
}

/// `sem_unlink()`.
///
/// # Safety
///
/// `name` must be a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_unlink(name: *const c_char) -> c_int {
    unlink(name).map_or_else(|err| fail(err, -1), |()| 0)
}
