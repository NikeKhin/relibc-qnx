/* Shared memory by name, files that grow and shrink with ftruncate(), and
 * address space reserved without committing memory. */
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include "check.h"

int main(void)
{
    char name[64];
    snprintf(name, sizeof name, "/relibc-link-check-shm-%d", (int)getpid());
    shm_unlink(name);

    int fd = shm_open(name, O_CREAT | O_EXCL | O_RDWR, 0600);
    CHECK(fd >= 0);
    if (fd >= 0) {
        struct stat st;
        CHECK(ftruncate(fd, 8192) == 0 && fstat(fd, &st) == 0 && st.st_size == 8192);
        char *map = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        CHECK(map != MAP_FAILED);
        if (map != MAP_FAILED) {
            strcpy(map + 4096, "shared");
            CHECK(munmap(map, 8192) == 0);
        }
        int again = shm_open(name, O_RDWR, 0);
        CHECK(again >= 0);
        if (again >= 0) {
            char *view = mmap(NULL, 8192, PROT_READ, MAP_SHARED, again, 0);
            CHECK(view != MAP_FAILED && strcmp(view + 4096, "shared") == 0);
            if (view != MAP_FAILED)
                munmap(view, 8192);
            close(again);
        }
        CHECK(ftruncate(fd, 100) == 0 && fstat(fd, &st) == 0 && st.st_size == 100);
        close(fd);
        CHECK(shm_unlink(name) == 0);
        CHECK(shm_open(name, O_RDWR, 0) == -1 && errno == ENOENT);
    }

    /* 8 GiB of address space, inaccessible: commits nothing until a part of
     * it is made accessible and touched. */
    size_t reserve = (size_t)8 << 30;
    char *space = mmap(NULL, reserve, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    CHECK(space != MAP_FAILED);
    if (space != MAP_FAILED) {
        CHECK(mprotect(space, 65536, PROT_READ | PROT_WRITE) == 0);
        memset(space, 0x5a, 65536);
        CHECK(space[65535] == 0x5a);
        CHECK(munmap(space, reserve) == 0);
    }
    return finish("mman");
}
