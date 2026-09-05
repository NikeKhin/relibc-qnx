/* math.h: the functions build-qnx.sh used to strip from libc.a, the
 * classification macros openlibm's math.h implemented with helper functions
 * relibc's Rust libm lacks, and the constants and types relibc's own math.h
 * left undefined. */
#include <math.h>

#include "check.h"

typedef double (*d1)(double);
typedef float (*f1)(float);
typedef double (*d2)(double, double);
typedef float (*f2)(float, float);

/* Through pointers, so that the program references each library function;
 * a call GCC can turn into an instruction would not. */
static d1 volatile dbl1[] = {cbrt, ceil, fabs, floor, rint, round, sqrt, trunc, nearbyint};
static f1 volatile flt1[] = {cbrtf, ceilf, fabsf, floorf, rintf, roundf, sqrtf, truncf, nearbyintf};
static d2 volatile dbl2[] = {copysign, fdim, fmax, fmin, fmod};
static f2 volatile flt2[] = {copysignf, fdimf, fmaxf, fminf, fmodf};
static double (*volatile dbl3)(double, double, double) = fma;
static float (*volatile flt3)(float, float, float) = fmaf;

int main(void)
{
    volatile double one = 1.0, zero = 0.0;
    volatile float onef = 1.0f, zerof = 0.0f;
    volatile long double onel = 1.0L, zerol = 0.0L;

    /* cbrt ceil fabs floor rint round sqrt trunc nearbyint */
    CHECK(dbl1[0](8.0 * one) == 2.0);
    CHECK(dbl1[1](2.25 * one) == 3.0);
    CHECK(dbl1[2](-2.25 * one) == 2.25);
    CHECK(dbl1[3](2.75 * one) == 2.0);
    CHECK(dbl1[4](2.5 * one) == 2.0);
    CHECK(dbl1[5](2.5 * one) == 3.0);
    CHECK(dbl1[6](2.25 * one) == 1.5);
    CHECK(dbl1[7](-2.75 * one) == -2.0);
    CHECK(dbl1[8](3.5 * one) == 4.0);
    CHECK(flt1[0](8.0f * onef) == 2.0f);
    CHECK(flt1[1](2.25f * onef) == 3.0f);
    CHECK(flt1[2](-2.25f * onef) == 2.25f);
    CHECK(flt1[3](2.75f * onef) == 2.0f);
    CHECK(flt1[4](2.5f * onef) == 2.0f);
    CHECK(flt1[5](2.5f * onef) == 3.0f);
    CHECK(flt1[6](2.25f * onef) == 1.5f);
    CHECK(flt1[7](-2.75f * onef) == -2.0f);
    CHECK(flt1[8](3.5f * onef) == 4.0f);

    /* copysign fdim fmax fmin fmod, and fma */
    CHECK(dbl2[0](3.0 * one, -zero) == -3.0);
    CHECK(dbl2[1](5.0 * one, 3.0) == 2.0);
    CHECK(dbl2[2](one, 2.0) == 2.0);
    CHECK(dbl2[3](one, 2.0) == 1.0);
    CHECK(dbl2[4](7.0 * one, 3.0) == 1.0);
    CHECK(flt2[0](3.0f * onef, -zerof) == -3.0f);
    CHECK(flt2[1](5.0f * onef, 3.0f) == 2.0f);
    CHECK(flt2[2](onef, 2.0f) == 2.0f);
    CHECK(flt2[3](onef, 2.0f) == 1.0f);
    CHECK(flt2[4](7.0f * onef, 3.0f) == 1.0f);
    CHECK(dbl3(2.0 * one, 3.0, 1.0) == 7.0);
    CHECK(flt3(2.0f * onef, 3.0f, 1.0f) == 7.0f);

    /* The classification macros, for each floating type. */
    double nan_d = zero / zero, inf_d = one / zero;
    float nan_f = zerof / zerof, inf_f = onef / zerof;
    long double nan_l = zerol / zerol, inf_l = onel / zerol;
    CHECK(isnan(nan_d) && isnan(nan_f) && isnan(nan_l));
    CHECK(!isnan(one) && !isnan(onef) && !isnan(onel));
    CHECK(isinf(inf_d) && isinf(inf_f) && isinf(inf_l) && !isinf(one));
    CHECK(isfinite(one) && isfinite(onef) && isfinite(onel));
    CHECK(!isfinite(inf_d) && !isfinite(nan_f) && !isfinite(inf_l));
    CHECK(isnormal(one) && isnormal(onef) && isnormal(onel) && !isnormal(zero));
    CHECK(signbit(-one) && signbit(-onef) && signbit(-onel) && signbit(-zero));
    CHECK(!signbit(one) && !signbit(onef) && !signbit(onel));
    CHECK(fpclassify(nan_d) == FP_NAN && fpclassify(inf_f) == FP_INFINITE);
    CHECK(fpclassify(zerol) == FP_ZERO && fpclassify(one) == FP_NORMAL);
    CHECK(fpclassify(one * 4.9e-324) == FP_SUBNORMAL);
    CHECK(isgreater(one, zero) && isgreaterequal(one, one) && isless(zero, one));
    CHECK(islessequal(zero, zero) && islessgreater(one, zero));
    CHECK(isunordered(nan_d, one) && !isunordered(one, zero));

    /* The constants and types. */
    CHECK(isnan(NAN) && isinf(INFINITY) && INFINITY > 0);
    CHECK(HUGE_VAL == inf_d && HUGE_VALF == inf_f && HUGE_VALL == inf_l);
    float_t ft = 1.5f;
    double_t dt = 2.5;
    CHECK(ft + dt == 4.0);
    CHECK((math_errhandling & (MATH_ERRNO | MATH_ERREXCEPT)) != 0);
    return finish("math");
}
