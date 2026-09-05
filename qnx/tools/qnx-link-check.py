#!/usr/bin/env python3
"""Checks a QNX relibc build against its own headers and link modes, without
QNX.

    qnx-link-check.py SYSROOT [--out DIR] [--interp PATH]

SYSROOT is the sysroot build-qnx.sh lays out. With the aarch64-none-elf GCC
toolchain build-qnx.sh takes its binutils from, it proves:

* Every function the installed headers declare is defined in libc.a and
  exported by libc.so. Each header is compiled on its own, the way a QNX
  program would include it, and GCC's -aux-info lists every function it
  declares; inline definitions need no library code and are left out. A
  header that does not compile on its own is reported as well.

* The C programs in qnx/tools/qnx-link-check compile against the headers without
  implicit declarations and link in every mode GCC offers, through both the
  GCC specs file and the qnx-gcc driver: static (ET_EXEC), static PIE and
  dynamically linked PIE, with -pg for gprof.c. dlopen.c loads a plugin
  (plugin/plugin.c, built -fPIC -shared) as a static PIE and as a dynamically
  linked PIE. The C++ program, cxx.cc, links the same ways through g++ with
  the specs file and through qnx-g++.

* Each result is what its mode needs at run time, read back from the ELF
  file: a static PIE has its first segment at address 0 and only relative
  relocations, which is all rcrt1.o applies; a dynamically linked PIE names
  relibc's interpreter, needs libc.so.6 and nothing libc.so lacks; every
  dynamic relocation is a kind relibc's linker applies; and a plugin needs no
  symbol its host does not export. A C++ program has relibc's cxxabi.o in
  place of the libsupc++ members it replaces, and the unwind tables its
  crtbegin registers cover main and end where crtend ends them.

The linked programs are also tests: copied to a QNX target (they are left in
--out, by default next to SYSROOT) each prints what it checked and exits
non-zero on a failure. The dynamically linked ones name --interp as their
interpreter, /usr/lib/ld-relibc.so.1 by default, where libc.so has to be on
the target. Nothing runs here. Exits non-zero on any finding.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
# The relibc root: this file is in its qnx/tools/ directory.
TREE = os.path.normpath(os.path.join(HERE, "..", ".."))
PROGRAMS = os.path.join(HERE, "qnx-link-check")
SPECS = os.path.join(TREE, "targets", "aarch64-none-elf-relibc.specs")
STATIC_LD = os.path.join(TREE, "targets", "aarch64-qnx-static.ld")
INTERP = "/usr/lib/ld-relibc.so.1"

# Headers relibc installs for openlibm, which a build with its pure-Rust libm
# (math_libm, as for QNX) does not have — upstream's USE_RUST_LIBM alike.
OPENLIBM_ONLY = {"complex.h", "fenv.h"}

# Parts of other headers, not meant to be included on their own; what they
# declare is checked through the headers that include them.
INTERNAL = re.compile(r"^(bits/|dl-tls\.h$)")

# Declared by relibc's headers on every target but implemented on none, so
# not a gap of this port. Anything else missing fails the check.
UPSTREAM_GAPS = {
    "times": "src/header/mod.rs leaves `sys_times` commented out",
    "atomic_flag_clear": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
    "atomic_flag_clear_explicit": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
    "atomic_flag_test_and_set": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
    "atomic_flag_test_and_set_explicit": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
    "atomic_signal_fence": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
    "atomic_thread_fence": "GCC's <stdatomic.h>, a macro; the function is libatomic's",
}

# Declared by relibc's headers, defined in libc.a, and deliberately not
# exported by libc.so.
HIDDEN_HELPERS = {
    "__sigsetjmp_tail": "sigsetjmp's tail, which its assembly declares hidden",
}

# The dynamic relocations relibc's linker applies (src/ld_so/dso.rs), as
# readelf names them, and the only one rcrt1.o applies to a static PIE.
LINKER_RELOCS = {"NONE", "RELATIVE", "GLOB_DAT", "JUMP_SLOT", "ABS64", "COPY", "IRELATIVE",
                 "TLS_DTPMOD", "TLS_DTPMOD64", "TLS_DTPREL", "TLS_DTPREL64", "TLS_TPREL",
                 "TLS_TPREL64", "TLSDESC"}
STATIC_PIE_RELOCS = {"NONE", "RELATIVE"}

# Options every program compiles and links with, which need libc's support:
# the stack protector's canary, and the libraries other systems split out.
# The bare-metal GCC has no -pthread; qnx-gcc takes it.
COMMON_CFLAGS = ["-fstack-protector-strong"]
COMMON_LIBS = ["-lm", "-ldl", "-lpthread", "-lrt", "-lsocket", "-lutil", "-lcrypt"]
DRIVER_FLAGS = {"specs": [], "qnx-gcc": ["-pthread"]}

# What the libsupc++ members cxxabi.o replaces define, which a C++ program
# must not contain: they are built for a single thread.
SUPCXX_REPLACED = {
    "_ZN12_GLOBAL__N_1L10eh_globalsE": "eh_globals.o, one exception state for the process",
    "_ZTIN9__gnu_cxx20recursive_init_errorE": "guard.o, static-initialization guards without locking",
    "_ZN12_GLOBAL__N_1L13single_threadE": "atexit_thread.o, thread_local destructors queued for exit",
}

# Link modes: compile options, and options for the specs file and qnx-gcc.
MODES = {
    "static": ["-fno-pic"],
    "static-pie": ["-fPIE", "-static-pie"],
    "pie": ["-fPIE", "-pie"],
}

AUX = re.compile(r"^/\* (.+?):(\d+):([A-Z]+) \*/ (.*)$")
# The declared name is the first identifier that opens a parameter list, as
# opposed to a parenthesised declarator such as `void (*signal (int, ...`.
NAME = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*) \((?!\*)")


def find_tool(name, default):
    """`$name`, else `default` from $GCC_PREFIX/bin or PATH, as gcc-binutils.sh finds it."""
    path = os.environ.get("PATH", "")
    if os.environ.get("GCC_PREFIX"):
        path = os.path.join(os.environ["GCC_PREFIX"], "bin") + os.pathsep + path
    return os.environ.get(name) or shutil.which(default, path=path)


def tool(name, default):
    return (find_tool(name, default)
            or sys.exit(f"{default} not found: put the aarch64-none-elf GCC toolchain's "
                        f"bin directory on PATH, or set GCC_PREFIX or {name}"))


def run(cmd, env=None):
    return subprocess.run(cmd, capture_output=True, text=True, env=env)


def flags(cc, inc):
    gcc_inc = run([cc, "-print-file-name=include"]).stdout.strip()
    # The flags a program built against the sysroot uses: relibc's headers,
    # then GCC's own.
    return ["-D__QNXNTO__", "-nostdinc", "-isystem", inc, "-isystem", gcc_inc,
            "-mno-outline-atomics"]


def cxx_flags(cxx, inc):
    """As flags(), with GCC's C++ headers first: they #include_next the C
    library's."""
    sysroot = run([cxx, "-print-sysroot"]).stdout.strip()
    version = run([cxx, "-dumpversion"]).stdout.strip()
    machine = run([cxx, "-dumpmachine"]).stdout.strip()
    cxx_inc = os.path.join(sysroot, "include", "c++", version)
    return ["-isystem", cxx_inc, "-isystem", os.path.join(cxx_inc, machine),
            *flags(cxx, inc)]


