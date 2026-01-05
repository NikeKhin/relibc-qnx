//! Process-manager operations.
//!
//! Identity, sessions, `umask`, forking, spawning and reaping are all messages
//! to procnto on [`SYSMGR_COID`]. The vendor `getppid`, for instance, is
//! nothing but a `_PROC_GETSETID` send whose reply's `eid` field holds the
//! parent's pid.
//!
//! [`SYSMGR_COID`]: super::abi::neutrino::SYSMGR_COID

use alloc::vec::Vec;

use super::{
    abi::{
        iomsg::{
            MsgInheritance, ProcEvent, ProcFork, ProcGetSetId, ProcGetSetIdReply, ProcResourceGetlimit,
            ProcResourceSetlimit, ProcResourceUsage, ProcSession, ProcSpawn, ProcWait, SysConf,
            SysConfReply, event, event_flag, fork_flags, getsetid, idtype, proc, resource,
            session, spawn_flags, spawn_inherit, sys_conf, umask, wait, PROC_NGROUPS_MAX,
        },
        neutrino::{ClientInfo, NGROUPS_MAX, SYSMGR_COID},
        signal::{Siginfo, qnx as qnx_signal, set_to_kernel, wait_status},
    },
    connect,
    kernel::{
        connect_client_info, msg_send, msg_send_nc, msg_send_struct, msg_sendvnc, sched_get, sched_set,
        thread_destroy,
    },
    tls,
};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        errno::{EINTR, EINVAL, ENOMEM, ENOSYS, ERANGE},
        fcntl::AT_FDCWD,
        sys_resource::{
            RLIMIT_AS, RLIMIT_CORE, RLIMIT_CPU, RLIMIT_DATA, RLIMIT_FSIZE, RLIMIT_MEMLOCK,
            RLIMIT_NOFILE, RLIMIT_NPROC, RLIMIT_RSS, RLIMIT_STACK, RLIM_INFINITY, rlimit, rusage,
            PRIO_PROCESS, RUSAGE_CHILDREN, RUSAGE_SELF,
        },
        sys_utsname::{UTSLENGTH, utsname},
        sys_wait::{WCONTINUED, WNOHANG, WUNTRACED},
    },
    iter::NulTerminated,
    out::Out,
    platform::types::*,
};

/// The fixed reply size procnto expects for `_PROC_GETSETID` transactions.
const GETSETID_REPLY_MAX: usize = 88;

/// How many times to retry a message that procnto interrupted.
const EINTR_RETRIES: u32 = 256;

/// QNX `RLIM_INFINITY` is `-3`, not Linux's `u64::MAX`.
const QNX_RLIM_INFINITY: u64 = !2;

fn rlim_to_qnx(v: u64) -> u64 {
    if v == RLIM_INFINITY { QNX_RLIM_INFINITY } else { v }
}

fn rlim_from_qnx(v: u64) -> u64 {
    if v == QNX_RLIM_INFINITY { RLIM_INFINITY } else { v }
}

fn rlimit_to_qnx(resource: c_int) -> Result<u32> {
    Ok(match resource {
        RLIMIT_CPU => 0,
        RLIMIT_FSIZE => 1,
        RLIMIT_DATA => 2,
        RLIMIT_STACK => 3,
        RLIMIT_CORE => 4,
        RLIMIT_NOFILE => 5,
        RLIMIT_AS | RLIMIT_RSS => 6,
        RLIMIT_MEMLOCK => 7,
        RLIMIT_NPROC => 8,
        _ => return Err(Errno(EINVAL)),
    })
}

#[repr(C)]
struct ClientInfoBuf {
    info: ClientInfo,
    groups: [gid_t; NGROUPS_MAX],
}

fn client_info(ngroups: c_int) -> Result<ClientInfoBuf> {
    let mut buf = ClientInfoBuf {
        info: ClientInfo::default(),
        groups: [0; NGROUPS_MAX],
    };
    unsafe {
        connect_client_info(
            -1,
            ptr::from_mut(&mut buf).cast::<u8>(),
            ngroups,
        )
    }?;
    Ok(buf)
}

use core::ptr;

