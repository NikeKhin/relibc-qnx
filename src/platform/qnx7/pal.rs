//! The [`Pal`] implementation, shared by the 7.1 and 8.0 backends.
//!
//! Every method delegates to one of the sibling modules, and the 8.0 backend
//! swaps in its own versions of the version-specific ones (`version`, `tls`,
//! `dupfd`), so this file itself carries no kernel-release knowledge.

use super::{
    PAGE_SIZE, abi, connect, io, ioctl, kercall, kernel, mem, proc, sync, thread, time, version,
};
use core::{ptr, slice};

use crate::platform::{Pal, types::*};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        bits_iovec::{gather, iovec, scatter},
        bits_sigset_t::sigset_t,
        errno::{EFAULT, EINVAL, ENOENT, ENOSYS, ERANGE},
        fcntl::{self, AT_EMPTY_PATH, AT_FDCWD, F_GETFL, F_GETLK, F_SETFL, F_SETLK, F_SETLKW},
        poll::pollfd,
        signal::sigevent,
        sys_resource::{rlimit, rusage},
        sys_select::timeval,
        sys_stat::stat,
        sys_statvfs::statvfs,
        sys_time::timezone,
        sys_utsname::utsname,
        time::{CLOCK_REALTIME, itimerspec, timespec},
    },
    iter::NulTerminated,
    ld_so::tcb::OsSpecific,
    out::Out,
    pthread,
};

/// QNX Neutrino implementation of [`Pal`].
pub struct Sys;

impl Sys {
    /// `ioctl()`; see the `ioctl` module.
    ///
    /// # Safety
    ///
    /// `buf` must be what `cmd` expects.
    pub unsafe fn ioctl(fd: c_int, cmd: c_ulong, buf: *mut c_void) -> Result<c_int> {
        unsafe { ioctl::ioctl(fd, cmd, buf) }
    }
}

/// `iovcnt` entries at `iov` as a slice; empty, whatever `iov` is, for none.
///
/// # Safety
///
/// `iovcnt` must be non-negative, and `iov` point at that many entries.
unsafe fn iovecs<'a>(iov: *const iovec, iovcnt: c_int) -> &'a [iovec] {
    match usize::try_from(iovcnt) {
        Ok(0) | Err(_) => &[],
        Ok(count) => unsafe { slice::from_raw_parts(iov, count) },
    }
}

impl Pal for Sys {
    fn faccessat(fd: c_int, path: CStr, amode: c_int, _flags: c_int) -> Result<()> {
        connect::faccessat(fd, path, amode)
    }

    unsafe fn brk(_addr: *mut c_void) -> Result<*mut c_void> {
        // Neutrino has no break; everything goes through mmap.
        Err(Errno(ENOSYS))
    }

    fn chdir(path: CStr) -> Result<()> {
        connect::chdir(path)
    }

    fn clock_getres(clk_id: clockid_t, tp: Option<Out<timespec>>) -> Result<()> {
        time::clock_getres(clk_id, tp)
    }

    fn clock_gettime(clk_id: clockid_t, tp: Out<timespec>) -> Result<()> {
        time::clock_gettime(clk_id, tp)
    }

    unsafe fn clock_settime(clk_id: clockid_t, tp: *const timespec) -> Result<()> {
        unsafe { time::clock_settime(clk_id, tp) }
    }

    fn close(fildes: c_int) -> Result<()> {
        io::close(fildes)
    }

    fn dup(fildes: c_int) -> Result<c_int> {
        Self::fcntl(fildes, fcntl::F_DUPFD, 0)
    }

    fn dup2(fildes: c_int, fildes2: c_int) -> Result<c_int> {
        if fildes == fildes2 {
            return Ok(fildes2);
        }
        // POSIX requires the target to be closed first, and for the new
        // descriptor to land on exactly that number.
        let _ = io::close(fildes2);
        io::dup_to(fildes, fildes2)
    }

    unsafe fn execve(path: CStr, argv: *const *mut c_char, envp: *const *mut c_char) -> Result<()> {
        // SAFETY: the caller guarantees NUL-terminated argument arrays.
        let argv = unsafe { NulTerminated::new(argv) }.ok_or(Errno(EINVAL))?;
        let envp = unsafe { NulTerminated::new(envp) };
        unsafe { proc::execve(path, argv, envp) }
    }

