/* A plugin for dlopen.c: it uses libc, thread-local storage with an initial
 * value, a constructor and a destructor, and symbols of the program that
 * loads it. Built with -fPIC -shared against libc.so. */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Defined by the program, which exports them with -rdynamic. */
extern int host_value;
extern void host_note(const char *what);

static __thread int counter = 41;

int plugin_add(int a, int b)
{
    return a + b;
}

/* Allocated here and freed by the program: one libc, one heap. */
char *plugin_greet(const char *who)
{
    size_t n = strlen(who) + sizeof "hello ";
    char *s = malloc(n);
    if (s)
        snprintf(s, n, "hello %s", who);
    return s;
}

int plugin_tls_next(void)
{
    return ++counter;
}

/* Sets errno here for the program to read: one errno per thread, shared. */
int plugin_fail_close(void)
{
    return close(-1);
}

int plugin_host_value(void)
{
    return host_value;
}

__attribute__((constructor)) static void up(void)
{
    host_note("constructor");
}

__attribute__((destructor)) static void down(void)
{
    host_note("destructor");
}