/// Runs a `_PROC_GETSETID` transaction.
fn getsetid_msg(subtype: u16, pid: pid_t, eid: i32, rid: i32, ngroups: i32) -> Result<ProcGetSetIdReply> {
    let request = ProcGetSetId {
        type_: proc::GETSETID,
        subtype,
        pid,
        eid,
        rid,
        ngroups,
    };
    let mut reply = [0u8; GETSETID_REPLY_MAX];
    unsafe {
        msg_send_nc(
            SYSMGR_COID,
            ptr::from_ref(&request).cast::<u8>(),
            size_of::<ProcGetSetId>(),
            reply.as_mut_ptr(),
            GETSETID_REPLY_MAX,
        )
    }?;
    Ok(unsafe { ptr::read_unaligned(reply.as_ptr().cast::<ProcGetSetIdReply>()) })
}

/// `getpid()`. The kernel keeps it in the thread's TLS block, so this is a
/// register read rather than a message.
pub fn getpid() -> pid_t {
    tls::pid()
}

/// `gettid()`.
pub fn gettid() -> pid_t {
    tls::tid()
}

/// `getppid()`.
pub fn getppid() -> pid_t {
    getsetid_msg(getsetid::GETID_NO_CRED, 0, 0, 0, 0)
        .map(|r| r.ppid)
        .unwrap_or(0)
}

/// `getuid()`.
pub fn getuid() -> uid_t {
    client_info(0).map(|c| c.info.ruid).unwrap_or(0)
}

/// `geteuid()`.
pub fn geteuid() -> uid_t {
    client_info(0).map(|c| c.info.euid).unwrap_or(0)
}

/// `getgid()`.
pub fn getgid() -> gid_t {
    client_info(0).map(|c| c.info.rgid).unwrap_or(0)
}

/// `getegid()`.
pub fn getegid() -> gid_t {
    client_info(0).map(|c| c.info.egid).unwrap_or(0)
}

/// `getresuid()`.
pub fn getresuid(
    ruid: Option<Out<uid_t>>,
    euid: Option<Out<uid_t>>,
    suid: Option<Out<uid_t>>,
) -> Result<()> {
    let cred = client_info(0)?.info;
    if let Some(mut ruid) = ruid {
        ruid.write(cred.ruid);
    }
    if let Some(mut euid) = euid {
        euid.write(cred.euid);
    }
    if let Some(mut suid) = suid {
        suid.write(cred.suid);
    }
    Ok(())
}

/// `getresgid()`.
pub fn getresgid(
    rgid: Option<Out<gid_t>>,
    egid: Option<Out<gid_t>>,
    sgid: Option<Out<gid_t>>,
) -> Result<()> {
    let cred = client_info(0)?.info;
    if let Some(mut rgid) = rgid {
        rgid.write(cred.rgid);
    }
    if let Some(mut egid) = egid {
        egid.write(cred.egid);
    }
    if let Some(mut sgid) = sgid {
        sgid.write(cred.sgid);
    }
    Ok(())
}

/// `getgroups()`.
pub fn getgroups(list: Out<[gid_t]>) -> Result<c_int> {
    let n = list.len() as c_int;
    let asked = if n > 0 { n + 1 } else { 0 };
    let buf = client_info(asked)?;
    let count = buf.info.ngroups as c_int;
    if n == 0 {
        return Ok(count);
    }
    if count > n {
        return Err(Errno(ERANGE));
    }
    let mut list = list;
    let copy = count as usize;
    if copy > 0 {
        list.split_at_checked(copy)
            .ok_or(Errno(ERANGE))?[0]
            .copy_from_slice(&buf.groups[..copy]);
    }
    Ok(count)
}