    unsafe fn fexecve(
        fildes: c_int,
        argv: *const *mut c_char,
        envp: *const *mut c_char,
    ) -> Result<()> {
        let argv = unsafe { NulTerminated::new(argv) }.ok_or(Errno(EINVAL))?;
        let envp = unsafe { NulTerminated::new(envp) };
        unsafe { proc::fexecve(fildes, argv, envp) }
    }

    fn exit(status: c_int) -> ! {
        proc::exit(status)
    }

    unsafe fn exit_thread(stack_base: *mut (), stack_size: usize) -> ! {
        unsafe { thread::exit_thread(stack_base, stack_size) }
    }

    fn fchdir(fildes: c_int) -> Result<()> {
        proc::fchdir(fildes)
    }

    fn fchmod(fildes: c_int, mode: mode_t) -> Result<()> {
        io::fchmod(fildes, mode)
    }

    fn fchmodat(fildes: c_int, path: Option<CStr>, mode: mode_t, _flags: c_int) -> Result<()> {
        match path {
            // An empty path means "the descriptor itself", which is a plain
            // _IO_CHMOD on the existing connection.
            None => io::fchmod(fildes, mode),
            Some(path) if path.to_bytes().is_empty() => io::fchmod(fildes, mode),
            Some(path) => {
                let fd = connect::openat(fildes, path, fcntl::O_RDONLY, 0)?;
                let result = io::fchmod(fd, mode);
                let _ = io::close(fd);
                result
            }
        }
    }

    fn fchown(fildes: c_int, owner: uid_t, group: gid_t) -> Result<()> {
        io::fchown(fildes, owner, group)
    }

    fn fchownat(
        fildes: c_int,
        path: CStr,
        owner: uid_t,
        group: gid_t,
        _flags: c_int,
    ) -> Result<()> {
        if path.to_bytes().is_empty() {
            return io::fchown(fildes, owner, group);
        }
        let fd = connect::openat(fildes, path, fcntl::O_RDONLY, 0)?;
        let result = io::fchown(fd, owner, group);
        let _ = io::close(fd);
        result
    }

    fn fdatasync(fildes: c_int) -> Result<()> {
        io::fsync(fildes, 0)
    }

    fn flock(fd: c_int, operation: c_int) -> Result<()> {
        io::flock(fd, operation)
    }

    fn fstat(fildes: c_int, buf: Out<stat>) -> Result<()> {
        io::fstat(fildes, buf)
    }

    fn fstatat(fildes: c_int, path: Option<CStr>, buf: Out<stat>, flags: c_int) -> Result<()> {
        match path {
            None => io::fstat(fildes, buf),
            Some(path) if path.to_bytes().is_empty() => io::fstat(fildes, buf),
            Some(path) => {
                let oflag = if flags & fcntl::AT_SYMLINK_NOFOLLOW != 0 {
                    fcntl::O_RDONLY | fcntl::O_NOFOLLOW
                } else {
                    fcntl::O_RDONLY
                };
                let fd = connect::openat(fildes, path, oflag, 0)?;
                let result = io::fstat(fd, buf);
                let _ = io::close(fd);
                result
            }
        }
    }

    fn fstatvfs(fildes: c_int, buf: Out<statvfs>) -> Result<()> {
        io::fstatvfs(fildes, buf)
    }

    fn fcntl(fildes: c_int, cmd: c_int, arg: c_ulonglong) -> Result<c_int> {
        match cmd {
            fcntl::F_DUPFD => io::dup_to(fildes, arg as c_int),
            fcntl::F_DUPFD_CLOEXEC => {
                let new_fd = io::dup_to(fildes, arg as c_int)?;
                kernel::connect_flags(
                    0,
                    new_fd,
                    abi::neutrino::cof::CLOEXEC,
                    abi::neutrino::cof::CLOEXEC,
                )?;
                Ok(new_fd)
            }
            fcntl::F_GETFD => kernel::connect_flags(0, fildes, 0, 0).map(|flags| flags as c_int),
            fcntl::F_SETFD => kernel::connect_flags(
                0,
                fildes,
                abi::neutrino::cof::CLOEXEC,
                if arg as c_int & fcntl::FD_CLOEXEC != 0 {
                    abi::neutrino::cof::CLOEXEC
                } else {
                    0
                },
            )
            .map(|_| 0),
            F_GETFL => io::getfl(fildes),
            F_SETFL => io::setfl(fildes, arg as c_int).map(|_| 0),
            F_GETLK | F_SETLK | F_SETLKW => io::fcntl_lock(fildes, cmd, arg),
            _ => Err(Errno(ENOSYS)),
        }
    }

