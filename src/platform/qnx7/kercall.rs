//! Raw Neutrino kernel-call veneers.
//!
//! # Calling convention (AArch64)
//!
//! Arguments are passed in `x0`-`x7` following AAPCS64, the kernel call number
//! goes in `w8`, and the kernel is entered with `svc #0x51`.
//!
//! The kernel signals failure by *skipping an instruction* on the way back:
//! it resumes at `svc + 4` on success and at `svc + 8` on failure, with `x0`
//! holding the (positive) `errno`. Every vendor veneer has this shape:
//!
//! ```text
//! ConnectAttach_r:
//!     mov w8, #0x27
//!     svc #0x51
//!     ret             ; success resumes here
//!     neg x0, x0      ; failure resumes here
//!     ret
//! ```
//!
//! Rather than branching, the sequences below pre-load an error flag and let
//! the success path clear it, which needs exactly one instruction on each side
//! of the `svc`:
//!
//! ```text
//!     mov  wERR, #1
//!     svc  #0x51
//!     mov  wERR, #0   ; only executed when the kernel returns successfully
//! ```
//!
//! The kernel preserves every register except `x0`, but `x0`-`x17` are marked
//! as clobbered here because they are call-clobbered in the vendor ABI and
//! nothing guarantees the kernel leaves them alone.

use core::arch::asm;

use super::version::SVC_IMM;
use crate::{
    error::{Errno, Result},
    platform::types::*,
};

/// Turns a kernel-call return into a `Result`.
///
/// This is the single point where Neutrino error numbers become relibc ones,
/// so no caller can accidentally leak a QNX `errno` into the rest of the
/// library.
#[inline]
fn wrap(err: u32, ret: usize) -> Result<usize> {
    // The kernel is free to overwrite `tpidr_el0` on the way back from `svc`.
    // Relibc's ELF TLS and `Tcb::current` both live there; put ours back.
    super::tls::restore_tpidr();
    if err == 0 {
        Ok(ret)
    } else {
        Err(Errno(super::abi::errno::from_kernel(ret as c_int)))
    }
}

/// Issues a kernel call with no arguments.
///
/// # Safety
///
/// See [`kercall1`].
#[inline]
pub unsafe fn kercall0(nr: u32) -> Result<usize> {
    let ret: usize;
    let err: u32;
    unsafe {
        asm!(
            "mov {err:w}, #1",
            concat!("svc #{imm}"),
            "mov {err:w}, #0",
            imm = const SVC_IMM,
            err = out(reg) err,
            inlateout("x8") nr => _,
            lateout("x0") ret,
            lateout("x1") _,
            lateout("x2") _,
            lateout("x3") _,
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            lateout("x7") _,
            lateout("x9") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
        );
    }
    wrap(err, ret)
}

/// Issues a kernel call with one argument.
///
/// # Safety
///
/// The caller must pass arguments matching the kernel call `nr`, including the
/// validity and lifetime of any pointers.
#[inline]
pub unsafe fn kercall1(nr: u32, a0: usize) -> Result<usize> {
    let ret: usize;
    let err: u32;
    unsafe {
        asm!(
            "mov {err:w}, #1",
            concat!("svc #{imm}"),
            "mov {err:w}, #0",
            imm = const SVC_IMM,
            err = out(reg) err,
            inlateout("x8") nr => _,
            inlateout("x0") a0 => ret,
            lateout("x1") _,
            lateout("x2") _,
            lateout("x3") _,
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            lateout("x7") _,
            lateout("x9") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
        );
    }
    wrap(err, ret)
}

macro_rules! kercall_n {
    ($name:ident, $( $arg:ident : $reg:tt ),+ ; $( $clob:tt ),* ) => {
        /// Issues a kernel call. See the module documentation for the ABI.
        ///
        /// # Safety
        ///
        /// The caller must pass arguments matching the kernel call `nr`,
        /// including the validity and lifetime of any pointers.
        #[inline]
        pub unsafe fn $name(nr: u32, a0: usize $(, $arg: usize)+) -> Result<usize> {
            let ret: usize;
            let err: u32;
            unsafe {
                asm!(
                    "mov {err:w}, #1",
                    concat!("svc #{imm}"),
                    "mov {err:w}, #0",
                    imm = const SVC_IMM,
                    err = out(reg) err,
                    inlateout("x8") nr => _,
                    inlateout("x0") a0 => ret,
                    $( inlateout($reg) $arg => _, )+
                    $( lateout($clob) _, )*
                    lateout("x9") _,
                    lateout("x10") _,
                    lateout("x11") _,
                    lateout("x12") _,
                    lateout("x13") _,
                    lateout("x14") _,
                    lateout("x15") _,
                    lateout("x16") _,
                    lateout("x17") _,
                );
            }
            wrap(err, ret)
        }
    };
}

kercall_n!(kercall2, a1: "x1"; "x2", "x3", "x4", "x5", "x6", "x7");
kercall_n!(kercall3, a1: "x1", a2: "x2"; "x3", "x4", "x5", "x6", "x7");
kercall_n!(kercall4, a1: "x1", a2: "x2", a3: "x3"; "x4", "x5", "x6", "x7");
kercall_n!(kercall5, a1: "x1", a2: "x2", a3: "x3", a4: "x4"; "x5", "x6", "x7");
kercall_n!(kercall6, a1: "x1", a2: "x2", a3: "x3", a4: "x4", a5: "x5"; "x6", "x7");
kercall_n!(kercall7, a1: "x1", a2: "x2", a3: "x3", a4: "x4", a5: "x5", a6: "x6"; "x7");
