/* Named semaphores, which relibc did not provide: create and open by name,
 * the same address for every open in a process, O_EXCL, timeouts, a post
 * from another process, close and unlink. */
#include <errno.h>
#include <fcntl.h>
#include <semaphore.h>
#include <stdio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "check.h"

int main(void)
{
    char name[64];
    snprintf(name, sizeof name, "/relibc-link-check-%d", (int)getpid());
    sem_unlink(name);

    sem_t *sem = sem_open(name, O_CREAT | O_EXCL, 0600, 1);
    CHECK(sem != SEM_FAILED);
    if (sem == SEM_FAILED)
        return finish("semaphore");
    CHECK(sem_open(name, O_CREAT | O_EXCL, 0600, 1) == SEM_FAILED && errno == EEXIST);
    sem_t *again = sem_open(name, 0);
    CHECK(again == sem);
    sem_t *created = sem_open(name, O_CREAT, 0600, 7);
    CHECK(created == sem);

    int value = -1;
    CHECK(sem_getvalue(sem, &value) == 0 && value == 1);
    CHECK(sem_trywait(sem) == 0);
    CHECK(sem_trywait(sem) == -1 && errno == EAGAIN);

    /* A timed wait on an empty semaphore times out, and not much later. */
    struct timespec start, deadline, end;
    clock_gettime(CLOCK_REALTIME, &start);
    deadline = start;
    deadline.tv_nsec += 50000000L;
    deadline.tv_sec += deadline.tv_nsec / 1000000000L;
    deadline.tv_nsec %= 1000000000L;
    CHECK(sem_timedwait(sem, &deadline) == -1 && errno == ETIMEDOUT);
    clock_gettime(CLOCK_REALTIME, &end);
    CHECK(end.tv_sec - start.tv_sec < 5);

    /* Another process opens it by name and posts; this one is woken. */
    pid_t child = fork();
    if (child == 0) {
        sem_t *other = sem_open(name, 0);
        struct timespec pause = {0, 100000000L};
        nanosleep(&pause, NULL);
        _exit(other != SEM_FAILED && sem_post(other) == 0 && sem_close(other) == 0 ? 0 : 1);
    }
    CHECK(child > 0);
    if (child > 0) {
        CHECK(sem_wait(sem) == 0);
        int status = -1;
        CHECK(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    }

    CHECK(sem_post(sem) == 0 && sem_getvalue(sem, &value) == 0 && value == 1);
    CHECK(sem_close(created) == 0 && sem_close(again) == 0 && sem_close(sem) == 0);
    CHECK(sem_unlink(name) == 0);
    CHECK(sem_unlink(name) == -1 && errno == ENOENT);
    CHECK(sem_open(name, 0) == SEM_FAILED && errno == ENOENT);
    CHECK(sem_open("/no/slashes", O_CREAT, 0600, 0) == SEM_FAILED && errno == EINVAL);
    CHECK(sem_close(NULL) == -1 && errno == EINVAL);
    return finish("semaphore");
}
