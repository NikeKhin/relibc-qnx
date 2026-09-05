/* Shared by the link-check programs, which are also tests for a QNX target:
 * each prints what failed and exits non-zero if anything did. */
#ifndef LINK_CHECK_H
#define LINK_CHECK_H

#include <stdio.h>

static int failures;

#define CHECK(cond)                                                    \
    do {                                                               \
        if (!(cond)) {                                                 \
            printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);     \
            failures++;                                                \
        }                                                              \
    } while (0)

static int finish(const char *what)
{
    if (failures)
        printf("%s: %d check(s) failed\n", what, failures);
    else
        printf("%s: all checks passed\n", what);
    return failures != 0;
}

#endif