/// `setgroups()`.
pub fn setgroups(size: size_t, list: *const gid_t) -> Result<()> {
    if size > PROC_NGROUPS_MAX {
        return Err(Errno(EINVAL));
    }
    let request = ProcGetSetId {
        type_: proc::GETSETID,
        subtype: getsetid::SETGROUPS,
        pid: 0,
        eid: 0,
        rid: 0,
        ngroups: size as i32,
    };
    let iov = [
        super::abi::neutrino::IoVec::new(
            ptr::from_ref(&request).cast::<u8>(),
            size_of::<ProcGetSetId>(),
        ),
        super::abi::neutrino::IoVec::new(list.cast::<u8>(), size * size_of::<gid_t>()),
    ];
    unsafe {
        msg_sendvnc(
            SYSMGR_COID,
            iov.as_ptr(),
            if size == 0 { 1 } else { 2 },
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// `setuid()`.
pub fn setuid(uid: uid_t) -> Result<()> {
    getsetid_msg(getsetid::SETUID, 0, uid as i32, -1, 0).map(|_| ())
}

/// `setgid()`.
pub fn setgid(gid: gid_t) -> Result<()> {
    getsetid_msg(getsetid::SETGID, 0, gid as i32, -1, 0).map(|_| ())
}

/// `setresuid()`.
pub fn setresuid(ruid: uid_t, euid: uid_t, _suid: uid_t) -> Result<()> {
    match (ruid != uid_t::MAX, euid != uid_t::MAX) {
        (true, true) => getsetid_msg(getsetid::SETREUID, 0, euid as i32, ruid as i32, 0).map(|_| ()),
        (true, false) => setuid(ruid),
        (false, true) => getsetid_msg(getsetid::SETEUID, 0, euid as i32, -1, 0).map(|_| ()),
        (false, false) => Ok(()),
    }
}

/// `setresgid()`.
pub fn setresgid(rgid: gid_t, egid: gid_t, _sgid: gid_t) -> Result<()> {
    match (rgid != gid_t::MAX, egid != gid_t::MAX) {
        (true, true) => getsetid_msg(getsetid::SETREGID, 0, egid as i32, rgid as i32, 0).map(|_| ()),
        (true, false) => setgid(rgid),
        (false, true) => getsetid_msg(getsetid::SETEGID, 0, egid as i32, -1, 0).map(|_| ()),
        (false, false) => Ok(()),
    }
}

/// Reads or replaces the process's file creation mask.
pub fn umask(mask: mode_t, set: bool) -> mode_t {
    let request = ProcUmaskMsg {
        type_: proc::UMASK,
        subtype: if set { umask::SET } else { umask::GET },
        umask: mask as u32,
        pid: 0,
    };
    let mut reply = ProcUmaskMsg::default();
    match unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) } {
        Ok(_) => reply.umask as mode_t,
        Err(_) => 0,
    }
}

use super::abi::iomsg::ProcUmask as ProcUmaskMsg;

/// Runs a `_PROC_SESSION` transaction.
fn session(xid: u16, sid: pid_t, id: i32) -> Result<i32> {
    let request = ProcSession {
        type_: proc::SESSION,
        xid,
        sid,
        id,
        event: 0,
    };
    let mut reply = ProcSession::default();
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }?;
    Ok(reply.id)
}

/// `setsid()`.
pub fn setsid() -> Result<pid_t> {
    session(session::SETSID, 0, 0)
}

/// `getsid()`.
pub fn getsid(pid: pid_t) -> Result<pid_t> {
    session(session::GETSID, pid, 0)
}

/// `getpgid()`.
pub fn getpgid(pid: pid_t) -> Result<pid_t> {
    session(session::GETPGRP, pid, 0)
}

/// `setpgid()`.
pub fn setpgid(pid: pid_t, pgid: pid_t) -> Result<()> {
    session(session::SETPGRP, pid, pgid).map(|_| ())
}

/// `fork()`.
///
/// procnto duplicates the caller when the message's `frame` is zero, capturing
/// the register state itself; the reply status is the child's pid in the parent
/// and zero in the child. The vendor `fork()` is `_fork(1, 0)`, where bit zero
/// asks for the `pthread_atfork` handlers to run — relibc runs those itself in
/// `header::unistd::fork`, so the flag is not set here.
///
/// # Safety
///
/// Forking a multi-threaded process leaves the child with one thread and any
/// lock the other threads held still held, which is the caller's problem.
pub unsafe fn fork() -> Result<pid_t> {
    let request = ProcFork {
        type_: proc::FORK,
        zero: 0,
        flags: 0,
        frame: 0,
    };
    let _ = fork_flags::ATFORK;

    for _ in 0..EINTR_RETRIES {
        let result = unsafe {
            msg_send_nc(
                SYSMGR_COID,
                core::ptr::from_ref(&request).cast::<u8>(),
                size_of::<ProcFork>(),
                core::ptr::null_mut(),
                0,
            )
        };
        match result {
            Ok(pid) => return Ok(pid as pid_t),
            Err(Errno(EINTR)) => continue,
            Err(err) => return Err(err),
        }
    }
    Err(Errno(EINTR))
}

