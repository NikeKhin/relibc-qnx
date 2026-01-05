//! `dl_iterate_phdr`, from `link.h`.
//!
//! This exists for one reason: `dl_iterate_phdr` is how a C++ or Rust
//! unwinder finds exception tables. libgcc's `_Unwind_*` machinery walks the
//! loaded objects looking for the `PT_GNU_EH_FRAME` segment that covers the
//! faulting address, and asks libc for that list. Without it, throwing across
//! a function boundary terminates the process.
//!
//! The main program's program headers come from the auxiliary vector, which
//! [`init`] (core's `platform::init`, run before any constructor) records.
//! Objects brought in by the
//! dynamic linker are enumerated from its object list; their headers are
//! derived from the ELF header at each load address, which is valid because the
//! first `PT_LOAD` of a shared object or PIE maps file offset zero.
//!
//! [`init`] also gives a statically linked program relibc's dynamic linker,
//! so that it can `dlopen`, when the program has the dynamic symbol table
//! that needs: a static PIE linked with `-rdynamic`.

use core::{
    ffi::c_void,
    ptr, slice,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};
use object::{
    NativeEndian, elf,
    read::elf::{Dyn as _, ProgramHeader as _},
};

use crate::{
    c_str::CStr,
    header::sys_auxv::{AT_PHDR, AT_PHENT, AT_PHNUM},
    ld_so::{
        dso::{Dyn, ProgramHeader},
        linker::{Config, Linker, Me},
        tcb::Tcb,
    },
    platform::{
        self, get_auxv,
        types::{c_char, c_int, size_t},
    },
    sync::mutex::Mutex,
};

/// One object's identity, as passed to a `dl_iterate_phdr` callback.
///
/// This is QNX's four-field `struct dl_phdr_info` from `sys/link.h`, not the
/// glibc one — it has no `dlpi_adds`, `dlpi_subs` or TLS fields. Matching it
/// matters because the consumer is usually the SDP's own `libgcc_s.so.1`,
/// which was compiled against that header.
#[repr(C)]
pub struct dl_phdr_info {
    /// Base address the object was loaded at.
    pub dlpi_addr: *mut c_void,
    /// Object name; the main program reports an empty string.
    pub dlpi_name: *const c_char,
    /// Pointer to the object's program headers.
    pub dlpi_phdr: *const c_void,
    /// Number of program headers.
    pub dlpi_phnum: u16,
}

static MAIN_PHDR: AtomicUsize = AtomicUsize::new(0);
static MAIN_PHNUM: AtomicUsize = AtomicUsize::new(0);
static MAIN_BIAS: AtomicUsize = AtomicUsize::new(0);

/// `p_type` of the program header table's own entry.
const PT_PHDR: u32 = 6;

/// Core's `platform::init`: records the main program's program headers and
/// load bias from the auxiliary vector, before any constructor can throw.
///
/// The bias is what `dlpi_addr` reports: zero for an `ET_EXEC`, the load
/// address of a position-independent executable. It is `AT_PHDR` minus the
/// `p_vaddr` of the `PT_PHDR` entry; without one the program is taken to be
/// loaded at its link address.
///
/// # Safety
///
/// `auxvs` must be this process's auxiliary vector, whose `AT_PHDR` points at
/// `AT_PHNUM` mapped program headers of `AT_PHENT` bytes each.
pub unsafe fn init(auxvs: Box<[[usize; 2]]>) {
    let (Some(phdr), Some(phent), Some(phnum)) = (
        get_auxv(&auxvs, AT_PHDR),
        get_auxv(&auxvs, AT_PHENT),
        get_auxv(&auxvs, AT_PHNUM),
    ) else {
        return;
    };
    let mut bias = 0;
    for i in 0..phnum {
        let entry = phdr + i * phent;
        // SAFETY: as documented; `p_type` is the first word and `p_vaddr` the
        // third in an `Elf64_Phdr`.
        let p_type = unsafe { ptr::read_unaligned(entry as *const u32) };
        if p_type == PT_PHDR {
            let p_vaddr = unsafe { ptr::read_unaligned((entry + 16) as *const u64) } as usize;
            bias = phdr.wrapping_sub(p_vaddr);
            break;
        }
    }
    MAIN_PHDR.store(phdr, Ordering::Relaxed);
    MAIN_PHNUM.store(phnum, Ordering::Relaxed);
    MAIN_BIAS.store(bias, Ordering::Relaxed);
    if phent == size_of::<ProgramHeader>() {
        // SAFETY: as documented, with the entry size checked.
        let phdrs = unsafe { slice::from_raw_parts(phdr as *const ProgramHeader, phnum) };
        unsafe { adopt_for_dlopen(phdrs, bias) };
    }
}