    unsafe fn fork() -> Result<pid_t> {
        unsafe { proc::fork() }
    }

    fn fpath(fildes: c_int, out: &mut [u8]) -> Result<usize> {
        io::fpath(fildes, out)
    }

    fn fsync(fildes: c_int) -> Result<()> {
        io::fsync(fildes, 1)
    }

    fn ftruncate(fildes: c_int, length: off_t) -> Result<()> {
        io::ftruncate(fildes, length)
    }

    unsafe fn futex_wait(addr: *mut u32, val: u32, deadline: Option<&timespec>) -> Result<()> {
        unsafe { sync::futex_wait(addr, val, deadline) }
    }

    unsafe fn futex_wake(addr: *mut u32, num: u32) -> Result<u32> {
        unsafe { sync::futex_wake(addr, num) }
    }

    fn utimensat(
        dirfd: c_int,
        path: CStr,
        times: Option<&[timespec; 2]>,
        flag: c_int,
    ) -> Result<()> {
        let times = times.map_or(ptr::null(), |times| times.as_ptr());
        if path.to_bytes().is_empty() || flag & AT_EMPTY_PATH != 0 {
            return io::futimens(dirfd, times);
        }
        let oflag = if flag & fcntl::AT_SYMLINK_NOFOLLOW != 0 {
            fcntl::O_RDONLY | fcntl::O_NOFOLLOW
        } else {
            fcntl::O_RDONLY
        };
        let fd = connect::openat(dirfd, path, oflag, 0)?;
        let result = io::futimens(fd, times);
        let _ = io::close(fd);
        result
    }