def frame_problems(readelf, nm, path):
    """What keeps the unwind tables a C++ program's crtbegin registers from
    covering it: they start at __EH_FRAME_BEGIN__, on an entry, and end at
    crtend's zero terminator."""
    text = run([readelf, "-SW", path]).stdout
    m = re.search(r"\]\s+\.eh_frame\s+PROGBITS\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)", text)
    syms = {}
    for line in run([nm, path]).stdout.splitlines():
        parts = line.split()
        if len(parts) == 3:
            syms.setdefault(parts[2], int(parts[0], 16))
    if not m or "__EH_FRAME_BEGIN__" not in syms:
        return ["no .eh_frame or __EH_FRAME_BEGIN__"]
    start = syms["__EH_FRAME_BEGIN__"] - int(m.group(1), 16)
    entries = []
    for e in re.finditer(r"^([0-9a-f]{8}) (?:[0-9a-f]+ [0-9a-f]+ )?(CIE|FDE cie=[0-9a-f]+ "
                         r"pc=([0-9a-f]+)\.\.([0-9a-f]+)|ZERO terminator)",
                         run([readelf, "--debug-dump=frames", path]).stdout, re.M):
        lo, hi = (int(e.group(3), 16), int(e.group(4), 16)) if e.group(3) else (0, 0)
        entries.append((int(e.group(1), 16), e.group(2).split()[0], lo, hi))
    problems = []
    if start not in [off for off, _, _, _ in entries]:
        problems.append("__EH_FRAME_BEGIN__ is not on an .eh_frame entry")
    after = [e for e in entries if e[0] >= start]
    if not any(kind == "ZERO" for _, kind, _, _ in after):
        problems.append("no zero terminator after __EH_FRAME_BEGIN__")
    if not any(kind == "FDE" and lo <= syms.get("main", -1) < hi for _, kind, lo, hi in after):
        problems.append("the registered unwind tables do not cover main")
    return problems


