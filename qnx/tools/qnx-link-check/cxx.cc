/* C++ with relibc: exceptions, RTTI and new and delete, and what relibc's
 * cxxabi.o replaces in GCC's single-threaded libsupc++. Exceptions are
 * thrown in several threads at once and each sees only its own; threads
 * racing to a function-local static initialize it once; thread_local
 * destructors run when a thread exits; global destructors run at exit;
 * the terminate handler prints to stderr; and a static whose initializer
 * uses itself stops the program instead of hanging it. */
#include <errno.h>
#include <exception>
#include <new>
#include <pthread.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <typeinfo>
#include <unistd.h>

#include "check.h"

#define THREADS 8

static void sleep_ms(long ms)
{
    struct timespec t = {ms / 1000, ms % 1000 * 1000000L};
    nanosleep(&t, nullptr);
}

struct Base {
    virtual ~Base() {}
    virtual int kind() const { return 1; }
};

struct Derived : Base {
    static int alive;
    Derived() { alive++; }
    ~Derived() override { alive--; }
    int kind() const override { return 2; }
};

int Derived::alive;

struct Guard {
    int *count;
    explicit Guard(int *c) : count(c) {}
    ~Guard() { ++*count; }
};

static int __attribute__((noinline)) dive(int depth, int *unwound)
{
    Guard g(unwound);
    if (depth == 0)
        throw Derived();
    return dive(depth - 1, unwound) + 1;
}

static void exceptions()
{
    int unwound = 0, kind = 0;
    try {
        dive(20, &unwound);
    } catch (const Base &e) {
        kind = e.kind();
    }
    CHECK(kind == 2);
    CHECK(unwound == 21);
    CHECK(Derived::alive == 0);

    int value = 0;
    try {
        try {
            throw 7;
        } catch (...) {
            std::exception_ptr p = std::current_exception();
            CHECK(p != nullptr);
            std::rethrow_exception(p);
        }
    } catch (int v) {
        value = v;
    }
    CHECK(value == 7);
    CHECK(std::uncaught_exceptions() == 0);
    CHECK(std::current_exception() == nullptr);

    bool bad_cast = false;
    Base plain;
    try {
        (void)dynamic_cast<Derived &>(plain);
    } catch (const std::bad_cast &) {
        bad_cast = true;
    }
    CHECK(bad_cast);
}

static void rtti_and_new()
{
    Base *b = new Derived;
    CHECK(Derived::alive == 1);
    CHECK(typeid(*b) == typeid(Derived));
    CHECK(strcmp(typeid(*b).name(), "7Derived") == 0);
    CHECK(dynamic_cast<Derived *>(b) != nullptr);
    delete b;
    CHECK(Derived::alive == 0);

    Derived *many = new Derived[5];
    CHECK(Derived::alive == 5);
    delete[] many;
    CHECK(Derived::alive == 0);

    int *p = new (std::nothrow) int[16];
    CHECK(p != nullptr);
    delete[] p;
}

static pthread_barrier_t start;
static int bad_values;

/* Each thread throws and catches its own exceptions, nested, while the
 * others do the same. */
static void *throw_in_parallel(void *arg)
{
    long id = (long)arg;
    pthread_barrier_wait(&start);
    for (int i = 0; i < 2000; i++) {
        try {
            try {
                throw (int)(id * 100000 + i);
            } catch (int v) {
                if (v != id * 100000 + i)
                    __atomic_add_fetch(&bad_values, 1, __ATOMIC_RELAXED);
                throw (long)v;
            }
        } catch (long v) {
            if (v != id * 100000 + i || std::uncaught_exceptions() != 0)
                __atomic_add_fetch(&bad_values, 1, __ATOMIC_RELAXED);
        }
    }
    return nullptr;
}

/* While one thread handles an exception, another has none. */
static int handling, checked, saw_foreign;

static void *handle_and_wait(void *)
{
    try {
        throw 1;
    } catch (int) {
        __atomic_store_n(&handling, 1, __ATOMIC_RELEASE);
        while (!__atomic_load_n(&checked, __ATOMIC_ACQUIRE))
            sleep_ms(1);
    }
    return nullptr;
}

static void *look_for_foreign(void *)
{
    while (!__atomic_load_n(&handling, __ATOMIC_ACQUIRE))
        sleep_ms(1);
    saw_foreign = std::current_exception() != nullptr || std::uncaught_exceptions() != 0;
    __atomic_store_n(&checked, 1, __ATOMIC_RELEASE);
    return nullptr;
}

