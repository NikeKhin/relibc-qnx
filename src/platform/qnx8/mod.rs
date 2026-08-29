//! QNX Neutrino 8.0 platform backend.
//!
//! Selected for `target_env = "nto80"`, at the same level as the 7.1 backend
//! in `../qnx7`. Most of the implementation is shared with it: the files
//! included below with `#[path]` are compiled a second time as this backend's
//! own modules, so their `super::` references resolve here and pick up the 8.0
//! versions of the three modules that differ:
//!
//! * [`version`] — the 8.0 kernel-call numbering, `svc` immediate, thread
//!   control block offsets, and the layouts and constants 8.0 changed
//!   (`sigaction`, `_msg_info64`, `_IO_CONNECT_EXTRA_LINK`, the pipe path).
//! * [`tls`] — 8.0 reaches the kernel's thread block through
//!   `[tpidr_el0 + 8]` instead of `tpidrro_el0`.
//! * `dupfd` — the 8.0 `_io_dup` and `_io_openfd` messages.
//!
//! A fix to any shared file in `../qnx7` applies to both backends.
//!
//! # Status
//!
//! The kernel ABI here is taken from the SDP 8.0 binaries, not from hardware.
//! Spawn and exec report `ENOSYS` until `_PROC_POSIX_SPAWN` is implemented.
//! See `QNX_PORT.md` for what is verified and what is not.

#[path = "../qnx7/abi/mod.rs"]
pub mod abi;
#[path = "../qnx7/connect.rs"]
mod connect;
#[path = "../qnx7/cshim.rs"]
mod cshim;
mod dupfd;
#[path = "../qnx7/epoll.rs"]
mod epoll;
#[path = "../qnx7/io.rs"]
pub(crate) mod io;
#[path = "../qnx7/gmon.rs"]
mod gmon;
#[path = "../qnx7/ioctl.rs"]
mod ioctl;
#[path = "../qnx7/kercall.rs"]
mod kercall;
#[path = "../qnx7/kernel.rs"]
mod kernel;
#[path = "../qnx7/link.rs"]
mod link;
#[path = "../qnx7/mem.rs"]
mod mem;
#[path = "../qnx7/pal.rs"]
mod pal;
#[path = "../qnx7/proc.rs"]
mod proc;
#[path = "../qnx7/ptrace.rs"]
mod ptrace;
#[path = "../qnx7/semaphore.rs"]
mod semaphore;
#[path = "../qnx7/signal.rs"]
mod signal;
#[path = "../qnx7/socket.rs"]
mod socket;
#[path = "../qnx7/startup.rs"]
mod startup;
#[path = "../qnx7/sync.rs"]
mod sync;
#[path = "../qnx7/thread.rs"]
mod thread;
#[path = "../qnx7/time.rs"]
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
