//! Start-up linking support: core's `platform::init`.
//!
//! [`init`] gives a statically linked program relibc's dynamic linker, so
//! that it can `dlopen`, when the program has the dynamic symbol table that
//! needs: a static PIE linked with `-rdynamic`.
//!
//! `dl_iterate_phdr` is relibc core's (`header::link`), which reports the
//! objects the dynamic linker holds.

use core::{ptr, slice};

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
    platform::{self, get_auxv},
    sync::mutex::Mutex,
};

/// `p_type` of the program header table's own entry.
const PT_PHDR: u32 = 6;

/// Core's `platform::init`: finds the main program's program headers and
/// load bias in the auxiliary vector, and adopts the program for `dlopen`.
///
/// The bias is zero for an `ET_EXEC` and the load address of a
/// position-independent executable. It is `AT_PHDR` minus the `p_vaddr` of
/// the `PT_PHDR` entry; without one the program is taken to be loaded at its
/// link address.
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
