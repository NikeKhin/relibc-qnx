# relibc for QNX

This is an unofficial fork of [relibc](https://gitlab.redox-os.org/redox-os/relibc), the C library written in Rust for Redox OS, that adds support for QNX Neutrino 7.1 and 8.0 on AArch64. It is not affiliated with or endorsed by the Redox OS project or BlackBerry. For everything else about relibc, see the [upstream README](https://gitlab.redox-os.org/redox-os/relibc/-/blob/master/README.md).

## Why

QNX programs are normally built with the QNX Software Development Platform (SDP): its `qcc` compiler driver, linking against QNX's own C library. With relibc as the C library, they can be built with standard Arm toolchains instead:

- **Rust:** the nightly toolchain from rustup, with its `rust-lld` linker, builds relibc and Rust code that uses it.
- **C and C++:** an `aarch64-none-elf` GCC, such as the [Arm GNU Toolchain](https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads), compiles and links against relibc.

Nothing from the QNX SDP is needed to build. The programs run on an ordinary QNX system: relibc talks to the Neutrino kernel, the process manager and the resource managers directly, with the kernel calls and message formats QNX's own C library uses.

## What this fork adds

| Path | Contents |
| --- | --- |
| [`src/platform/qnx7`](src/platform/qnx7) | The QNX 7.1 backend, most of which 8.0 shares |
| [`src/platform/qnx8`](src/platform/qnx8) | The QNX 8.0 backend: the 7.1 files, with 8.0's kernel-call numbering and changed layouts, thread-local storage and `dup` |
| [`targets`](targets) | Rust target specs for both releases, a GCC specs file, and the linker script for static executables |
| [`qnx`](qnx) | Build, check and test scripts, with the extra start files and C++ runtime support (`crt`), a self-test (`selftest`) and `tools` |
| `build.rs`, `cbindgen.globdefs.toml`, elsewhere in `src` | Small changes that select the QNX backends for `target_os = "nto"` and give them relibc's Linux definitions |

Programs are compiled against relibc's headers. For QNX these keep the definitions of structures and constants relibc uses on Linux, and the backends translate them to QNX's at the kernel boundary. So objects and libraries compiled against QNX's own headers can't be linked in.

## Branches

- `qnx/main` (default): the QNX port, updated by merging upstream `master`.
- `master`: an unmodified mirror of upstream `master`.

## Requirements

- [rustup](https://rustup.rs), with the nightly that [`rust-toolchain.toml`](rust-toolchain.toml) names and its `rust-src` component:
  ```sh
  rustup toolchain install nightly-2026-05-24 --component rust-src
  ```
  The scripts use it even when another `cargo` comes first on `PATH`.
- [cbindgen](https://github.com/mozilla/cbindgen), which generates most of the C headers: `cargo install cbindgen`.
- An `aarch64-none-elf` GCC, such as the Arm GNU Toolchain (tested with 15.2.Rel1), with its `bin` directory on `PATH` or `GCC_PREFIX` set to its root directory. Its binutils finish `libc.a` and `libc.so`, and its GCC compiles and links C and C++ programs.
- Python 3 and git.

The sources include two submodules, `openlibm` and `dlmalloc-rs`:

```sh
git clone --recursive https://github.com/NikeKhin/relibc-qnx.git
```

## Building

```sh
qnx/build-qnx.sh                                                      # QNX 7.1
TARGET_SPEC=targets/aarch64-unknown-nto-qnx800.json qnx/build-qnx.sh  # QNX 8.0
```

The scripts run from any directory. Each of the QNX scripts works on the 7.1 build unless `TARGET_SPEC` names the 8.0 target spec, as above. A build leaves in `target/aarch64-unknown-nto-qnx710/release/` (`…qnx800…` for 8.0):

| File | Contents |
| --- | --- |
| `libc.a` | The static C library, position-independent |
| `libc.so` | The shared C library, which is also relibc's dynamic linker |
| `crt1.o`, `Scrt1.o`, `rcrt1.o`, `gcrt1.o`, `grcrt1.o`, `crti.o`, `crtn.o`, `crtbeginS.o` | Start files for each link mode |
| `cxxabi.o` | What C++ needs besides GCC's `libsupc++` |
| `sysroot/` | The libraries and start files in `aarch64le/lib`, with a copy of `libsupc++` for C++, and the headers in `usr/include` |

`libm`, `libpthread`, `libsocket` and the like are part of `libc`. The sysroot has empty archives with their names, so that `-lm` and similar options keep working.

## Building C and C++ programs

Compile against relibc's headers, then GCC's own, and not newlib's:

```sh
SYSROOT=<relibc>/target/aarch64-unknown-nto-qnx710/release/sysroot
CFLAGS="-D__QNXNTO__ -nostdinc -isystem $SYSROOT/usr/include \
  -isystem $(aarch64-none-elf-gcc -print-file-name=include) -mno-outline-atomics"
```

`__QNXNTO__` selects the QNX definitions in relibc's headers. `-mno-outline-atomics` inlines atomic operations, so that programs don't need libgcc's helpers for them, which can't detect LSE support on QNX.

To link, use the drivers `qnx/qnx-gcc` and `qnx/qnx-g++`, which wrap `aarch64-none-elf-gcc` and `aarch64-none-elf-g++`. A build system can use them as both compiler and linker. `TARGET_SPEC` selects the release they link for, or `RELIBC_OUT` any build directory:

```sh
<relibc>/qnx/qnx-gcc $CFLAGS -o hello hello.c
```

Or use GCC itself with the specs file:

```sh
export RELIBC_OUT=<relibc>/target/aarch64-unknown-nto-qnx710/release
aarch64-none-elf-gcc -specs=<relibc>/targets/aarch64-none-elf-relibc.specs $CFLAGS -o hello hello.c
```

Both pick the start files and the library by GCC's link options:

| Option | Result | Links |
| --- | --- | --- |
| none, or `-static` | Static executable | `libc.a` |
| `-static-pie` | Static position-independent executable, which relocates itself | `libc.a` |
| `-pie` | Dynamically linked executable | `libc.so` |
| `-shared` | Shared library | `libc.so` |
| `-p`, `-pg` | Profiling with gprof, for any of the executables above | |

Compile with `-fPIE` or `-fPIC` for `-static-pie`, `-pie` and `-shared`. `dlopen()` is supported in dynamically linked executables, and in static PIEs linked with `-rdynamic`.

A dynamically linked executable names `/usr/lib/ld-relibc.so.1` as its interpreter: copy `libc.so` there on the target, or pick another path with `-Wl,-dynamic-linker,PATH`. Its `libc.so.6` dependency is that interpreter itself. Other libraries are looked up in its `RUNPATH`, in `LD_LIBRARY_PATH` and in `/lib`.

### C++

C++ also needs GCC's C++ headers, ahead of relibc's, because they `#include_next` the C library's:

```sh
CXXINC=$(aarch64-none-elf-g++ -print-sysroot)/include/c++/$(aarch64-none-elf-g++ -dumpversion)
CXXFLAGS="-isystem $CXXINC -isystem $CXXINC/aarch64-none-elf $CFLAGS"
```

Link with `qnx/qnx-g++`, or with `aarch64-none-elf-g++` and the specs file, in any of the modes above. C++ programs get GCC's language support library, `libsupc++`: `new` and `delete`, exceptions, RTTI, function-local statics and `thread_local`. They don't get the C++ standard library (containers, strings, streams), because the bare-metal toolchain builds `libstdc++` for newlib.

That toolchain also builds `libsupc++` for a single thread. relibc's `cxxabi.o`, which C++ links include, replaces the parts that have to know about threads:
- each thread has its own exception state;
- threads that race to a function-local static initialize it once;
- `thread_local` destructors run when their thread exits.

`build-qnx.sh` puts a copy of `libsupc++` in the sysroot that position-independent links can use, and `crtbeginS.o` registers the unwind tables that exceptions need. Each shared library links its own copy of the C++ runtime, so an exception can't cross a shared library's boundary.

## Building Rust code

Rust code is built for the same target spec as relibc, as `no_std` code that calls relibc through `extern "C"`, with `core` and `alloc` built by `-Z build-std`. It links with `rust-lld` against the start files and `libc.a`. [`qnx/smoke-link.sh`](qnx/smoke-link.sh) is a complete example. Rust's `std` isn't supported: its QNX support is written for QNX's own C library.

## Checking a build

Without a QNX target:

| Command | Checks |
| --- | --- |
| `qnx/smoke-link.sh` | A minimal Rust program links against `libc.a` |
| `qnx/inspect-libc-so.sh` | `libc.so` has only relocations that relibc's dynamic linker applies |
| `python3 qnx/tools/qnx-link-check.py <sysroot>` | Every function the headers declare is in `libc.a` and `libc.so`, and the C and C++ test programs link in every mode, through both the drivers and the specs file |
| `qnx/check-linux.sh`, `qnx/check-redox.sh` | relibc still builds for Linux, and redox-rt for Redox |

On a QNX target:

- `qnx/build-selftest.sh` builds `qnx-selftest` next to `libc.a`, a static program that checks the backend layer by layer and prints one line per step. Copy it to the target and run it.
- `qnx/build-tests-qnx.sh` compiles relibc's C tests into `tests/build_qnx`. Copy that whole directory to the target and run `./run-on-qnx.sh` in it.
- The programs `qnx-link-check.py` links are tests as well. It leaves them in `link-check/`, next to the sysroot.

## Status

- Both releases build and pass the smoke, relocation and link checks. On-target testing is in progress; QNX 8.0 has not run on hardware yet.
- The target specs enable ARMv8.1's LSE atomics (`+lse`), so relibc needs an ARMv8.1-A or later core.
- `epoll`, `poll`, `ppoll`, `select` and `pselect` report `ENOSYS`. Neutrino has no `epoll`, and its native mechanism, `ionotify`, isn't used yet.
- `ptrace` reports `ENOSYS`, and so does `brk`, since Neutrino has no program break.
- On QNX 7.1, `sigaltstack()` can't install an alternate signal stack.
- On QNX 8.0, `posix_spawn()` and the `exec` functions report `ENOSYS` until spawning through `_PROC_POSIX_SPAWN` is implemented.
- C++ is checked only at link time so far. The link check's `cxx` program is its on-target test.
- The pseudo-terminal functions are untested.
- What upstream relibc lacks is missing here too, for example `times()`.

## Keeping up with upstream

```sh
git remote add upstream https://gitlab.redox-os.org/redox-os/relibc.git
git fetch upstream
git switch master && git merge --ff-only upstream/master
git switch qnx/main && git merge master
git submodule update --init --recursive
```

Then adapt the backends to any change in relibc's platform interface, and run the checks above. When upstream changes its `README.md`, the merge conflicts on it; keep this branch's version.

relibc's `Cargo.lock` pins some Redox-only crates as git dependencies on gitlab.redox-os.org. The QNX scripts point Cargo at stand-ins for them in `qnx/tools/redox-git-stubs`, so that it resolves the lockfile without fetching them, and build with `--locked`. If a merge changes those pins, the build stops until `python3 qnx/tools/refresh-redox-stubs.py` regenerates the stand-ins.

## License

relibc's MIT license applies to this fork, see [LICENSE](LICENSE).

QNX and Neutrino are trademarks of BlackBerry Limited. All other trademarks belong to their respective owners.