def declared(cc, cflags, inc, header, tmp):
    """Functions `header` declares, or the compiler's complaint."""
    aux = os.path.join(tmp, "decls.aux")
    proc = subprocess.run([cc, *cflags, "-std=gnu11", "-fsyntax-only", "-aux-info", aux,
                           "-x", "c", "-"], input=f"#include <{header}>\n",
                          capture_output=True, text=True)
    if proc.returncode != 0:
        first = next((l for l in proc.stderr.splitlines() if "error" in l), proc.stderr.strip())
        return None, first
    names = {}
    with open(aux) as f:
        for line in f:
            m = AUX.match(line.strip())
            if not m:
                continue
            path, _, kind, decl = m.groups()
            if not os.path.realpath(path).startswith(os.path.realpath(inc) + os.sep):
                continue
            # Definitions, static functions and compiler builtins need no
            # library code.
            if kind.endswith("F") or re.match(r"(extern )?static\b", decl):
                continue
            m = NAME.search(decl)
            if m and not m.group(1).startswith("__builtin_"):
                rel = os.path.relpath(os.path.realpath(path), os.path.realpath(inc))
                names.setdefault(m.group(1), rel)
    return names, None


def defined(nm, archive):
    out = run([nm, "-g", "--defined-only", archive]).stdout
    syms = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[1] in "TWiDBRV":
            syms.add(parts[2])
    return syms


def check_headers(cc, nm, readelf, sysroot):
    inc = os.path.join(sysroot, "usr", "include")
    cflags = flags(cc, inc)
    syms = defined(nm, os.path.join(sysroot, "aarch64le", "lib", "libc.a"))
    exported = Elf(readelf, os.path.join(sysroot, "aarch64le", "lib", "libc.so")).exported
    unexported = {}
    headers = sorted(os.path.relpath(os.path.join(d, f), inc)
                     for d, _, fs in os.walk(inc) for f in fs if f.endswith(".h"))
    missing, known, broken, total = {}, {}, [], {}
    with tempfile.TemporaryDirectory() as tmp:
        for header in headers:
            if INTERNAL.match(header) or header in OPENLIBM_ONLY:
                continue
            names, error = declared(cc, cflags, inc, header, tmp)
            if names is None:
                broken.append((header, error))
                continue
            for name, origin in names.items():
                total[name] = origin
                if name in syms:
                    if name not in exported and name not in HIDDEN_HELPERS:
                        unexported.setdefault(origin, set()).add(name)
                    continue
                if name in UPSTREAM_GAPS:
                    known[name] = origin
                else:
                    missing.setdefault(origin, set()).add(name)
    print(f"headers: {len(headers)}, functions declared: {len(total)}, defined in libc.a: "
          f"{len(total) - len(known) - sum(map(len, missing.values()))}")
    for name in sorted(known):
        print(f"  not on any target ({known[name]}): {name}: {UPSTREAM_GAPS[name]}")
    for header, error in broken:
        print(f"  header does not compile on its own: {header}: {error}")
    for origin in sorted(missing):
        print(f"  declared in {origin} but not in libc.a: {' '.join(sorted(missing[origin]))}")
    for origin in sorted(unexported):
        print(f"  declared in {origin}, in libc.a, but not exported by libc.so: "
              f"{' '.join(sorted(unexported[origin]))}")
    return not broken and not missing and not unexported


