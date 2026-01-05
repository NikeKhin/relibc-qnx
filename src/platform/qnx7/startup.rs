//! The parts of process and thread start-up that relibc's core hands to the
//! QNX backend.
//!
//! Core's `ld_so::init` builds the main thread's static TLS block when the
//! thread pointer is unset, and `Tcb::os_arch_activate` installs it. Both
//! assume Linux: a thread pointer that is zero at entry, an auxiliary vector
//! that always carries `AT_PHDR`, and an executable that is not position
//! independent. None of that holds on Neutrino, so on `nto` core calls
//! [`own_thread_pointer`], [`static_init`] and [`activate_tls`] instead, each
//! through a single `cfg(target_os = "nto")` line.

use core::{
    mem, ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use object::{
    Endianness,
    elf::{self, ProgramHeader32, ProgramHeader64},
    read::elf::ProgramHeader,
};

use crate::{
    header::sys_auxv::{AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM},
    ld_so::tcb::{Master, Tcb},
    platform::{Pal, Sys},
    start::Stack,
};

/// Whether relibc has installed a thread pointer of its own.
///
/// Elsewhere a non-zero thread pointer at entry means a dynamic loader
/// already built the TLS block. Not on Neutrino: 7.1 publishes its thread
/// control block through `tpidrro_el0` and leaves `tpidr_el0` holding
/// whatever it held, and 8.0 points `tpidr_el0` at the kernel's own block,
/// which has no room for relibc's TLS. Only [`activate_tls`] sets this.
static TLS_ACTIVATED: AtomicBool = AtomicBool::new(false);

/// The thread pointer `ld_so::init` read, if relibc wrote it; otherwise
/// zero, so that it builds the TLS block.
pub fn own_thread_pointer(tp: usize) -> usize {
    if TLS_ACTIVATED.load(Ordering::Acquire) {
        tp
    } else {
        0
    }
}

/// `Tcb::os_arch_activate`: the AArch64 Linux sequence, plus what each
/// release needs around it (see `tls::prepare_abi_ptr` and
/// `tls::note_abi_ptr`).
///
/// # Safety
///
/// `tls_end` and `tls_len` must describe the thread's live TLS block, as they
/// do when core calls this.
pub unsafe fn activate_tls(tls_end: usize, tls_len: usize) {
    // Uses ABI page
    let abi_ptr = tls_end - tls_len - 16;
    super::tls::prepare_abi_ptr(abi_ptr);
    unsafe {
        ptr::write(abi_ptr as *mut usize, tls_end);
        core::arch::asm!("msr tpidr_el0, {}", "isb", in(reg) abi_ptr);
    }
    super::tls::note_abi_ptr(abi_ptr);
    TLS_ACTIVATED.store(true, Ordering::Release);
}

/// TLS room kept spare in a static program that can `dlopen`. A thread
/// created after a library with TLS is loaded copies the library's TLS image
/// into its static block, which every thread sizes like the main thread's.
const DLOPEN_TLS_SURPLUS: usize = 16 * 1024;

static mut STATIC_TCB_MASTER: Master = Master {
    ptr: ptr::null(),
    image_size: 0,
    segment_size: 0,
    offset: 0,
};

unsafe extern "C" {
    // rustc E0791: extern_weak statics must be a nullable pointer type.
    #[linkage = "extern_weak"]
    static __ehdr_start: Option<&'static u8>;
}

/// `ld_so::static_init` for Neutrino: builds the main thread's TLS block from
/// the program's `PT_TLS` segment.
///
/// It differs from core's in two ways, both down to how procnto starts a
/// program:
///
/// * the auxiliary vector of a static `ET_EXEC` without `PT_PHDR` may lack
///   `AT_PHDR`, so the program headers are then found through
///   `__ehdr_start`, which rust-lld always defines and GNU ld defines with
///   `targets/aarch64-qnx-static.ld`;
/// * the QNX toolchain links position-independent executables, and `PT_TLS`
///   holds a link-time address, so the TLS image is at `p_vaddr` plus the load
///   bias (`AT_PHDR` minus `PT_PHDR`'s `p_vaddr`).
#[inline(never)]
pub fn static_init(sp: &'static Stack) {
    const SIZEOF_PHDR64: usize = mem::size_of::<ProgramHeader64<Endianness>>();
    const SIZEOF_PHDR32: usize = mem::size_of::<ProgramHeader32<Endianness>>();

    let mut phdr_opt = None;
    let mut phent_opt = None;
    let mut phnum_opt = None;

    let mut auxv = sp.auxv();
    loop {
        let (kind, value) = unsafe { *auxv };
        if kind == AT_NULL {
            break;
        }

        match kind {
            AT_PHDR => phdr_opt = Some(value),
            AT_PHENT => phent_opt = Some(value),
            AT_PHNUM => phnum_opt = Some(value),
            _ => (),
        }

        auxv = unsafe { auxv.add(1) };
    }

    let (phdr, phent, phnum) = match (phdr_opt, phent_opt, phnum_opt) {
        (Some(phdr), Some(phent), Some(phnum)) => (phdr, phent, phnum),
        _ => phdrs_from_ehdr().expect(
            "failed to find AT_PHDR (and no usable __ehdr_start); \
             link with rust-lld or targets/aarch64-qnx-static.ld",
        ),
    };

    let read_ph = |i: usize| -> (u32, usize, usize, usize, usize) {
        let ph_addr = phdr + phent * i;
        let endian = Endianness::default();
        match phent {
            SIZEOF_PHDR64 => unsafe {
                let ph = &*(ph_addr as *const ProgramHeader64<Endianness>);
                (
                    ph.p_type(endian),
                    ph.p_vaddr(endian) as usize,
                    ph.p_filesz(endian) as usize,
                    ph.p_memsz(endian) as usize,
                    ph.p_align(endian) as usize,
                )
            },
            SIZEOF_PHDR32 => unsafe {
                let ph = &*(ph_addr as *const ProgramHeader32<Endianness>);
                (
                    ph.p_type(endian),
                    ph.p_vaddr(endian) as usize,
                    ph.p_filesz(endian) as usize,
                    ph.p_memsz(endian) as usize,
                    ph.p_align(endian) as usize,
                )
            },
            _ => panic!("unknown AT_PHENT size {}", phent),
        }
    };

    let load_bias = (0..phnum)
        .map(read_ph)
        .find(|&(p_type, ..)| p_type == elf::PT_PHDR)
        .map_or(0, |(_, p_vaddr, ..)| phdr.wrapping_sub(p_vaddr));
    // A dynamic section is what lets a static program dlopen (`link::init`).
    let surplus = if (0..phnum).any(|i| read_ph(i).0 == elf::PT_DYNAMIC) {
        DLOPEN_TLS_SURPLUS
    } else {
        0
    };

    for i in 0..phnum {
        let (p_type, p_vaddr, p_filesz, p_memsz, p_align) = read_ph(i);

        let page_size = Sys::getpagesize();
        let voff = p_vaddr % page_size;
        let vsize = (p_memsz + voff).div_ceil(page_size) * page_size;

        if p_type == elf::PT_TLS {
            let valign = if p_align > 0 {
                p_memsz.div_ceil(p_align) * p_align
            } else {
                p_memsz
            };

            unsafe {
                STATIC_TCB_MASTER.ptr = p_vaddr.wrapping_add(load_bias) as *const u8;
                STATIC_TCB_MASTER.image_size = p_filesz;
                STATIC_TCB_MASTER.offset = valign;

                let tcb = Tcb::new(vsize + surplus).expect("failed to allocate TCB");
                tcb.masters_ptr = ptr::addr_of_mut!(STATIC_TCB_MASTER);
                tcb.masters_len = mem::size_of::<Master>();
                tcb.copy_masters().expect("failed to copy TLS master data");
                tcb.activate();
            }

            //TODO: Warning on multiple TLS sections?
            return;
        }
    }
}

/// The program header table, read from the ELF header the linker mapped at
/// `__ehdr_start`.
fn phdrs_from_ehdr() -> Option<(usize, usize, usize)> {
    let ehdr = (unsafe { __ehdr_start })?;
    let base = ptr::from_ref(ehdr) as usize;

    // SAFETY: `__ehdr_start` is the first byte of the mapped ELF header, which
    // is at least the 64 bytes of an `Elf64_Ehdr` once its class is checked.
    let ident = unsafe { core::slice::from_raw_parts(base as *const u8, 6) };
    // ELF magic, then ELFCLASS64.
    if ident[..4] != [0x7f, b'E', b'L', b'F'] || ident[4] != 2 {
        return None;
    }
    let phoff = unsafe { ptr::read_unaligned((base + 32) as *const u64) } as usize;
    let phent = unsafe { ptr::read_unaligned((base + 54) as *const u16) } as usize;
    let phnum = unsafe { ptr::read_unaligned((base + 56) as *const u16) } as usize;
    Some((base + phoff, phent, phnum))
}