/// Gives a statically linked program relibc's dynamic linker, with the
/// program as its root object, if the program can be one: it needs a
/// dynamic section, for the symbols a loaded library binds to, and the TLS
/// segment every relibc program has. Without the linker, `dlopen` fails.
///
/// # Safety
///
/// `phdrs` must be this program's mapped program headers, loaded with `bias`.
unsafe fn adopt_for_dlopen(phdrs: &'static [ProgramHeader], bias: usize) {
    let has = |p_type| phdrs.iter().find(|ph| ph.p_type(NativeEndian) == p_type);
    let (Some(dynamic), Some(_)) = (has(elf::PT_DYNAMIC), has(elf::PT_TLS)) else {
        return;
    };
    let Some(tcb) = (unsafe { Tcb::current() }) else {
        return;
    };
    if !tcb.linker_ptr.is_null() {
        return;
    }
    let start = bias.wrapping_add(dynamic.p_vaddr(NativeEndian) as usize) as *const Dyn;
    let count = dynamic.p_memsz(NativeEndian) as usize / size_of::<Dyn>();
    // SAFETY: `PT_DYNAMIC` describes the program's mapped dynamic section.
    let all = unsafe { slice::from_raw_parts(start, count) };
    // The program's constructors and destructors are run by its own start-up
    // and exit; the linker, which runs those of every object it holds, must
    // not run them again.
    let own = [
        elf::DT_INIT,
        elf::DT_FINI,
        elf::DT_INIT_ARRAY,
        elf::DT_INIT_ARRAYSZ,
        elf::DT_FINI_ARRAY,
        elf::DT_FINI_ARRAYSZ,
        elf::DT_PREINIT_ARRAY,
        elf::DT_PREINIT_ARRAYSZ,
    ];
    let dyns: Vec<Dyn> = all
        .iter()
        .take_while(|d| d.d_tag(NativeEndian) != u64::from(elf::DT_NULL))
        .filter(|d| {
            !own.iter()
                .any(|&tag| d.d_tag(NativeEndian) == u64::from(tag))
        })
        .copied()
        .collect();

    let me = Me {
        base: ptr::without_provenance(bias),
        phdrs,
        dyns: dyns.leak(),
    };
    let mut linker = Linker::new(me, Config::from_env(&linker_environment()));
    linker.adopt_static_program();
    // A loaded library's TLS is allocated by `__tls_get_addr` on first use,
    // which needs the thread's DTV to exist already, as relibc's ld.so creates
    // it for a dynamically linked program.
    let static_modules = tcb.masters().map_or(0, |masters| masters.len());
    tcb.setup_dtv(static_modules);
    tcb.linker_ptr = Box::into_raw(Box::new(Mutex::new(linker)));
}

/// The `LD_*` variables the linker reads, from `environ`.
fn linker_environment() -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    let mut entry = unsafe { platform::environ };
    while !entry.is_null() && !unsafe { *entry }.is_null() {
        // SAFETY: `environ` is a NULL-terminated array of C strings.
        let var = unsafe { CStr::from_ptr(*entry) }.to_bytes();
        if let Some(eq) = var.iter().position(|&b| b == b'=')
            && var.starts_with(b"LD_")
            && let (Ok(key), Ok(value)) =
                (str::from_utf8(&var[..eq]), str::from_utf8(&var[eq + 1..]))
        {
            vars.insert(String::from(key), String::from(value));
        }
        entry = unsafe { entry.add(1) };
    }
    vars
}

