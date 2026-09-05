//! `_start` for a static PIE (`rcrt1.o`), a profiled program (`gcrt1.o`) and
//! a profiled static PIE (`grcrt1.o`), `crtbeginS.o` for everything
//! position-independent, and `cxxabi.o` for C++; see `Cargo.toml`.
//!
//! Like core's crt0, each `_start` enters `relibc_start_v1` with the stack
//! the kernel left. A static PIE has no dynamic linker to relocate it, so
//! `_start` first applies its own relative relocations, in assembly, because
//! until they are applied no Rust code may load a pointer from memory. A
//! profiled program passes a `main` that starts gprof first.

#![no_std]
#![feature(linkage)]
#![cfg_attr(relibc_crt = "cxxabi", feature(thread_local))]

#[cfg(not(any(
    relibc_crt = "rcrt1",
    relibc_crt = "gcrt1",
    relibc_crt = "grcrt1",
    relibc_crt = "crtbeginS",
    relibc_crt = "cxxabi"
)))]
compile_error!(
    "build with --cfg relibc_crt=\"rcrt1\", \"gcrt1\", \"grcrt1\", \"crtbeginS\" or \"cxxabi\""
);

// crtbeginS.o, which GCC's bare-metal toolchain lacks: a shared object's or
// PIE's `__dso_handle`, the identity C++ registers its destructors under
// (`__cxa_atexit`), is its own address and hidden, so each object has its
// own; and the object's last destructor runs them (`__cxa_finalize`), when
// it is unloaded or the program exits.
//
// It also registers the object's unwind tables, as GCC's crtbegin.o does:
// the bare-metal libgcc unwinder that C++ exceptions use finds them only
// through `__register_frame_info`. `__EH_FRAME_BEGIN__` starts this object's
// .eh_frame, which is empty, so it is where the linker puts the next
// object's; crtend.o ends them. The unwinder is linked only into programs
// that unwind, so it is a weak reference, and hidden: a program without it
// resolves it to null at link time, with no dynamic relocation for it.
#[cfg(relibc_crt = "crtbeginS")]
core::arch::global_asm!(
    ".pushsection .data.rel.ro.__dso_handle, \"aw\", %progbits",
    ".p2align 3",
    ".globl __dso_handle",
    ".hidden __dso_handle",
    ".type __dso_handle, %object",
    ".size __dso_handle, 8",
    "__dso_handle:",
    ".quad __dso_handle",
    ".popsection",
    ".pushsection .eh_frame, \"a\", %progbits",
    ".p2align 2",
    "__EH_FRAME_BEGIN__:",
    ".popsection",
    // libgcc's `struct object`, which the unwinder keeps the tables in.
    ".pushsection .bss.__relibc_qnx_eh_object, \"aw\", %nobits",
    ".p2align 3",
    "__relibc_qnx_eh_object:",
    ".zero 64",
    ".popsection",
    ".weak __register_frame_info",
    ".hidden __register_frame_info",
    ".weak __deregister_frame_info",
    ".hidden __deregister_frame_info",
    ".pushsection .init_array, \"aw\", %init_array",
    ".p2align 3",
    ".quad __relibc_qnx_register_frames",
    ".popsection",
    ".pushsection .text.__relibc_qnx_register_frames, \"ax\", %progbits",
    ".p2align 2",
    ".type __relibc_qnx_register_frames, %function",
    "__relibc_qnx_register_frames:",
    "adrp x16, :got:__register_frame_info",
    "ldr x16, [x16, :got_lo12:__register_frame_info]",
    "cbz x16, 1f",
    "adrp x0, __EH_FRAME_BEGIN__",
    "add x0, x0, :lo12:__EH_FRAME_BEGIN__",
    "adrp x1, __relibc_qnx_eh_object",
    "add x1, x1, :lo12:__relibc_qnx_eh_object",
    "br x16",
    "1:",
    "ret",
    ".size __relibc_qnx_register_frames, . - __relibc_qnx_register_frames",
    ".popsection",
    ".pushsection .fini_array, \"aw\", %fini_array",
    ".p2align 3",
    ".quad __relibc_qnx_finalize_dso",
    ".popsection",
    ".pushsection .text.__relibc_qnx_finalize_dso, \"ax\", %progbits",
    ".p2align 2",
    ".type __relibc_qnx_finalize_dso, %function",
    "__relibc_qnx_finalize_dso:",
    "stp x29, x30, [sp, #-16]!",
    "mov x29, sp",
    "adrp x0, __dso_handle",
    "add x0, x0, :lo12:__dso_handle",
    "bl __cxa_finalize",
    "adrp x16, :got:__deregister_frame_info",
    "ldr x16, [x16, :got_lo12:__deregister_frame_info]",
    "cbz x16, 1f",
    "adrp x0, __EH_FRAME_BEGIN__",
    "add x0, x0, :lo12:__EH_FRAME_BEGIN__",
    "blr x16",
    "1:",
    "ldp x29, x30, [sp], #16",
    "ret",
    ".size __relibc_qnx_finalize_dso, . - __relibc_qnx_finalize_dso",
    ".popsection",
);

