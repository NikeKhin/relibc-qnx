
/* Completes relibc's math.h for its pure-Rust libm, which QNX builds use.
 * Its constants are openlibm's fallback for compilers without builtins, and
 * the __nan and HUGE_VAL that fallback needs are not defined, so these are
 * the compiler's, as openlibm has them with GCC. float_t, double_t and
 * math_errhandling are C99's, with openlibm's values. */
#ifndef _RELIBC_QNX_MATH_H
#define _RELIBC_QNX_MATH_H

#undef HUGE_VAL
#undef HUGE_VALF
#undef HUGE_VALL
#undef INFINITY
#undef NAN
#define HUGE_VAL __builtin_huge_val()
#define HUGE_VALF __builtin_huge_valf()
#define HUGE_VALL __builtin_huge_vall()
#define INFINITY __builtin_inff()
#define NAN __builtin_nanf("")

typedef float float_t;
typedef double double_t;

#define MATH_ERRNO 1
#define MATH_ERREXCEPT 2
#define math_errhandling MATH_ERREXCEPT

#endif /* _RELIBC_QNX_MATH_H */