/// The offset of `e_phoff` in a 64-bit ELF header, and the field widths needed
/// to read the program header table out of a mapped object.
const E_PHOFF: usize = 0x20;
const E_PHNUM: usize = 0x38;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Derives an object's program header table from its mapped ELF header.
///
/// Returns `None` when the load address does not start with an ELF header,
/// which happens if the first `PT_LOAD` does not map file offset zero.
unsafe fn phdrs_from_base(base: *const u8) -> Option<(*const c_void, u16)> {
    if base.is_null() {
        return None;
    }
    // SAFETY: the caller passes a load address the linker mapped, so the first
    // page is readable.
    let magic = unsafe { ptr::read_unaligned(base.cast::<[u8; 4]>()) };
    if magic != ELF_MAGIC {
        return None;
    }
    let phoff = unsafe { ptr::read_unaligned(base.add(E_PHOFF).cast::<u64>()) } as usize;
    let phnum = unsafe { ptr::read_unaligned(base.add(E_PHNUM).cast::<u16>()) };
    if phoff == 0 || phnum == 0 {
        return None;
    }
    Some((unsafe { base.add(phoff) }.cast::<c_void>(), phnum))
}

/// See <https://man7.org/linux/man-pages/man3/dl_iterate_phdr.3.html>.
///
/// Iteration stops as soon as the callback returns a non-zero value, and that
/// value is returned; this is what lets an unwinder stop at the object holding
/// the address it is looking for.
///
/// The main program comes first, from the auxiliary vector, then every object
/// relibc's dynamic linker holds, in a dynamically linked program or a static
/// one that has `dlopen`ed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dl_iterate_phdr(
    callback: Option<
        unsafe extern "C" fn(info: *mut dl_phdr_info, size: size_t, data: *mut c_void) -> c_int,
    >,
    data: *mut c_void,
) -> c_int {
    let Some(callback) = callback else {
        return 0;
    };

    let report = |name: *const c_char, base: usize, phdr: *const c_void, phnum: u16| {
        let mut info = dl_phdr_info {
            dlpi_addr: ptr::without_provenance_mut(base),
            dlpi_name: name,
            dlpi_phdr: phdr,
            dlpi_phnum: phnum,
        };
        // SAFETY: `info` outlives the call, and the callback contract is the
        // caller's to uphold.
        unsafe { callback(&raw mut info, size_of::<dl_phdr_info>(), data) }
    };

    // The main program first, which is where most exception tables live.
    let phdr = MAIN_PHDR.load(Ordering::Relaxed);
    let phnum = MAIN_PHNUM.load(Ordering::Relaxed);
    let main_reported = phdr != 0 && phnum != 0;
    if main_reported {
        let result = report(
            c"".as_ptr(),
            MAIN_BIAS.load(Ordering::Relaxed),
            ptr::without_provenance(phdr),
            phnum as u16,
        );
        if result != 0 {
            return result;
        }
    }

    // Then everything the dynamic linker has loaded. A statically linked
    // program that cannot dlopen has no linker and stops here; one that can
    // has itself as the linker's first object, already reported.
    let Some(tcb) = (unsafe { Tcb::current() }) else {
        return 0;
    };
    if tcb.linker_ptr.is_null() {
        return 0;
    }
    let linker = unsafe { (*tcb.linker_ptr).lock() };
    // The object names the linker holds are Rust strings rather than
    // NUL-terminated ones, and an unwinder only needs the load address and the
    // headers, so the name is reported as empty rather than allocating here.
    for base in linker.loaded_object_bases() {
        if main_reported && base as usize == MAIN_BIAS.load(Ordering::Relaxed) {
            continue;
        }
        let Some((phdr, phnum)) = (unsafe { phdrs_from_base(base) }) else {
            continue;
        };
        let result = report(c"".as_ptr(), base as usize, phdr, phnum);
        if result != 0 {
            return result;
        }
    }

    0
}