static int initializations;

static int slow_init()
{
    __atomic_add_fetch(&initializations, 1, __ATOMIC_RELAXED);
    sleep_ms(20);
    return 42;
}

static int shared_value()
{
    static int value = slow_init();
    return value;
}

static int wrong_values;

static void *race_to_static(void *)
{
    pthread_barrier_wait(&start);
    if (shared_value() != 42)
        __atomic_add_fetch(&wrong_values, 1, __ATOMIC_RELAXED);
    return nullptr;
}

static int thread_dtors;

struct PerThread {
    int touched = 0;
    ~PerThread() { __atomic_add_fetch(&thread_dtors, 1, __ATOMIC_RELAXED); }
};

static thread_local PerThread per_thread;

static void *touch_thread_local(void *)
{
    per_thread.touched = 1;
    return nullptr;
}

static void threads()
{
    pthread_t t[THREADS];
    pthread_barrier_init(&start, nullptr, THREADS);
    for (long i = 0; i < THREADS; i++)
        CHECK(pthread_create(&t[i], nullptr, throw_in_parallel, (void *)i) == 0);
    for (int i = 0; i < THREADS; i++)
        pthread_join(t[i], nullptr);
    CHECK(bad_values == 0);

    pthread_t a, b;
    CHECK(pthread_create(&a, nullptr, handle_and_wait, nullptr) == 0);
    CHECK(pthread_create(&b, nullptr, look_for_foreign, nullptr) == 0);
    pthread_join(a, nullptr);
    pthread_join(b, nullptr);
    CHECK(saw_foreign == 0);

    for (int i = 0; i < THREADS; i++)
        CHECK(pthread_create(&t[i], nullptr, race_to_static, nullptr) == 0);
    for (int i = 0; i < THREADS; i++)
        pthread_join(t[i], nullptr);
    pthread_barrier_destroy(&start);
    CHECK(initializations == 1);
    CHECK(wrong_values == 0);

    for (int i = 0; i < THREADS; i++)
        CHECK(pthread_create(&t[i], nullptr, touch_thread_local, nullptr) == 0);
    for (int i = 0; i < THREADS; i++)
        pthread_join(t[i], nullptr);
    CHECK(thread_dtors == THREADS);
}

struct AtExit {
    int fd = -1;
    ~AtExit()
    {
        if (fd >= 0)
            (void)!write(fd, "d", 1);
    }
};

static AtExit at_exit;

static int recursive_init();

static int recursive_value()
{
    static int value = recursive_init();
    return value;
}

static int recursive_init()
{
    return recursive_value() + 1;
}

/* Runs `child` in a child process with its stderr on a pipe; returns what
 * it wrote there, up to `size` bytes, and its wait status. */
static int in_child(void (*child)(int), char *buf, size_t size, int *status)
{
    int fds[2];
    if (pipe(fds) != 0)
        return -1;
    pid_t pid = fork();
    if (pid == 0) {
        close(fds[0]);
        dup2(fds[1], 2);
        child(fds[1]);
        _exit(0);
    }
    close(fds[1]);
    size_t got = 0;
    ssize_t n;
    while (got + 1 < size && (n = read(fds[0], buf + got, size - 1 - got)) > 0)
        got += n;
    buf[got] = 0;
    close(fds[0]);
    if (pid < 0 || waitpid(pid, status, 0) != pid)
        return -1;
    return 0;
}

static void exit_with_global_destructor(int fd)
{
    at_exit.fd = fd;
    exit(0);
}

static void throw_uncaught(int)
{
    throw 42;
}

static void initialize_recursively(int)
{
    (void)recursive_value();
}

static void processes()
{
    char buf[256];
    int status = 0;

    CHECK(in_child(exit_with_global_destructor, buf, sizeof buf, &status) == 0);
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    CHECK(strcmp(buf, "d") == 0);

    CHECK(in_child(throw_uncaught, buf, sizeof buf, &status) == 0);
    CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGABRT);
    CHECK(strstr(buf, "terminate called after throwing an instance of 'int'") != nullptr);

    CHECK(in_child(initialize_recursively, buf, sizeof buf, &status) == 0);
    CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGABRT);
    CHECK(strstr(buf, "initialization used itself") != nullptr);
}

int main()
{
    exceptions();
    rtti_and_new();
    threads();
    processes();
    return finish("cxx");
}