    fn getcwd(mut buf: Out<[u8]>) -> Result<()> {
        if buf.is_empty() {
            return Err(Errno(ERANGE));
        }
        let fd = connect::openat(AT_FDCWD, c".".into(), fcntl::O_RDONLY, 0)?;
        // Initialised first: a `&mut [u8]` must not cover uninitialised bytes.
        buf.zero();
        let slice = unsafe {
            core::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len())
        };
        let n = io::fpath(fd, slice);
        let _ = io::close(fd);
        let n = n?;
        if n + 1 > slice.len() {
            return Err(Errno(ERANGE));
        }
        slice[n] = 0;
        Ok(())
    }

    fn getdents(fd: c_int, buf: &mut [u8], _opaque_offset: u64) -> Result<usize> {
        // A directory descriptor answers _IO_READ with a run of QNX dirents.
        io::read(fd, buf)
    }

    fn dir_seek(fd: c_int, opaque_offset: u64) -> Result<()> {
        io::lseek(fd, opaque_offset as off_t, crate::header::unistd::SEEK_SET).map(|_| ())
    }

    unsafe fn dent_reclen_offset(this_dent: &[u8], _offset: usize) -> Option<(u16, u64)> {
        let dent = this_dent
            .get(..size_of::<abi::iomsg::QnxDirent>())?
            .as_ptr()
            .cast::<abi::iomsg::QnxDirent>();
        // SAFETY: the slice was just length-checked, and every field is plain
        // data.
        let dent = unsafe { dent.read_unaligned() };
        let reclen = u16::try_from(dent.d_reclen).ok()?;
        Some((reclen, dent.d_offset as u64))
    }

    fn getegid() -> gid_t {
        proc::getegid()
    }

    fn geteuid() -> uid_t {
        proc::geteuid()
    }

    fn getgid() -> gid_t {
        proc::getgid()
    }

    fn getgroups(list: Out<[gid_t]>) -> Result<c_int> {
        proc::getgroups(list)
    }

    fn getpagesize() -> usize {
        PAGE_SIZE
    }

    fn getpgid(pid: pid_t) -> Result<pid_t> {
        proc::getpgid(pid)
    }

    fn getpid() -> pid_t {
        proc::getpid()
    }

    fn getppid() -> pid_t {
        proc::getppid()
    }

    fn getpriority(which: c_int, who: id_t) -> Result<c_int> {
        proc::getpriority(which, who)
    }

    fn getrandom(buf: &mut [u8], _flags: c_uint) -> Result<usize> {
        let fd = connect::openat(AT_FDCWD, c"/dev/random".into(), fcntl::O_RDONLY, 0)?;
        let result = io::read(fd, buf);
        let _ = io::close(fd);
        result
    }

    fn getresgid(
        rgid: Option<Out<gid_t>>,
        egid: Option<Out<gid_t>>,
        sgid: Option<Out<gid_t>>,
    ) -> Result<()> {
        proc::getresgid(rgid, egid, sgid)
    }

    fn getresuid(
        ruid: Option<Out<uid_t>>,
        euid: Option<Out<uid_t>>,
        suid: Option<Out<uid_t>>,
    ) -> Result<()> {
        proc::getresuid(ruid, euid, suid)
    }

    fn getrlimit(resource: c_int, rlim: Out<rlimit>) -> Result<()> {
        proc::getrlimit(resource, rlim)
    }

    unsafe fn setrlimit(resource: c_int, rlim: *const rlimit) -> Result<()> {
        // SAFETY: non-null, and otherwise the caller's `struct rlimit`.
        let rlim = unsafe { rlim.as_ref() }.ok_or(Errno(EFAULT))?;
        proc::setrlimit(resource, rlim)
    }

    fn getrusage(who: c_int, r_usage: Out<rusage>) -> Result<()> {
        proc::getrusage(who, r_usage)
    }

    fn getsid(pid: pid_t) -> Result<pid_t> {
        proc::getsid(pid)
    }

    fn gettid() -> pid_t {
        proc::gettid()
    }

    fn gettimeofday(mut tp: Out<timeval>, tzp: Option<Out<timezone>>) -> Result<()> {
        let mut ts = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        time::clock_gettime(CLOCK_REALTIME, Out::from_mut(&mut ts))?;
        tp.write(timeval {
            tv_sec: ts.tv_sec,
            tv_usec: (ts.tv_nsec / 1000) as suseconds_t,
        });
        if let Some(mut tzp) = tzp {
            tzp.write(timezone {
                tz_minuteswest: 0,
                tz_dsttime: 0,
            });
        }
        Ok(())
    }

    fn getuid() -> uid_t {
        proc::getuid()
    }

    fn linkat(
        _fd1: c_int,
        oldpath: CStr,
        fd2: c_int,
        newpath: CStr,
        _flags: c_int,
    ) -> Result<()> {
        connect::linkat(oldpath, fd2, newpath)
    }

    fn lseek(fildes: c_int, offset: off_t, whence: c_int) -> Result<off_t> {
        io::lseek(fildes, offset, whence)
    }

    fn mkdirat(fildes: c_int, path: CStr, mode: mode_t) -> Result<()> {
        connect::mkdirat(fildes, path, mode)
    }

    fn mkfifoat(dir_fd: c_int, path: CStr, mode: mode_t) -> Result<()> {
        connect::mkfifoat(dir_fd, path, mode)
    }

    fn mknodat(fildes: c_int, path: CStr, mode: mode_t, _dev: dev_t) -> Result<()> {
        connect::mknodat(fildes, path, mode)
    }

    unsafe fn mlock(addr: *const c_void, len: usize) -> Result<()> {
        unsafe { mem::mlock(addr, len) }
    }

    unsafe fn mlockall(flags: c_int) -> Result<()> {
        mem::mlockall(flags)
    }

    unsafe fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fildes: c_int,
        off: off_t,
    ) -> Result<*mut c_void> {
        unsafe { mem::mmap(addr, len, prot, flags, fildes, off) }
    }

    unsafe fn mremap(
        addr: *mut c_void,
        len: usize,
        new_len: usize,
        flags: c_int,
        args: *mut c_void,
    ) -> Result<*mut c_void> {
        unsafe { mem::mremap(addr, len, new_len, flags, args) }
    }

    unsafe fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> Result<()> {
        unsafe { mem::mprotect(addr, len, prot) }
    }

    unsafe fn msync(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> {
        unsafe { mem::msync(addr, len, flags) }
    }

    unsafe fn munlock(addr: *const c_void, len: usize) -> Result<()> {
        unsafe { mem::munlock(addr, len) }
    }

    unsafe fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> Result<()> {
        unsafe { mem::madvise(addr, len, advice) }
    }

    unsafe fn munlockall() -> Result<()> {
        mem::munlockall()
    }

    unsafe fn munmap(addr: *mut c_void, len: usize) -> Result<()> {
        unsafe { mem::munmap(addr, len) }
    }

    fn nanosleep(rqtp: &timespec, rmtp: Option<Out<timespec>>) -> Result<()> {
        let rmtp = rmtp.map_or(ptr::null_mut(), |mut rmtp| rmtp.as_mut_ptr());
        // SAFETY: `rqtp` is a reference, and `rmtp` null or writable.
        unsafe { time::nanosleep(rqtp, rmtp) }
    }

    fn openat(dirfd: c_int, path: CStr, oflag: c_int, mode: mode_t) -> Result<c_int> {
        connect::openat(dirfd, path, oflag, mode)
    }

    fn pipe2(mut fildes: Out<[c_int; 2]>, flags: c_int) -> Result<()> {
        let mut request = connect::Request::open(
            fcntl::O_RDONLY | (flags & (fcntl::O_CLOEXEC | fcntl::O_NONBLOCK)),
            0,
            AT_FDCWD,
        );
        request.file_type = abi::iomsg::ftype::PIPE;
        request.qnx_oflag |= abi::oflag::o::ANON | abi::oflag::o::NOCTTY;
        let fd0 = match connect::resolve(version::PIPE_PATH.into(), &request)? {
            connect::Resolved::Connection(fd) => fd,
            connect::Resolved::Done => return Err(Errno(ENOENT)),
        };
        let wrflag = fcntl::O_WRONLY | (flags & (fcntl::O_CLOEXEC | fcntl::O_NONBLOCK));
        match io::openfd(fd0, wrflag, abi::iomsg::openfd::PIPE) {
            Ok(fd1) => {
                fildes.write([fd0, fd1]);
                Ok(())
            }
            Err(err) => {
                let _ = io::close(fd0);
                Err(err)
            }
        }
    }

    /// Not supported yet. relibc used to build `poll` on `epoll`, which
    /// Neutrino has no equivalent of (see `epoll.rs`), so it never worked
    /// here; the native route is `ionotify` arming each descriptor.
    fn poll(_fds: &mut [pollfd], _timeout: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    /// Not supported yet, as [`poll`](Self::poll).
    fn ppoll(
        _fds: &mut [pollfd],
        _timeout: Option<&timespec>,
        _sigmask: Option<&sigset_t>,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn posix_fallocate(fd: c_int, offset: u64, length: core::num::NonZeroU64) -> Result<()> {
        io::space(fd, abi::iomsg::space::ALLOC, offset, length.get()).map(|_| ())
    }

    fn posix_getdents(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        io::read(fildes, buf)
    }

    unsafe fn rlct_clone(
        stack: *mut usize,
        _os_specific: &mut OsSpecific,
    ) -> Result<pthread::OsTid, Errno> {
        unsafe { thread::clone(stack) }
    }

    unsafe fn rlct_kill(os_tid: pthread::OsTid, signal: usize) -> Result<()> {
        let signal = abi::signal::to_kernel(signal as c_int).ok_or(Errno(EINVAL))?;
        kernel::signal_kill(0, proc::getpid(), os_tid.thread_id as c_int, signal, 0, 0)
    }

    fn current_os_tid() -> pthread::OsTid {
        thread::current()
    }

    fn read(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        io::read(fildes, buf)
    }

    /// One read into a buffer as large as all of `iov`, then copied out, as
    /// relibc did on every platform before `readv` moved into [`Pal`].
    unsafe fn readv(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        // SAFETY: `readv` checked `iovcnt`, and `iov` holds that many entries.
        let iovs = unsafe { iovecs(iov, iovcnt) };
        let mut buf = unsafe { gather(iovs) };
        let n = io::read(fildes, &mut buf)?;
        unsafe { scatter(iovs, &buf) };
        Ok(n)
    }

    fn pread(fildes: c_int, buf: &mut [u8], offset: off_t) -> Result<usize> {
        io::pread(fildes, buf, offset)
    }

    fn readlinkat(dirfd: c_int, pathname: CStr, out: &mut [u8]) -> Result<usize> {
        connect::readlinkat(dirfd, pathname, out)
    }

    fn renameat2(
        _old_dir: c_int,
        old_path: CStr,
        new_dir: c_int,
        new_path: CStr,
        flags: c_uint,
    ) -> Result<()> {
        // QNX has no equivalent of RENAME_NOREPLACE and friends.
        if flags != 0 {
            return Err(Errno(EINVAL));
        }
        connect::renameat(old_path, new_dir, new_path)
    }

    fn sched_yield() -> Result<()> {
        kernel::sched_yield()
    }

    unsafe fn setgroups(size: size_t, list: *const gid_t) -> Result<()> {
        proc::setgroups(size, list)
    }

    fn setpgid(pid: pid_t, pgid: pid_t) -> Result<()> {
        proc::setpgid(pid, pgid)
    }

    fn setpriority(which: c_int, who: id_t, prio: c_int) -> Result<()> {
        proc::setpriority(which, who, prio)
    }

    fn setresgid(rgid: gid_t, egid: gid_t, sgid: gid_t) -> Result<()> {
        proc::setresgid(rgid, egid, sgid)
    }

    fn setresuid(ruid: uid_t, euid: uid_t, suid: uid_t) -> Result<()> {
        proc::setresuid(ruid, euid, suid)
    }

    fn setsid() -> Result<c_int> {
        proc::setsid()
    }

    unsafe fn spawn(
        program: CStr,
        fac: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
        fat: Option<&crate::header::spawn::posix_spawnattr_t>,
        argv: NulTerminated<*mut c_char>,
        envp: Option<NulTerminated<*mut c_char>>,
    ) -> Result<pid_t> {
        unsafe { proc::spawn(program, fac, fat, argv, envp) }
    }

    fn symlinkat(path1: CStr, fd: c_int, path2: CStr) -> Result<()> {
        connect::symlinkat(path1, fd, path2)
    }

    fn sync() -> Result<()> {
        proc::sync()
    }

    fn timer_create(clock_id: clockid_t, evp: &sigevent) -> Result<timer_t> {
        time::timer_create(clock_id, evp)
    }

    fn timer_delete(timerid: timer_t) -> Result<()> {
        time::timer_delete(timerid)
    }

    fn timer_gettime(timerid: timer_t) -> Result<itimerspec> {
        time::timer_gettime(timerid)
    }

    fn timer_settime(
        timerid: timer_t,
        flags: c_int,
        value: &itimerspec,
        ovalue: Option<Out<itimerspec>>,
    ) -> Result<()> {
        time::timer_settime(timerid, flags, value, ovalue)
    }

    fn umask(mask: mode_t) -> mode_t {
        proc::umask(mask, true)
    }

    fn uname(utsname: Out<utsname>) -> Result<()> {
        proc::uname(utsname)
    }

    fn unlinkat(fd: c_int, path: CStr, flags: c_int) -> Result<()> {
        if flags & fcntl::AT_REMOVEDIR != 0 {
            connect::rmdir(fd, path)
        } else {
            connect::unlinkat(fd, path)
        }
    }

    fn waitpid(pid: pid_t, stat_loc: Option<Out<c_int>>, options: c_int) -> Result<pid_t> {
        let (child, status) = proc::waitpid(pid, options)?;
        if let Some(mut out) = stat_loc {
            out.write(status);
        }
        Ok(child)
    }

    fn write(fildes: c_int, buf: &[u8]) -> Result<usize> {
        io::write(fildes, buf)
    }

    /// One write of all of `iov`, gathered into one buffer, as relibc did on
    /// every platform before `writev` moved into [`Pal`].
    unsafe fn writev(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        // SAFETY: `writev` checked `iovcnt`, and `iov` holds that many entries.
        let buf = unsafe { gather(iovecs(iov, iovcnt)) };
        io::write(fildes, &buf)
    }

    fn pwrite(fildes: c_int, buf: &[u8], offset: off_t) -> Result<usize> {
        io::pwrite(fildes, buf, offset)
    }

    fn verify() -> bool {
        // A successful kernel call proves both the `svc` immediate and the
        // error convention are right for this kernel.
        unsafe { kercall::kercall0(version::ker::NOP) }.is_ok()
    }
}
