/* Threads get the stack size they are created with, pthread_getattr_np()
 * reports the stack they run on, and timed waits time out. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <string.h>
#include <time.h>

#include "check.h"

#define STACK (4 * 1024 * 1024)

struct report {
    void *addr;
    size_t size;
    char *local;
    int deep;
};

/* Uses about `depth` KiB of stack; returns non-zero once at the bottom. */
static int __attribute__((noinline)) descend(int depth)
{
    volatile char frame[1024];
    memset((char *)frame, depth, sizeof frame);
    return depth == 0 ? frame[0] == 0 : descend(depth - 1) + (frame[1] != (char)depth);
}

static void *probe(void *arg)
{
    struct report *r = arg;
    pthread_attr_t attr;
    char local;
    r->local = &local;
    if (pthread_getattr_np(pthread_self(), &attr) == 0) {
        pthread_attr_getstack(&attr, &r->addr, &r->size);
        pthread_attr_destroy(&attr);
    }
    /* Three quarters of the requested size: far more than a default stack. */
    r->deep = descend(STACK / 1024 * 3 / 4);
    return NULL;
}

static void *quick(void *arg)
{
    return arg;
}

static struct timespec in(long ms)
{
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    t.tv_nsec += ms * 1000000L;
    t.tv_sec += t.tv_nsec / 1000000000L;
    t.tv_nsec %= 1000000000L;
    return t;
}

static pthread_mutex_t held = PTHREAD_MUTEX_INITIALIZER;
static int timedlock_result;

static void *try_lock(void *arg)
{
    (void)arg;
    struct timespec deadline = in(50);
    timedlock_result = pthread_mutex_timedlock(&held, &deadline);
    return NULL;
}

int main(void)
{
    struct report r = {0};
    pthread_attr_t attr;
    pthread_t thread;
    CHECK(pthread_attr_init(&attr) == 0);
    CHECK(pthread_attr_setstacksize(&attr, STACK) == 0);
    CHECK(pthread_create(&thread, &attr, probe, &r) == 0);
    CHECK(pthread_join(thread, NULL) == 0);
    pthread_attr_destroy(&attr);
    CHECK(r.size >= STACK);
    CHECK(r.local >= (char *)r.addr && r.local < (char *)r.addr + r.size);
    CHECK(r.deep == 1);

    /* Many short-lived threads, joined and detached: each exit releases its
     * stack, so this does not run out of memory. */
    int started = 0;
    for (int i = 0; i < 2000; i++) {
        pthread_attr_t small;
        pthread_attr_init(&small);
        pthread_attr_setstacksize(&small, 256 * 1024);
        if (pthread_create(&thread, &small, quick, NULL) == 0) {
            started++;
            if (i % 2)
                pthread_detach(thread);
            else
                pthread_join(thread, NULL);
        }
        pthread_attr_destroy(&small);
    }
    CHECK(started == 2000);

    pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
    pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
    struct timespec start, end, deadline = in(50);
    clock_gettime(CLOCK_REALTIME, &start);
    pthread_mutex_lock(&mutex);
    int rc = pthread_cond_timedwait(&cond, &mutex, &deadline);
    pthread_mutex_unlock(&mutex);
    clock_gettime(CLOCK_REALTIME, &end);
    CHECK(rc == ETIMEDOUT);
    CHECK(end.tv_sec - start.tv_sec < 5);

    pthread_mutex_lock(&held);
    CHECK(pthread_create(&thread, NULL, try_lock, NULL) == 0);
    CHECK(pthread_join(thread, NULL) == 0);
    pthread_mutex_unlock(&held);
    CHECK(timedlock_result == ETIMEDOUT);
    return finish("pthread");
}