/// Builds the descriptor map a spawn request carries.
///
/// procnto gives the child descriptor `i` whatever `fd_map[i]` names in the
/// parent, or closes it when the entry is -1. That covers the whole of POSIX's
/// `dup2` and `close` file actions directly; an `open` action is performed in
/// the parent first and then mapped into place, which is what the vendor
/// implementation does too.
///
/// Returns the map together with the descriptors opened on its behalf, which
/// the caller closes once the message has been sent.
fn build_fd_map(
    actions: &crate::header::spawn::posix_spawn_file_actions_t,
) -> Result<(Vec<i32>, Vec<c_int>)> {
    use crate::header::spawn::Action;

    let mut map: Vec<i32> = Vec::new();
    let mut opened: Vec<c_int> = Vec::new();

    let set = |map: &mut Vec<i32>, child: c_int, parent: i32| -> Result<()> {
        let child = usize::try_from(child).map_err(|_| Errno(EINVAL))?;
        if child >= 1024 {
            return Err(Errno(EINVAL));
        }
        // Entries the caller did not mention keep their identity mapping.
        while map.len() <= child {
            let index = map.len() as i32;
            map.push(index);
        }
        map[child] = parent;
        Ok(())
    };

    for action in actions {
        match action {
            Action::Dup2(from, to) => set(&mut map, to, from)?,
            Action::Close(fd) => set(&mut map, fd, -1)?,
            Action::Open {
                fd,
                ref path,
                flag,
                mode,
            } => {
                // SAFETY: the action owns the CString for the duration of
                // this call.
                let path = unsafe { CStr::from_ptr(path.as_ptr()) };
                let opened_fd = connect::openat(AT_FDCWD, path, flag, mode)?;
                opened.push(opened_fd);
                set(&mut map, fd, opened_fd)?;
            }
            // procnto's spawn message has no working-directory field, so
            // these would have to be applied to the parent around the spawn —
            // which is not safe to do in a threaded process, and silently
            // ignoring them would start the child in the wrong place.
            Action::Chdir(_) | Action::FChdir(_) => {}
        }
    }

    Ok((map, opened))
}

/// Translates relibc's spawn attributes into the inheritance block.
fn build_inheritance(
    attr: Option<&crate::header::spawn::posix_spawnattr_t>,
    fd_list: bool,
) -> MsgInheritance {
    use crate::header::spawn::Flags;

    // Without an explicit descriptor map every descriptor is inherited except
    // those marked close-on-exec, which is what plain posix_spawn means.
    let mut parms = MsgInheritance {
        flags: if fd_list {
            spawn_flags::FD_LIST
        } else {
            spawn_flags::FD_NOCLOEXEC
        },
        ..MsgInheritance::default()
    };

    let Some(attr) = attr else {
        return parms;
    };
    let flags = Flags::from_bits_truncate(attr.flags);

    if flags.contains(Flags::POSIX_SPAWN_SETPGROUP) {
        parms.flags |= spawn_inherit::SETGROUP;
        parms.pgroup = attr.pgroup;
    }
    if flags.contains(Flags::POSIX_SPAWN_SETSIGMASK) {
        parms.flags |= spawn_inherit::SETSIGMASK;
        parms.sigmask = set_to_kernel(attr.sigmask);
    }
    if flags.contains(Flags::POSIX_SPAWN_SETSIGDEF) {
        parms.flags |= spawn_inherit::SETSIGDEF;
        parms.sigdefault = set_to_kernel(attr.sigdefault);
    }
    if flags.contains(Flags::POSIX_SPAWN_SETSCHEDULER) || flags.contains(Flags::POSIX_SPAWN_SETSCHEDPARAM) {
        // relibc keeps the policy private, so only the priority crosses
        // over; the child inherits the caller's policy.
        parms.flags |= spawn_inherit::EXPLICIT_SCHED;
        parms.param.sched_priority = attr.param.sched_priority;
    }
    if flags.contains(Flags::POSIX_SPAWN_SETSID) {
        parms.flags |= spawn_inherit::SETSID;
    }

    parms
}

