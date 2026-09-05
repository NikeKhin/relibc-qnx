/* clock_nanosleep(), which relibc did not provide: relative and absolute
 * sleeps on the monotonic and realtime clocks, and its error returns. */
#include <errno.h>
#include <time.h>

#include "check.h"

static long long nanos(const struct timespec *t)
{
    return (long long)t->tv_sec * 1000000000LL + t->tv_nsec;
}

static struct timespec after(clockid_t clock, long ms)
{
    struct timespec t;
    clock_gettime(clock, &t);
    t.tv_nsec += ms * 1000000L;
    t.tv_sec += t.tv_nsec / 1000000000L;
    t.tv_nsec %= 1000000000L;
    return t;
}

int main(void)
{
    struct timespec start, end;

    /* Absolute on the monotonic clock: returns at, not before, the time. */
    struct timespec until = after(CLOCK_MONOTONIC, 30);
    CHECK(clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &until, NULL) == 0);
    clock_gettime(CLOCK_MONOTONIC, &end);
    CHECK(nanos(&end) >= nanos(&until));
    CHECK(nanos(&end) - nanos(&until) < 1000000000LL);

    /* Absolute on the realtime clock. */
    until = after(CLOCK_REALTIME, 20);
    CHECK(clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, &until, NULL) == 0);
    clock_gettime(CLOCK_REALTIME, &end);
    CHECK(nanos(&end) >= nanos(&until));

    /* A time that has passed returns at once. */
    struct timespec past = {0, 0};
    clock_gettime(CLOCK_MONOTONIC, &start);
    CHECK(clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &past, NULL) == 0);
    clock_gettime(CLOCK_MONOTONIC, &end);
    CHECK(nanos(&end) - nanos(&start) < 100000000LL);

    /* Relative, with the remaining time reported as zero. */
    struct timespec rel = {0, 20000000}, rem = {9, 9};
    clock_gettime(CLOCK_MONOTONIC, &start);
    CHECK(clock_nanosleep(CLOCK_MONOTONIC, 0, &rel, &rem) == 0);
    clock_gettime(CLOCK_MONOTONIC, &end);
    CHECK(nanos(&end) - nanos(&start) >= 20000000LL);
    CHECK(rem.tv_sec == 0 && rem.tv_nsec == 0);

    /* Errors are returned, not left in errno. */
    struct timespec bad = {0, 1000000000L};
    errno = 0;
    CHECK(clock_nanosleep(CLOCK_MONOTONIC, 0, &bad, NULL) == EINVAL && errno == 0);
    CHECK(clock_nanosleep(CLOCK_THREAD_CPUTIME_ID, 0, &rel, NULL) == EINVAL);
    CHECK(clock_nanosleep(CLOCK_PROCESS_CPUTIME_ID, 0, &rel, NULL) == ENOTSUP);
    CHECK(clock_nanosleep(12345, 0, &rel, NULL) == EINVAL);
    return finish("time");
}
