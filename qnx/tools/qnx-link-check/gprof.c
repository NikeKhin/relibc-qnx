/* -pg: a child runs profiled code and exits, which writes its gmon.out; the
 * parent checks the file is GNU gprof's format with the call arcs recorded.
 * GMON_OUT_PREFIX puts it at <prefix>.<pid>. The parent is profiled too, and
 * QNX restarts no call that SIGPROF interrupts, so it retries on EINTR. */
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "check.h"

static volatile unsigned long sink;

static unsigned long __attribute__((noinline)) leaf(unsigned long x)
{
    return x * 2654435761u;
}

static void __attribute__((noinline)) work(int n)
{
    for (int i = 0; i < n; i++)
        sink += leaf((unsigned long)i);
}

int main(void)
{
    char prefix[64];
    snprintf(prefix, sizeof prefix, "/tmp/relibc-gmon-%d", (int)getpid());
    setenv("GMON_OUT_PREFIX", prefix, 1);

    pid_t child = fork();
    if (child == 0) {
        work(200000);
        exit(0); /* _mcleanup runs at exit */
    }
    CHECK(child > 0);
    int status = -1;
    pid_t waited;
    while ((waited = waitpid(child, &status, 0)) == -1 && errno == EINTR)
        ;
    CHECK(waited == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);

    char path[96];
    snprintf(path, sizeof path, "%s.%d", prefix, (int)child);
    int fd = open(path, O_RDONLY);
    CHECK(fd >= 0);
    static unsigned char data[1 << 22];
    ssize_t n = 0;
    while (fd >= 0 && n < (ssize_t)sizeof data) {
        ssize_t got = read(fd, data + n, sizeof data - (size_t)n);
        if (got > 0)
            n += got;
        else if (got == 0 || errno != EINTR)
            break;
    }
    if (fd >= 0)
        close(fd);
    unlink(path);
    CHECK(n > 20 && memcmp(data, "gmon", 4) == 0 && data[4] == 1);

    /* Walk the records: one histogram, then arcs; leaf is called from work. */
    size_t at = 20, arcs = 0, hist = 0;
    unsigned long leaf_calls = 0;
    while (n > 0 && at < (size_t)n) {
        unsigned char tag = data[at++];
        if (tag == 0) {
            unsigned int bins;
            memcpy(&bins, data + at + 16, 4);
            at += 16 + 4 + 4 + 15 + 1 + (size_t)bins * 2;
            hist++;
        } else if (tag == 1) {
            unsigned long self;
            unsigned int count;
            memcpy(&self, data + at + 8, 8);
            memcpy(&count, data + at + 16, 4);
            if (self > (unsigned long)leaf && self < (unsigned long)leaf + 64)
                leaf_calls += count;
            at += 20;
            arcs++;
        } else {
            break;
        }
    }
    CHECK(hist == 1);
    CHECK(arcs >= 2);
    CHECK(leaf_calls == 200000);
    /* Not exit: that would write the parent's own gmon.out. */
    int failed = finish("gprof");
    fflush(stdout);
    _exit(failed);
}