// cxxabi.o: what C++ needs from its runtime beyond what GCC's bare-metal
// toolchain gets right for relibc. That toolchain builds libsupc++ for a
// single thread, so its exception state is one for the process
// (eh_globals.o), its static-initialization guards do not lock (guard.o),
// and it queues thread_local destructors for exit (atexit_thread.o); these
// are per-thread or thread-safe replacements. It is linked as an object
// into every C++ program and library, ahead of libsupc++, so they always
// win over its members: qnx-g++ adds it, and so does the sysroot's
// libstdc++.a, a linker script, for g++ with the specs file.
#[cfg(relibc_crt = "cxxabi")]
mod cxxabi {
    use core::{
        ffi::{c_int, c_uint, c_void},
        ptr,
        sync::atomic::{AtomicU8, AtomicU32, Ordering},
    };

    // libsupc++'s default terminate handler (vterminate.o) prints through
    // newlib's stderr: it loads `_impure_ptr` and then the FILE * at offset
    // 24 of what that points to, `_stderr` in newlib's `struct _reent` on
    // AArch64. Pointing it 24 bytes before relibc's `stderr` makes that read
    // relibc's. Hidden, since libsupc++ is not position-independent and
    // reaches it PC-relative.
    core::arch::global_asm!(
        ".pushsection .data.rel.ro._impure_ptr, \"aw\", %progbits",
        ".p2align 3",
        ".globl _impure_ptr",
        ".hidden _impure_ptr",
        ".type _impure_ptr, %object",
        ".size _impure_ptr, 8",
        "_impure_ptr:",
        ".quad stderr - 24",
        ".popsection",
    );

    // GCC registers the destructors of a module's global objects with
    // __cxa_atexit under its __dso_handle, and relibc runs them only in
    // __cxa_finalize. crtbeginS.o calls that, but GCC's crtbegin.o, which a
    // static ET_EXEC starts with, does not; so this runs the module's own,
    // whichever crtbegin it has. A second call finds none left.
    core::arch::global_asm!(
        ".hidden __dso_handle",
        ".pushsection .fini_array, \"aw\", %fini_array",
        ".p2align 3",
        ".quad __relibc_qnx_cxx_finalize",
        ".popsection",
        ".pushsection .text.__relibc_qnx_cxx_finalize, \"ax\", %progbits",
        ".p2align 2",
        ".type __relibc_qnx_cxx_finalize, %function",
        "__relibc_qnx_cxx_finalize:",
        "adrp x0, __dso_handle",
        "add x0, x0, :lo12:__dso_handle",
        "b __cxa_finalize",
        ".size __relibc_qnx_cxx_finalize, . - __relibc_qnx_cxx_finalize",
        ".popsection",
    );

    /// libsupc++'s `__cxa_eh_globals`: the exceptions this thread is
    /// handling, and how many it has thrown and not yet caught.
    #[repr(C)]
    pub struct EhGlobals {
        caught_exceptions: *mut c_void,
        uncaught_exceptions: c_uint,
    }

    #[thread_local]
    static mut EH_GLOBALS: EhGlobals = EhGlobals {
        caught_exceptions: ptr::null_mut(),
        uncaught_exceptions: 0,
    };

