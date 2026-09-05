/* dlopen of a plugin built with -fPIC -shared against libc.so. Linked both as
 * a static PIE (-static-pie -rdynamic), where relibc's linker adopts the
 * program, and as a dynamically linked PIE, under relibc's own interpreter.
 * The plugin, libplugin.so, must be next to this program. */
#include <dlfcn.h>
#include <errno.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#include "check.h"

int host_value = 1234;
static int constructed, destructed;

void host_note(const char *what)
{
    if (strcmp(what, "constructor") == 0)
        constructed++;
    else if (strcmp(what, "destructor") == 0)
        destructed++;
}

static int (*tls_next)(void);
static int thread_value;

static void *in_thread(void *arg)
{
    (void)arg;
    thread_value = tls_next();
    return NULL;
}

int main(int argc, char **argv)
{
    (void)argc;
    char path[1024];
    const char *slash = strrchr(argv[0], '/');
    size_t dir = slash ? (size_t)(slash - argv[0]) + 1 : 0;
    if (dir + sizeof "libplugin.so" > sizeof path)
        return 1;
    memcpy(path, argv[0], dir);
    strcpy(path + dir, "libplugin.so");

    CHECK(dlopen("/no/such/plugin.so", RTLD_NOW) == NULL && dlerror() != NULL);

    void *plugin = dlopen(path, RTLD_NOW);
    if (!plugin) {
        const char *why = dlerror();
        printf("dlopen %s: %s\n", path, why ? why : "(no dlerror)");
    }
    CHECK(plugin != NULL);
    if (!plugin)
        return finish("dlopen");
    CHECK(constructed == 1);

    int (*add)(int, int) = (int (*)(int, int))dlsym(plugin, "plugin_add");
    char *(*greet)(const char *) = (char *(*)(const char *))dlsym(plugin, "plugin_greet");
    int (*fail_close)(void) = (int (*)(void))dlsym(plugin, "plugin_fail_close");
    int (*host)(void) = (int (*)(void))dlsym(plugin, "plugin_host_value");
    tls_next = (int (*)(void))dlsym(plugin, "plugin_tls_next");
    CHECK(add && greet && fail_close && host && tls_next);
    CHECK(dlsym(plugin, "no_such_symbol") == NULL);
    if (!(add && greet && fail_close && host && tls_next))
        return finish("dlopen");

    CHECK(add(2, 3) == 5);
    char *hello = greet("relibc");
    CHECK(hello && strcmp(hello, "hello relibc") == 0);
    free(hello);
    errno = 0;
    CHECK(fail_close() == -1 && errno == EBADF);
    CHECK(host() == 1234);

    /* The plugin's thread-local starts at its initial value in every thread. */
    CHECK(tls_next() == 42 && tls_next() == 43);
    pthread_t t;
    CHECK(pthread_create(&t, NULL, in_thread, NULL) == 0 && pthread_join(t, NULL) == 0);
    CHECK(thread_value == 42);
    CHECK(tls_next() == 44);

    /* The program itself, through dlopen(NULL). */
    void *self = dlopen(NULL, RTLD_NOW);
    CHECK(self != NULL && dlsym(self, "host_value") == (void *)&host_value);

    CHECK(dlclose(plugin) == 0);
    CHECK(destructed == 1);
    return finish("dlopen");
}