fn has_chdir_action(actions: &crate::header::spawn::posix_spawn_file_actions_t) -> bool {
    use crate::header::spawn::Action;
    actions.into_iter().any(|action| {
        matches!(action, Action::Chdir(_) | Action::FChdir(_))
    })
}

fn apply_file_actions(actions: &crate::header::spawn::posix_spawn_file_actions_t) -> Result<()> {
    use crate::header::spawn::Action;
    for action in actions {
        match action {
            Action::Dup2(from, to) => {
                if from != to {
                    let _ = super::io::close(to);
                    super::io::dup_to(from, to)?;
                }
            }
            Action::Close(fd) => {
                let _ = super::io::close(fd);
            }
            Action::Open {
                fd,
                ref path,
                flag,
                mode,
            } => {
                let path = unsafe { CStr::from_ptr(path.as_ptr()) };
                let opened = connect::openat(AT_FDCWD, path, flag, mode)?;
                if opened != fd {
                    let _ = super::io::close(fd);
                    super::io::dup_to(opened, fd)?;
                    let _ = super::io::close(opened);
                }
            }
            Action::Chdir(ref path) => {
                let path = unsafe { CStr::from_ptr(path.as_ptr()) };
                connect::chdir(path)?;
            }
            Action::FChdir(fd) => fchdir(fd)?,
        }
    }
    Ok(())
}

/// `posix_spawn()`.
///
/// The request is a `_proc_spawn` header followed by the descriptor map, the
/// search path, the program path, and finally the argument and environment
/// strings laid out back to back with their NUL terminators.
///
/// # Safety
///
/// `argv` and `envp` must be NUL-terminated arrays of valid C strings.
pub unsafe fn spawn(
    path: CStr,
    file_actions: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
    attr: Option<&crate::header::spawn::posix_spawnattr_t>,
    argv: NulTerminated<*mut c_char>,
    envp: Option<NulTerminated<*mut c_char>>,
) -> Result<pid_t> {
    if file_actions.is_some_and(has_chdir_action) {
        return unsafe { spawn_via_fork(path, file_actions, attr, argv, envp) };
    }
    unsafe { spawn_message(path, file_actions, attr, argv, envp, false) }
}

unsafe fn spawn_via_fork(
    path: CStr,
    file_actions: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
    attr: Option<&crate::header::spawn::posix_spawnattr_t>,
    argv: NulTerminated<*mut c_char>,
    envp: Option<NulTerminated<*mut c_char>>,
) -> Result<pid_t> {
    match unsafe { fork() }? {
        0 => {
            if let Some(actions) = file_actions
                && apply_file_actions(actions).is_err()
            {
                exit(127);
            }
            let _ = unsafe { spawn_message(path, None, attr, argv, envp, true) };
            exit(127);
        }
        pid => Ok(pid),
    }
}