    #[unsafe(no_mangle)]
    pub extern "C" fn __cxa_get_globals() -> *mut EhGlobals {
        &raw mut EH_GLOBALS
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __cxa_get_globals_fast() -> *mut EhGlobals {
        &raw mut EH_GLOBALS
    }

    /// Room for relibc's `pthread_mutex_t` and `pthread_cond_t`, 12 and 8
    /// bytes, whose static initializers are all zeros.
    #[repr(C, align(8))]
    struct Opaque([u8; 64]);

    static mut LOCK: Opaque = Opaque([0; 64]);
    static mut WAKE: Opaque = Opaque([0; 64]);

    unsafe extern "C" {
        fn pthread_mutex_lock(mutex: *mut Opaque) -> c_int;
        fn pthread_mutex_unlock(mutex: *mut Opaque) -> c_int;
        fn pthread_cond_wait(cond: *mut Opaque, mutex: *mut Opaque) -> c_int;
        fn pthread_cond_broadcast(cond: *mut Opaque) -> c_int;
        fn write(fd: c_int, buf: *const c_void, len: usize) -> isize;
        fn abort() -> !;
        fn __cxa_thread_atexit_impl(
            dtor: unsafe extern "C" fn(*mut c_void),
            obj: *mut c_void,
            dso: *mut c_void,
        );
    }

    static NEXT_THREAD: AtomicU32 = AtomicU32::new(1);

    #[thread_local]
    static mut THREAD: u32 = 0;

    /// A number for the calling thread, never zero.
    fn this_thread() -> u32 {
        unsafe {
            while THREAD == 0 {
                THREAD = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
            }
            THREAD
        }
    }

    /// A function-local static's guard, 64 bits. The compiler's own check
    /// reads byte 0 with acquire ordering and calls __cxa_guard_acquire
    /// while it is zero. Byte 1 says an initialization is running, and bytes
    /// 4 to 7 which thread runs it, so that a recursive one stops instead of
    /// waiting for itself.
    unsafe fn guard_parts(
        guard: *mut u64,
    ) -> (&'static AtomicU8, &'static AtomicU8, &'static AtomicU32) {
        let bytes = guard.cast::<u8>();
        unsafe {
            (
                AtomicU8::from_ptr(bytes),
                AtomicU8::from_ptr(bytes.add(1)),
                AtomicU32::from_ptr(bytes.add(4).cast()),
            )
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __cxa_guard_acquire(guard: *mut u64) -> c_int {
        let (done, running, runner) = unsafe { guard_parts(guard) };
        if done.load(Ordering::Acquire) != 0 {
            return 0;
        }
        let me = this_thread();
        unsafe { pthread_mutex_lock(&raw mut LOCK) };
        let initialize = loop {
            if done.load(Ordering::Acquire) != 0 {
                break 0;
            }
            if running.load(Ordering::Relaxed) == 0 {
                running.store(1, Ordering::Relaxed);
                runner.store(me, Ordering::Relaxed);
                break 1;
            }
            if runner.load(Ordering::Relaxed) == me {
                let msg =
                    b"terminate called: a function-local static's initialization used itself\n";
                unsafe {
                    write(2, msg.as_ptr().cast(), msg.len());
                    abort();
                }
            }
            unsafe { pthread_cond_wait(&raw mut WAKE, &raw mut LOCK) };
        };
        unsafe { pthread_mutex_unlock(&raw mut LOCK) };
        initialize
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __cxa_guard_release(guard: *mut u64) {
        let (done, running, runner) = unsafe { guard_parts(guard) };
        unsafe { pthread_mutex_lock(&raw mut LOCK) };
        running.store(0, Ordering::Relaxed);
        runner.store(0, Ordering::Relaxed);
        done.store(1, Ordering::Release);
        unsafe {
            pthread_cond_broadcast(&raw mut WAKE);
            pthread_mutex_unlock(&raw mut LOCK);
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __cxa_guard_abort(guard: *mut u64) {
        let (_, running, runner) = unsafe { guard_parts(guard) };
        unsafe { pthread_mutex_lock(&raw mut LOCK) };
        running.store(0, Ordering::Relaxed);
        runner.store(0, Ordering::Relaxed);
        unsafe {
            pthread_cond_broadcast(&raw mut WAKE);
            pthread_mutex_unlock(&raw mut LOCK);
        }
    }

    /// Registers a thread_local object's destructor with relibc, which runs
    /// it when the thread exits.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __cxa_thread_atexit(
        dtor: unsafe extern "C" fn(*mut c_void),
        obj: *mut c_void,
        dso: *mut c_void,
    ) -> c_int {
        unsafe { __cxa_thread_atexit_impl(dtor, obj, dso) };
        0
    }
}

#[cfg(any(relibc_crt = "rcrt1", relibc_crt = "gcrt1", relibc_crt = "grcrt1"))]
use core::ffi::{c_char, c_int};

// Applies the relocations of the static PIE loaded at `base`, whose dynamic
// section is at `dynamic` (both run-time addresses). A static PIE has only
// relative ones, in `DT_RELA` and `DT_RELR`; any other kind traps, rather
// than leaving a pointer wrong. Uses x0-x12 only, and no stack.
#[cfg(any(relibc_crt = "rcrt1", relibc_crt = "grcrt1"))]
core::arch::global_asm!(
    ".pushsection .text.__relibc_qnx_relocate_self, \"ax\", %progbits",
    ".p2align 2",
    ".type __relibc_qnx_relocate_self, %function",
    "__relibc_qnx_relocate_self:",
    "mov x2, xzr", // DT_RELA
    "mov x3, xzr", // DT_RELASZ
    "mov x4, xzr", // DT_RELR
    "mov x5, xzr", // DT_RELRSZ
    "1:",
    "ldp x6, x7, [x1], #16",
    "cbz x6, 2f",
    "cmp x6, #7",
    "csel x2, x7, x2, eq",
    "cmp x6, #8",
    "csel x3, x7, x3, eq",
    "cmp x6, #36",
    "csel x4, x7, x4, eq",
    "cmp x6, #35",
    "csel x5, x7, x5, eq",
    "b 1b",
    "2:",
    "cbz x2, 5f",
    "add x2, x2, x0",
    "add x3, x2, x3",
    "3:",
    "cmp x2, x3",
    "b.hs 5f",
    "ldp x6, x7, [x2]",
    "ldr x8, [x2, #16]",
    "add x2, x2, #24",
    "and x7, x7, #0xffffffff",
    "cbz x7, 3b",    // R_AARCH64_NONE
    "cmp x7, #1027", // R_AARCH64_RELATIVE
    "b.ne 4f",
    "add x8, x8, x0",
    "str x8, [x0, x6]",
    "b 3b",
    "4:",
    "brk #0x1",
    "5:",
    "cbz x4, 9f",
    "add x4, x4, x0",
    "add x5, x4, x5",
    "mov x9, xzr", // next address an address entry left off at
    "6:",
    "cmp x4, x5",
    "b.hs 9f",
    "ldr x6, [x4], #8",
    "tbnz x6, #0, 7f",
    "add x9, x6, x0", // an address entry: relocate it, then continue after it
    "ldr x7, [x9]",
    "add x7, x7, x0",
    "str x7, [x9], #8",
    "b 6b",
    "7:", // a bitmap entry: bit n relocates the word n-1 words after x9
    "mov x10, x9",
    "lsr x6, x6, #1",
    "8:",
    "cbz x6, 10f",
    "tbz x6, #0, 11f",
    "ldr x7, [x10]",
    "add x7, x7, x0",
    "str x7, [x10]",
    "11:",
    "lsr x6, x6, #1",
    "add x10, x10, #8",
    "b 8b",
    "10:",
    "add x9, x9, #504", // 63 words
    "b 6b",
    "9:",
    "ret",
    ".size __relibc_qnx_relocate_self, . - __relibc_qnx_relocate_self",
    ".popsection",
);

// _start for a static PIE: the ELF header is at link address 0, so its
// run-time address is the load bias.
#[cfg(any(relibc_crt = "rcrt1", relibc_crt = "grcrt1"))]
core::arch::global_asm!(
    ".globl _start",
    ".type _start, %function",
    "_start:",
    "mov x19, sp",
    "and sp, x19, #0xfffffffffffffff0",
    "adrp x0, __ehdr_start",
    "add x0, x0, :lo12:__ehdr_start",
    "adrp x1, _DYNAMIC",
    "add x1, x1, :lo12:_DYNAMIC",
    "bl __relibc_qnx_relocate_self",
    "mov x0, x19",
    "adrp x1, {main}",
    "add x1, x1, :lo12:{main}",
    "mov x29, xzr",
    "mov x30, xzr",
    "b relibc_start_v1",
    ".size _start, . - _start",
    main = sym program_main,
);

// _start for a profiled program that is not a static PIE.
#[cfg(relibc_crt = "gcrt1")]
core::arch::global_asm!(
    ".globl _start",
    ".type _start, %function",
    "_start:",
    "mov x0, sp",
    "and sp, x0, #0xfffffffffffffff0",
    "adrp x1, {main}",
    "add x1, x1, :lo12:{main}",
    "mov x29, xzr",
    "mov x30, xzr",
    "b relibc_start_v1",
    ".size _start, . - _start",
    main = sym program_main,
);

#[cfg(any(relibc_crt = "gcrt1", relibc_crt = "grcrt1"))]
#[allow(non_upper_case_globals)]
unsafe extern "C" {
    fn main(argc: isize, argv: *mut *mut c_char, envp: *mut *mut c_char) -> c_int;
    fn monstartup(lowpc: usize, highpc: usize);
    fn _mcleanup();
    fn atexit(f: unsafe extern "C" fn()) -> c_int;
    static __executable_start: u8;
    static etext: u8;
}

/// `main` for a profiled program: profiles its text until exit, as glibc's
/// gcrt1 and QNX's mcrt1 do.
#[cfg(any(relibc_crt = "gcrt1", relibc_crt = "grcrt1"))]
unsafe extern "C" fn program_main(
    argc: isize,
    argv: *mut *mut c_char,
    envp: *mut *mut c_char,
) -> c_int {
    unsafe {
        monstartup(
            &raw const __executable_start as usize,
            &raw const etext as usize,
        );
        atexit(_mcleanup);
        main(argc, argv, envp)
    }
}

#[cfg(relibc_crt = "rcrt1")]
unsafe extern "C" {
    #[link_name = "main"]
    fn program_main(argc: isize, argv: *mut *mut c_char, envp: *mut *mut c_char) -> c_int;
}

#[panic_handler]
#[linkage = "weak"]
pub unsafe fn rust_begin_unwind(_pi: &core::panic::PanicInfo) -> ! {
    loop {}
}