class Elf:
    """What `readelf` reports about a linked file."""

    def __init__(self, readelf, path):
        text = run([readelf, "-hlWd", "-r", "--dyn-syms", path]).stdout
        self.type = re.search(r"Type:\s+(\w+)", text).group(1)
        self.phdrs = re.findall(r"^\s+([A-Z_]+|0x[0-9a-f]+)\s+0x[0-9a-f]+\s+0x([0-9a-f]+)",
                                text, re.M)
        m = re.search(r"Requesting program interpreter: ([^\]]+)\]", text)
        self.interp = m.group(1) if m else None
        self.needed = re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", text)
        self.textrel = "TEXTREL" in text
        self.relocs = set(re.findall(r"\sR_AARCH64_(\w+)", text))
        self.undefined, self.exported = set(), set()
        for line in text.split("Symbol table '.dynsym'")[-1].splitlines():
            parts = line.split()
            if len(parts) >= 8 and parts[0].endswith(":") and parts[7] != "":
                name = parts[7].split("@")[0]
                if parts[6] == "UND":
                    if parts[4] != "WEAK" and name:
                        self.undefined.add(name)
                elif parts[4] in ("GLOBAL", "WEAK") and parts[5] == "DEFAULT":
                    self.exported.add(name)

    def has(self, p_type):
        return any(t == p_type for t, _ in self.phdrs)

    def first_load(self):
        return next((int(v, 16) for t, v in self.phdrs if t == "LOAD"), None)


def mode_problems(elf, mode, libc):
    """What keeps `elf` from running as `mode` with relibc."""
    problems = []
    unsupported = elf.relocs - LINKER_RELOCS
    if unsupported:
        problems.append(f"relocations relibc's linker does not apply: {sorted(unsupported)}")
    if elf.textrel:
        problems.append("text relocations")
    if mode == "static":
        if elf.type != "EXEC" or elf.interp or elf.has("DYNAMIC"):
            problems.append("not a static ET_EXEC")
        if not (elf.has("PHDR") and elf.has("TLS")):
            problems.append("no PT_PHDR or PT_TLS, which relibc's start-up reads")
    elif mode == "static-pie":
        if elf.type != "DYN" or elf.interp or not elf.has("DYNAMIC"):
            problems.append("not a static PIE")
        if elf.first_load() != 0:
            problems.append("first PT_LOAD not at 0, which rcrt1.o takes the load bias from")
        if elf.relocs - STATIC_PIE_RELOCS:
            problems.append(f"relocations rcrt1.o does not apply: "
                            f"{sorted(elf.relocs - STATIC_PIE_RELOCS)}")
        if elf.undefined:
            problems.append(f"undefined: {sorted(elf.undefined)}")
    elif mode in ("pie", "shared"):
        if elf.type != "DYN":
            problems.append("not ET_DYN")
        if mode == "pie" and elf.interp != INTERP:
            problems.append(f"interpreter {elf.interp}, not {INTERP}")
        if elf.needed != ["libc.so.6"]:
            problems.append(f"DT_NEEDED {elf.needed}, not libc.so.6")
        lacking = elf.undefined - libc.exported
        if mode == "pie" and lacking:
            problems.append(f"needs what libc.so does not define: {sorted(lacking)}")
    return problems


def link(driver, cc, out_dir, mode_flags, objs, exe, extra=(), cxx=False):
    """Links with `cc` and the specs file, or with qnx-gcc; for C++, `cc` is
    g++ and the driver qnx-g++."""
    env = dict(os.environ, RELIBC_OUT=out_dir, QNX_REAL_CC=cc)
    if cxx:
        env["QNX_REAL_CXX"] = cc
    if "-pie" in mode_flags:
        extra = (*extra, f"-Wl,-dynamic-linker,{INTERP}")
    if driver == "specs":
        cmd = [cc, f"-specs={SPECS}", *mode_flags, *extra, *objs, "-o", exe]
        if not any(f in mode_flags for f in ("-static-pie", "-pie", "-shared")):
            cmd[2:2] = ["-T", STATIC_LD]
    else:
        qnx_driver = "qnx-g++" if cxx else "qnx-gcc"
        cmd = [os.path.join(TREE, "qnx", qnx_driver), *mode_flags, *extra, *objs, "-o", exe]
    return run(cmd, env=env)