unsafe fn spawn_message(
    path: CStr,
    file_actions: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
    attr: Option<&crate::header::spawn::posix_spawnattr_t>,
    argv: NulTerminated<*mut c_char>,
    envp: Option<NulTerminated<*mut c_char>>,
    replace: bool,
) -> Result<pid_t> {
    // 8.0 procnto no longer accepts this message: `_PROC_SPAWN_START` moved
    // from subtype 0 to 6 and spawning goes through `_PROC_POSIX_SPAWN`
    // instead. Refuse rather than send a 7.1-shaped request.
    if !super::version::HAS_PROC_SPAWN {
        return Err(Errno(ENOSYS));
    }

    let path = path.to_bytes_with_nul();

    let (fd_map, opened) = match file_actions {
        Some(actions) => build_fd_map(actions)?,
        None => (Vec::new(), Vec::new()),
    };
    let mut parms = build_inheritance(attr, !fd_map.is_empty());
    if replace {
        parms.flags |= spawn_inherit::EXEC;
    }

    // Collect the strings first: the header has to declare their counts and
    // total size before any of them can be sent.
    let mut strings = Vec::new();
    let mut nargv: u16 = 0;
    for arg in argv {
        // SAFETY: the caller guarantees each element is a valid C string.
        let arg = unsafe { CStr::from_ptr(*arg) };
        strings.extend_from_slice(arg.to_bytes_with_nul());
        nargv = nargv.checked_add(1).ok_or(Errno(EINVAL))?;
    }
    let mut narge: u16 = 0;
    if let Some(envp) = envp {
        for var in envp {
            // SAFETY: as above.
            let var = unsafe { CStr::from_ptr(*var) };
            strings.extend_from_slice(var.to_bytes_with_nul());
            narge = narge.checked_add(1).ok_or(Errno(EINVAL))?;
        }
    }

    let header = ProcSpawn {
        type_: proc::SPAWN,
        subtype: 0,
        searchlen: 0,
        pathlen: u16::try_from(path.len()).map_err(|_| Errno(EINVAL))?,
        parms,
        nfds: u16::try_from(fd_map.len()).map_err(|_| Errno(EINVAL))?,
        nargv,
        narge,
        reserved: 0,
        nbytes: u32::try_from(strings.len()).map_err(|_| Errno(EINVAL))?,
    };

    // Header, then the descriptor map, the (empty) search path, the program
    // path, and finally the argument and environment strings.
    let mut message = Vec::new();
    let total = size_of::<ProcSpawn>() + fd_map.len() * 4 + path.len() + strings.len();
    if message.try_reserve_exact(total).is_err() {
        for fd in opened {
            let _ = super::io::close(fd);
        }
        return Err(Errno(ENOMEM));
    }
    // SAFETY: ProcSpawn is a plain-data #[repr(C)] struct.
    message.extend_from_slice(unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(&header).cast::<u8>(),
            size_of::<ProcSpawn>(),
        )
    });
    for entry in &fd_map {
        message.extend_from_slice(&entry.to_ne_bytes());
    }
    message.extend_from_slice(path);
    message.extend_from_slice(&strings);

    let result = unsafe {
        msg_send(
            SYSMGR_COID,
            message.as_ptr(),
            message.len(),
            core::ptr::null_mut(),
            0,
        )
    };

    // The child has its own copies now, so the parent's are no longer needed.
    for fd in opened {
        let _ = super::io::close(fd);
    }

    Ok(result? as pid_t)
}

/// `execve()`, expressed as a spawn that replaces the caller.
///
/// # Safety
///
/// As [`spawn`].
pub unsafe fn execve(
    path: CStr,
    argv: NulTerminated<*mut c_char>,
    envp: Option<NulTerminated<*mut c_char>>,
) -> Result<()> {
    // SPAWN_EXEC replaces the calling process in place, same pid. On success
    // the send does not return.
    unsafe { spawn_message(path, None, None, argv, envp, true) }.map(|_| ())
}

/// `fexecve()`.
pub unsafe fn fexecve(
    fildes: c_int,
    argv: NulTerminated<*mut c_char>,
    envp: Option<NulTerminated<*mut c_char>>,
) -> Result<()> {
    let mut buf = [0u8; connect::PATH_MAX];
    let n = super::io::fpath(fildes, &mut buf)?;
    if n + 1 > buf.len() {
        return Err(Errno(ENOMEM));
    }
    buf[n] = 0;
    let path = CStr::from_bytes_with_nul(&buf[..=n]).map_err(|_| Errno(EINVAL))?;
    unsafe { execve(path, argv, envp) }
}

/// `fchdir()`.
pub fn fchdir(fd: c_int) -> Result<()> {
    let mut buf = [0u8; connect::PATH_MAX];
    let n = super::io::fpath(fd, &mut buf)?;
    if n + 1 > buf.len() {
        return Err(Errno(ENOMEM));
    }
    buf[n] = 0;
    let path = CStr::from_bytes_with_nul(&buf[..=n]).map_err(|_| Errno(EINVAL))?;
    connect::chdir(path)
}

