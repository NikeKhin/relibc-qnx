//! QNX Neutrino 7.1 platform backend.
//!
//! # Layout
//!
//! * [`abi`] — error numbers, kernel types and message layouts. The struct
//!   definitions were extracted from the DWARF in the SDP's `libc.so.5.sym`
//!   with `qnx/tools/qnx-dwarf-types.py`.
//! * [`kercall`] — the AArch64 `svc #0x51` veneers.
//! * [`kernel`] — typed wrappers over the individual kernel calls.
//! * [`version`] — everything that changes between kernel releases: the
//!   kernel-call numbering, the `svc` immediate, the thread control block
//!   offsets, and the few layouts and constants 8.0 changed.
//! * `pal` — the [`Pal`](crate::platform::Pal) implementation.
//! * The remaining modules implement one area each of the platform layer.
//!
//! The QNX 8.0 backend in `../qnx8` compiles most of these files a second
//! time, as its own modules; it replaces `version`, `tls` and `dupfd` with 8.0
//! versions. A fix to any other file here therefore applies to both.
//!
//! # What is implemented
//!
//! The kernel-call layer is complete and the file, memory, clock, process,
//! socket, timer and credential paths are built on message layouts taken from
//! the vendor's own debug information. `epoll`, `ptrace` and `brk` report
//! `ENOSYS`. On a microkernel a malformed message to the wrong manager is far
//! worse than a clean failure.

pub mod abi;
mod connect;
mod cshim;
mod dupfd;
mod epoll;
pub(crate) mod io;
mod gmon;
mod ioctl;
mod kercall;
mod kernel;
mod link;
mod mem;
mod pal;
mod proc;
mod ptrace;
mod semaphore;
mod signal;
mod socket;
mod startup;
mod sync;
mod thread;
mod time;
pub(crate) mod tls;
pub mod version;

pub use self::pal::Sys;

// What relibc's core calls on QNX: `platform::init`, and the `ld_so`/`tcb`
// start-up hooks (see `startup.rs`).
pub use self::{
    link::init,
    startup::{activate_tls, own_thread_pointer, static_init},
    thread::{
        get_name as thread_get_name, main_thread_stack,
        record_main_thread_stack, set_name as thread_set_name,
    },
};

/// Neutrino's page size on AArch64.
const PAGE_SIZE: usize = 4096;