def check_programs(cc, cxx, readelf, sysroot, out):
    inc = os.path.join(sysroot, "usr", "include")
    cflags = flags(cc, inc)
    lib = os.path.join(sysroot, "aarch64le", "lib")
    out_dir = os.path.dirname(os.path.abspath(sysroot))
    libc = Elf(readelf, os.path.join(lib, "libc.so"))
    ok = True

    problems = []
    if libc.type != "DYN" or libc.needed or libc.relocs - LINKER_RELOCS:
        problems.append("libc.so is not a self-contained shared object relibc can relocate")
    if problems:
        print(f"  libc.so: {'; '.join(problems)}")
        ok = False

    def compile_(src, obj, extra, driver="specs"):
        compiler = cc if driver == "specs" else os.path.join(TREE, "qnx", "qnx-gcc")
        proc = run([compiler, *cflags, "-std=gnu11", "-O1", "-Wall", "-Werror", *extra,
                    *DRIVER_FLAGS[driver], "-c", src, "-o", obj],
                   env=dict(os.environ, QNX_REAL_CC=cc))
        if proc.returncode != 0:
            print(f"  {os.path.basename(src)}: does not compile:\n{proc.stderr.strip()}")
        return proc.returncode == 0

    with tempfile.TemporaryDirectory() as tmp:
        plugin_src = os.path.join(PROGRAMS, "plugin", "plugin.c")
        plugin_obj = os.path.join(tmp, "plugin.o")
        plugin = None
        if compile_(plugin_src, plugin_obj, ["-fPIC", *COMMON_CFLAGS]):
            for driver in ("specs", "qnx-gcc"):
                so = os.path.join(tmp, f"libplugin-{driver}.so")
                proc = link(driver, cc, out_dir, ["-shared"], [plugin_obj], so)
                if proc.returncode != 0:
                    print(f"  plugin ({driver}): does not link:\n{(proc.stdout + proc.stderr).strip()}")
                    ok = False
                    continue
                elf = Elf(readelf, so)
                found = mode_problems(elf, "shared", libc)
                if found:
                    print(f"  plugin ({driver}): {'; '.join(found)}")
                    ok = False
                plugin = plugin or (so, elf)

        sources = sorted(f for f in os.listdir(PROGRAMS) if f.endswith(".c"))
        counts = {}
        for mode, mode_flags in MODES.items():
            compile_flags = [f for f in mode_flags if f.startswith("-f")]
            link_flags = [f for f in mode_flags if not f.startswith("-f")]
            for name in sources:
                src = os.path.join(PROGRAMS, name)
                base = name[:-2]
                extra_c = COMMON_CFLAGS + (["-pg"] if base == "gprof" else [])
                extra_l = ["-pg"] if base == "gprof" else []
                if base == "dlopen":
                    if mode == "static":
                        continue  # a static ET_EXEC has no symbol table to bind a plugin to
                    extra_l = ["-rdynamic"]
                for driver in ("specs", "qnx-gcc"):
                    obj = os.path.join(tmp, f"{base}-{mode}-{driver}.o")
                    if not compile_(src, obj, compile_flags + extra_c, driver):
                        ok = False
                        continue
                    dest = os.path.join(out, f"{driver}-{mode}")
                    os.makedirs(dest, exist_ok=True)
                    exe = os.path.join(dest, base)
                    proc = link(driver, cc, out_dir, link_flags, [obj], exe,
                                [*extra_l, *COMMON_LIBS, *DRIVER_FLAGS[driver]])
                    if proc.returncode != 0:
                        print(f"  {name} ({driver}, {mode}): does not link:\n"
                              f"{(proc.stdout + proc.stderr).strip()}")
                        ok = False
                        continue
                    elf = Elf(readelf, exe)
                    found = mode_problems(elf, mode, libc)
                    if base == "gprof" and "monstartup" not in run(
                            [tool("NM", "aarch64-none-elf-nm"), exe]).stdout:
                        found.append("not started by gcrt1.o")
                    if base == "dlopen" and plugin:
                        shutil.copy(plugin[0], os.path.join(dest, "libplugin.so"))
                        if mode == "static-pie":
                            lacking = plugin[1].undefined - elf.exported
                            if lacking:
                                found.append(f"does not export what the plugin needs: {sorted(lacking)}")
                        else:
                            lacking = plugin[1].undefined - elf.exported - libc.exported
                            if lacking:
                                found.append(f"the plugin needs what neither it nor libc.so "
                                             f"exports: {sorted(lacking)}")
                    if found:
                        print(f"  {name} ({driver}, {mode}): {'; '.join(found)}")
                        ok = False
                    else:
                        counts[(driver, mode)] = counts.get((driver, mode), 0) + 1

        cxx_sources = sorted(f for f in os.listdir(PROGRAMS) if f.endswith(".cc"))
        if cxx_sources and not cxx:
            print("  no aarch64-none-elf-g++, so the C++ programs are not checked")
            cxx_sources = []
        nm = tool("NM", "aarch64-none-elf-nm")
        cxxflags = cxx_flags(cxx, inc) if cxx_sources else []
        for mode, mode_flags in MODES.items():
            compile_flags = [f for f in mode_flags if f.startswith("-f")]
            link_flags = [f for f in mode_flags if not f.startswith("-f")]
            for name in cxx_sources:
                src = os.path.join(PROGRAMS, name)
                base = name.rsplit(".", 1)[0]
                for driver in ("specs", "qnx-gcc"):
                    label = "g++ -specs" if driver == "specs" else "qnx-g++"
                    compiler = cxx if driver == "specs" else os.path.join(TREE, "qnx", "qnx-g++")
                    obj = os.path.join(tmp, f"{base}-{mode}-{driver}.o")
                    proc = run([compiler, *cxxflags, "-std=gnu++17", "-O1", "-Wall", "-Werror",
                                *compile_flags, *COMMON_CFLAGS, *DRIVER_FLAGS[driver], "-c", src,
                                "-o", obj], env=dict(os.environ, QNX_REAL_CXX=cxx))
                    if proc.returncode != 0:
                        print(f"  {name}: does not compile:\n{proc.stderr.strip()}")
                        ok = False
                        continue
                    dest = os.path.join(out, f"{driver}-{mode}")
                    os.makedirs(dest, exist_ok=True)
                    exe = os.path.join(dest, base)
                    proc = link(driver, cxx, out_dir, link_flags, [obj], exe,
                                [*COMMON_LIBS, *DRIVER_FLAGS[driver]], cxx=True)
                    if proc.returncode != 0:
                        print(f"  {name} ({label}, {mode}): does not link:\n"
                              f"{(proc.stdout + proc.stderr).strip()}")
                        ok = False
                        continue
                    found = mode_problems(Elf(readelf, exe), mode, libc)
                    symbols = run([nm, exe]).stdout
                    found += [f"has libsupc++'s {what}" for sym, what in SUPCXX_REPLACED.items()
                              if re.search(rf"\s{re.escape(sym)}$", symbols, re.M)]
                    found += frame_problems(readelf, nm, exe)
                    if found:
                        print(f"  {name} ({label}, {mode}): {'; '.join(found)}")
                        ok = False
                    else:
                        counts[(driver, mode)] = counts.get((driver, mode), 0) + 1
        for (driver, mode), n in sorted(counts.items()):
            print(f"  {driver}, {mode}: {n} programs link and are what the mode needs")
    return ok


def main():
    global INTERP
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("sysroot", help="the sysroot build-qnx.sh laid out")
    ap.add_argument("--out", help="where the linked test programs go")
    ap.add_argument("--interp", default=INTERP, help="the interpreter dynamically linked "
                    "programs name, where libc.so is on the target")
    args = ap.parse_args()
    INTERP = args.interp
    out = args.out or os.path.join(os.path.dirname(os.path.abspath(args.sysroot)), "link-check")
    cc = tool("CC", "aarch64-none-elf-gcc")
    cxx = find_tool("CXX", "aarch64-none-elf-g++")
    nm = tool("NM", "aarch64-none-elf-nm")
    readelf = tool("READELF", "aarch64-none-elf-readelf")
    print(f"== {args.sysroot}")
    headers_ok = check_headers(cc, nm, readelf, args.sysroot)
    programs_ok = check_programs(cc, cxx, readelf, args.sysroot, out)
    print(f"test programs for the target: {out}/<driver>-<mode>/")
    print("link check:", "passed" if headers_ok and programs_ok else "FAILED")
    return 0 if headers_ok and programs_ok else 1


if __name__ == "__main__":
    sys.exit(main())
