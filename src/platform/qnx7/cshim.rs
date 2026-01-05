//! Rust replacements for the `long double` helpers in `src/c`.
//!
//! relibc normally compiles `src/c/stdlib.c` with the target's C compiler to
//! get at `long double`, which Rust historically could not express. There is no
//! C compiler for Neutrino outside the QNX SDP, so building for QNX would
//! otherwise reintroduce exactly the dependency this port exists to remove.
//!
//! AArch64's `long double` is IEEE binary128, which nightly Rust does model as
//! `f128`, so the same conversions can be written here directly. The symbol
//! names and signatures match the C originals so that the rest of relibc (and
//! anything linking against the library) sees no difference.

use crate::platform::types::{c_char, c_double, c_longdouble};

/// Converts a `long double` to a `double`, as `relibc_ldtod` in `src/c`.
///
/// # Safety
///
/// `x` must point to a valid `c_longdouble`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_ldtod(x: *const c_longdouble) -> c_double {
    // SAFETY: the caller guarantees the pointer is valid; `c_longdouble` is the
    // bit pattern of an IEEE binary128 on this target.
    let bits = unsafe { x.read() };
    f128::from_bits(bits) as c_double
}

/// Converts a `double` to a `long double`, as `relibc_dtold` in `src/c`.
///
/// # Safety
///
/// `out` must point to writable storage for a `c_longdouble`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_dtold(val: c_double, out: *mut c_longdouble) {
    // SAFETY: the caller guarantees the pointer is valid for a write.
    unsafe { out.write((val as f128).to_bits()) }
}

/// `strtold()`.
///
/// The C original delegates to `strtod` and widens the result, losing the extra
/// precision binary128 could carry; this keeps that behaviour rather than
/// silently changing it on one platform.
///
/// # Safety
///
/// `nptr` must be a NUL-terminated string, and `endptr` must be null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtold(nptr: *const c_char, endptr: *mut *mut c_char) -> c_longdouble {
    // SAFETY: forwarded unchanged to relibc's own strtod.
    let value = unsafe { crate::header::stdlib::strtod(nptr, endptr) };
    (value as f128).to_bits()
}