/// `getrlimit()`.
pub fn getrlimit(resource: c_int, mut rlim: Out<rlimit>) -> Result<()> {
    let request = ProcResourceGetlimit {
        type_: proc::RESOURCE,
        subtype: resource::GETLIMIT,
        pid: 0,
        count: 1,
        reserved: 0,
        resource: rlimit_to_qnx(resource)?,
    };
    let mut reply = rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }?;
    reply.rlim_cur = rlim_from_qnx(reply.rlim_cur);
    reply.rlim_max = rlim_from_qnx(reply.rlim_max);
    rlim.write(reply);
    Ok(())
}

/// `setrlimit()`.
pub fn setrlimit(resource: c_int, rlim: &rlimit) -> Result<()> {
    let request = ProcResourceSetlimit {
        type_: proc::RESOURCE,
        subtype: resource::SETLIMIT,
        pid: 0,
        count: 1,
        reserved: 0,
        resource: rlimit_to_qnx(resource)?,
        reserved2: 0,
        rlim_cur: rlim_to_qnx(rlim.rlim_cur),
        rlim_max: rlim_to_qnx(rlim.rlim_max),
    };
    let mut reply = 0u64;
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }.map(|_| ())
}

/// `getrusage()`.
pub fn getrusage(who: c_int, mut r_usage: Out<rusage>) -> Result<()> {
    let qnx_who = match who {
        RUSAGE_SELF => 0u32,
        RUSAGE_CHILDREN => (-1i32) as u32,
        _ => return Err(Errno(EINVAL)),
    };
    let request = ProcResourceUsage {
        type_: proc::RESOURCE,
        subtype: resource::USAGE,
        pid: 0,
        who: qnx_who,
    };
    let mut reply = unsafe { core::mem::zeroed::<rusage>() };
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut reply) }?;
    r_usage.write(reply);
    Ok(())
}

/// `getpriority()`. Pal returns `20 - nice` so the C wrapper can undo it.
pub fn getpriority(which: c_int, who: id_t) -> Result<c_int> {
    if which != PRIO_PROCESS {
        return Ok(20);
    }
    let pid = if who == 0 { 0 } else { who as pid_t };
    let mut param = super::abi::neutrino::SchedParam::default();
    let _policy = unsafe { sched_get(pid, 0, &mut param) }?;
    Ok(10 + param.sched_priority)
}

/// `setpriority()`.
pub fn setpriority(which: c_int, who: id_t, nice: c_int) -> Result<()> {
    if which != PRIO_PROCESS {
        return Ok(());
    }
    let pid = if who == 0 { 0 } else { who as pid_t };
    let mut param = super::abi::neutrino::SchedParam::default();
    let policy = unsafe { sched_get(pid, 0, &mut param) }?;
    param.sched_priority = (10 - nice).clamp(1, 63);
    unsafe { sched_set(pid, 0, policy, &param) }.map(|_| ())
}

fn copy_uts_field(dst: &mut [c_char], src: &[u8]) {
    let n = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let n = n.min(dst.len().saturating_sub(1));
    for (i, byte) in src[..n].iter().enumerate() {
        dst[i] = *byte as c_char;
    }
    if n < dst.len() {
        dst[n] = 0;
    }
}

fn confstr_uts(name: i32) -> Option<[u8; UTSLENGTH]> {
    let request = SysConf {
        type_: 0,
        subtype: sys_conf::GET,
        cmd: sys_conf::CMD_STR,
        name,
        spare: 0,
        value: 0,
    };
    let mut reply = SysConfReply::default();
    let mut text = [0u8; 256];
    let send = super::abi::neutrino::IoVec::new(
        ptr::from_ref(&request).cast::<u8>(),
        size_of::<SysConf>(),
    );
    let mut recv = [
        super::abi::neutrino::IoVec::new(
            ptr::from_mut(&mut reply).cast::<u8>(),
            size_of::<SysConfReply>(),
        ),
        super::abi::neutrino::IoVec::new(text.as_mut_ptr(), text.len()),
    ];
    unsafe {
        msg_sendvnc(
            SYSMGR_COID,
            ptr::from_ref(&send).cast(),
            1,
            recv.as_mut_ptr(),
            2,
        )
    }
    .ok()?;
    let mut out = [0u8; UTSLENGTH];
    let n = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let n = n.min(UTSLENGTH - 1);
    out[..n].copy_from_slice(&text[..n]);
    Some(out)
}

