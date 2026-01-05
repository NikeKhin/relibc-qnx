//! Memory mapping, which on Neutrino is a message to procnto's memory manager.

use core::ptr;

use super::{
    abi::{
        iomsg::{MemCtrl, MemMap, MemMapReply, mem, mem_ctrl},
        mman::{madv, map, mcl, to_qnx_map, to_qnx_msync, to_qnx_prot},
        neutrino::SYSMGR_COID,
    },
    kernel::msg_send_struct,
};
use crate::{
    error::{Errno, Result},
    header::{
        errno::{EINVAL, ENOMEM},
        sys_mman as re,
    },
    platform::types::*,
};

/// `mmap()`.
///
/// # Safety
///
/// The caller must uphold the usual `mmap` contract; a `MAP_FIXED` request can
/// replace any existing mapping at `addr`.
pub unsafe fn mmap(
    addr: *mut c_void,
    len: usize,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: off_t,
) -> Result<*mut c_void> {
    if len == 0 {
        return Err(Errno(EINVAL));
    }
    let mut qnx_flags = to_qnx_map(flags);
    let anonymous = qnx_flags & map::ANON != 0;
    // Programs reserve address space with inaccessible anonymous mappings,
    // for which Linux commits no memory. QNX allocates an anonymous mapping's
    // memory up front unless told to wait for the first access.
    if anonymous && prot == 0 && qnx_flags & map::TYPE == map::PRIVATE {
        qnx_flags |= map::LAZY;
    }
    let request = MemMap {
        type_: mem::MAP,
        zero: 0,
        reserved1: 0,
        addr: addr as u64,
        len: len as u64,
        prot: to_qnx_prot(prot),
        flags: qnx_flags,
        // An anonymous mapping still has to name a descriptor; QNX uses -1.
        fd: if anonymous { -1 } else { fd },
        preload: 0,
        align: 0,
        offset: if anonymous { 0 } else { offset as i64 },
        shmem_handle: 0,
        elf_pvaddr: 0,
    };
    let mut reply = MemMapReply::default();
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }?;
    // A memory manager that declines the request can answer without failing
    // the kernel call, leaving the reply zeroed. Returning that address
    // unchecked hands the allocator memory that is not backed by anything,
    // and the fault then surfaces far from its cause — which is exactly how a
    // bad flag turned into a crash inside `basename` during startup.
    if reply.addr == 0 {
        return Err(Errno(ENOMEM));
    }
    Ok(ptr::with_exposed_provenance_mut(reply.addr as usize))
}

/// Issues a `_MEM_CTRL` message.
fn mem_ctrl_msg(subtype: u16, addr: *const c_void, len: usize, flags: u32) -> Result<()> {
    let request = MemCtrl {
        type_: mem::CTRL,
        subtype,
        flags,
        addr: addr as u64,
        len: len as u64,
    };
    let mut reply = MemCtrl::default();
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }.map(|_| ())
}

/// `munmap()`.
///
/// # Safety
///
/// The range must not be referenced afterwards.
pub unsafe fn munmap(addr: *mut c_void, len: usize) -> Result<()> {
    mem_ctrl_msg(mem_ctrl::UNMAP, addr, len, 0)
}

/// `mprotect()`.
///
/// # Safety
///
/// Removing protections from memory that is still in use is the caller's
/// problem.
pub unsafe fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> Result<()> {
    mem_ctrl_msg(mem_ctrl::PROTECT, addr, len, to_qnx_prot(prot))
}

/// `msync()`.
///
/// # Safety
///
/// `addr` must name a mapped range.
pub unsafe fn msync(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> {
    mem_ctrl_msg(mem_ctrl::SYNC, addr, len, to_qnx_msync(flags))
}

/// `mlock()`.
///
/// # Safety
///
/// `addr` must name a mapped range.
pub unsafe fn mlock(addr: *const c_void, len: usize) -> Result<()> {
    mem_ctrl_msg(mem_ctrl::LOCK, addr, len, 0)
}

/// `munlock()`.
///
/// # Safety
///
/// `addr` must name a mapped range.
pub unsafe fn munlock(addr: *const c_void, len: usize) -> Result<()> {
    mem_ctrl_msg(mem_ctrl::UNLOCK, addr, len, 0)
}

/// `mlockall()`.
pub fn mlockall(flags: c_int) -> Result<()> {
    let mut qnx = 0;
    if flags & re::MCL_CURRENT != 0 {
        qnx |= mcl::CURRENT;
    }
    if flags & re::MCL_FUTURE != 0 {
        qnx |= mcl::FUTURE;
    }
    mem_ctrl_msg(mem_ctrl::LOCKALL, ptr::null(), 0, qnx)
}

/// `munlockall()`.
pub fn munlockall() -> Result<()> {
    mem_ctrl_msg(mem_ctrl::UNLOCKALL, ptr::null(), 0, 0)
}

/// `madvise()`/`posix_madvise()`.
///
/// # Safety
///
/// `addr` must name a mapped range.
pub unsafe fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> Result<()> {
    // relibc follows Linux's MADV_* numbering, which shares only MADV_NORMAL
    // with QNX's; anything unrecognised is passed as "no advice" rather than
    // being turned into a different hint.
    let qnx = match advice {
        re::MADV_NORMAL => madv::NORMAL,
        re::MADV_SEQUENTIAL => madv::SEQUENTIAL,
        re::MADV_RANDOM => madv::RANDOM,
        re::MADV_WILLNEED => madv::WILLNEED,
        re::MADV_DONTNEED => madv::DONTNEED,
        _ => madv::NORMAL,
    };
    mem_ctrl_msg(mem_ctrl::ADVISE, addr, len, qnx)
}

/// `mremap()`, emulated with mmap/copy/munmap because Neutrino has no equivalent.
///
/// # Safety
///
/// `addr` must name a mapped range of `len` bytes.
pub unsafe fn mremap(
    addr: *mut c_void,
    len: usize,
    new_len: usize,
    flags: c_int,
    _args: *mut c_void,
) -> Result<*mut c_void> {
    if new_len == 0 {
        return Err(Errno(EINVAL));
    }
    if new_len == len {
        return Ok(addr);
    }
    if new_len < len {
        // Neutrino munmap rounds the start address down to a page and the
        // length up. Releasing from a non-page-aligned `new_len` would unmap
        // the tail of the prefix dlmalloc still owns. Keep a whole number of
        // pages and only unmap the rest.
        let keep = new_len.next_multiple_of(4096).min(len);
        if keep < len {
            unsafe { munmap(addr.wrapping_byte_add(keep), len - keep) }?;
        }
        return Ok(addr);
    }
    if flags & re::MREMAP_MAYMOVE == 0 {
        return Err(Errno(ENOMEM));
    }
    let new = unsafe {
        mmap(
            ptr::null_mut(),
            new_len,
            re::PROT_READ | re::PROT_WRITE,
            re::MAP_PRIVATE | re::MAP_ANON,
            -1,
            0,
        )
    }?;
    unsafe { ptr::copy_nonoverlapping(addr.cast::<u8>(), new.cast::<u8>(), len) };
    let _ = unsafe { munmap(addr, len) };
    Ok(new)
}