/// `uname()`.
pub fn uname(mut uts: Out<utsname>) -> Result<()> {
    let mut names = utsname {
        sysname: [0; UTSLENGTH],
        nodename: [0; UTSLENGTH],
        release: [0; UTSLENGTH],
        version: [0; UTSLENGTH],
        machine: [0; UTSLENGTH],
        domainname: [0; UTSLENGTH],
    };
    // Vendor `uname` reads confstr ids 11, 2, 3, 4, 5.
    let fields = [
        (&mut names.sysname[..], 11, &b"QNX"[..]),
        (&mut names.nodename[..], 2, &b"localhost"[..]),
        (&mut names.release[..], 3, &b"7.1.0"[..]),
        (&mut names.version[..], 4, &b"7.1.0"[..]),
        (&mut names.machine[..], 5, &b"aarch64"[..]),
    ];
    for (dst, name, fallback) in fields {
        if let Some(got) = confstr_uts(name) {
            copy_uts_field(dst, &got);
        } else {
            copy_uts_field(dst, fallback);
        }
    }
    uts.write(names);
    Ok(())
}

/// `sync()`.
pub fn sync() -> Result<()> {
    let request = ProcEvent {
        type_: proc::EVENT,
        subtype: event::TRIGGER,
        flags: event_flag::SYNC,
        event: [0; 32],
    };
    unsafe {
        msg_send_nc(
            SYSMGR_COID,
            ptr::from_ref(&request).cast::<u8>(),
            size_of::<ProcEvent>(),
            ptr::null_mut(),
            0,
        )
    }
    .map(|_| ())
}

/// `waitpid()`.
///
/// procnto answers `_PROC_WAIT` with the child's `siginfo_t`, as `waitid`
/// returns it; with `WNOHANG` and nothing to report it is left zeroed. The
/// vendor `wait4` always adds `WEXITED | WTRAPPED`.
pub fn waitpid(pid: pid_t, options: c_int) -> Result<(pid_t, c_int)> {
    let (idtype, id) = match pid {
        -1 => (idtype::ALL, 0),
        0 => (idtype::PGID, getpgid(getpid())?),
        pid if pid < 0 => (idtype::PGID, pid.checked_neg().ok_or(Errno(EINVAL))?),
        pid => (idtype::PID, pid),
    };
    if options & !(WNOHANG | WUNTRACED | WCONTINUED) != 0 {
        return Err(Errno(EINVAL));
    }
    let mut qnx_options = wait::EXITED | wait::TRAPPED;
    if options & WNOHANG != 0 {
        qnx_options |= wait::NOHANG;
    }
    if options & WUNTRACED != 0 {
        qnx_options |= wait::STOPPED;
    }
    if options & WCONTINUED != 0 {
        qnx_options |= wait::CONTINUED;
    }
    let request = ProcWait {
        type_: proc::WAIT,
        idtype,
        options: qnx_options,
        id,
    };
    let mut info = Siginfo::default();
    unsafe { msg_send_struct(SYSMGR_COID, &request, &mut info) }?;
    if info.si_signo != qnx_signal::SIGCHLD {
        return Ok((0, 0));
    }
    Ok((info.pid(), wait_status(&info)))
}

/// `_exit()`.
///
/// `ThreadDestroy` with a tid of -1 destroys every thread in the process and
/// records `status` as its exit status, which is exactly the vendor `__exit`:
/// `ThreadDestroy(-1, -1, status)` in a loop. `ThreadDestroyAll` takes no
/// status, so every child used to look like it exited with 0.
pub fn exit(status: c_int) -> ! {
    loop {
        let _ = thread_destroy(-1, -1, status as isize as *mut c_void);
    }
}
