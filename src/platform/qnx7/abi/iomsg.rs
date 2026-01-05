//! Resource-manager and manager-process message layouts.
//!
//! Every type and constant here was extracted from the DWARF in the SDP's
//! `libc.so.5.sym` with `tools/qnx-dwarf-types.py`, so the layouts are the
//! vendor's own and not a reconstruction. The `/* +n */` offsets in that tool's
//! output are reproduced in the field comments where a struct has padding or
//! reserved words that must be preserved.

use crate::platform::types::*;

/// `_IO_*` message types (`enum _io__Uint16types`).
#[allow(dead_code)]
pub mod msg {
    pub const CONNECT: u16 = 0x100;
    pub const READ: u16 = 0x101;
    pub const WRITE: u16 = 0x102;
    pub const RSVD_CLOSE_OCB: u16 = 0x103;
    pub const STAT: u16 = 0x104;
    pub const NOTIFY: u16 = 0x105;
    pub const DEVCTL: u16 = 0x106;
    pub const RSVD_UNBLOCK: u16 = 0x107;
    pub const PATHCONF: u16 = 0x108;
    pub const LSEEK: u16 = 0x109;
    pub const CHMOD: u16 = 0x10a;
    pub const CHOWN: u16 = 0x10b;
    pub const UTIME: u16 = 0x10c;
    pub const OPENFD: u16 = 0x10d;
    pub const FDINFO: u16 = 0x10e;
    pub const LOCK: u16 = 0x10f;
    pub const SPACE: u16 = 0x110;
    pub const SHUTDOWN: u16 = 0x111;
    pub const MMAP: u16 = 0x112;
    pub const MSG: u16 = 0x113;
    pub const RSVD: u16 = 0x114;
    pub const DUP: u16 = 0x115;
    pub const CLOSE: u16 = 0x116;
    pub const RSVD_LOCK_OCB: u16 = 0x117;
    pub const RSVD_UNLOCK_OCB: u16 = 0x118;
    pub const SYNC: u16 = 0x119;
    pub const POWER: u16 = 0x11a;
    pub const ACL: u16 = 0x11b;
    pub const RSVD_PAUSE: u16 = 0x11c;
    pub const RSVD_UNPAUSE: u16 = 0x11d;
    /// `_IO_READ` with a 64-bit length; the high half goes in the `zero` field.
    pub const READ64: u16 = 0x11e;
    /// `_IO_WRITE` with a 64-bit length; the high half goes in the `zero` field.
    pub const WRITE64: u16 = 0x11f;
    pub const NOTIFY64: u16 = 0x120;
    pub const UTIME64: u16 = 0x121;
}

/// `_IO_XTYPE_*` values from `sys/iomsg.h`.
#[allow(dead_code)]
pub mod xtype {
    pub const NONE: u32 = 0;
    pub const READCOND: u32 = 1;
    pub const MQUEUE: u32 = 2;
    pub const TCPIP: u32 = 3;
    pub const TCPIP_MSG: u32 = 4;
    pub const OFFSET: u32 = 5;
    pub const TCPIP_MSG2: u32 = 7;
}

/// `_IO_OPENFD_*` xtypes.
#[allow(dead_code)]
pub mod openfd {
    pub const NONE: u16 = 0;
    pub const PIPE: u16 = 1;
    pub const KQUEUE: u16 = 2;
    pub const ACCEPT: u16 = 3;
}

/// Path-manager message types (`_PATH_*`), sent to [`PATHMGR_COID`].
///
/// [`PATHMGR_COID`]: super::neutrino::PATHMGR_COID
#[allow(dead_code)]
pub mod path {
    /// `_PATHMGR_BASE`.
    pub const RESOLVE: u16 = 0x70;
    pub const CHDIR: u16 = 0x71;
    pub const CHROOT: u16 = 0x72;
}

/// `_IO_CONNECT` subtypes (`enum _io_connect_subtypes`).
#[allow(dead_code)]
pub mod connect_subtype {
    pub const COMBINE: u16 = 0;
    pub const COMBINE_CLOSE: u16 = 1;
    pub const OPEN: u16 = 2;
    pub const UNLINK: u16 = 3;
    pub const RENAME: u16 = 4;
    pub const MKNOD: u16 = 5;
    pub const READLINK: u16 = 6;
    pub const LINK: u16 = 7;
    pub const RSVD_UNBLOCK: u16 = 8;
    pub const MOUNT: u16 = 9;
}

/// `_IO_CONNECT` reply status bits (`_IO_CONNECT_RET_*`).
///
/// A connect that has not completed replies with a *negative* status — the
/// `FLAG` bit is always set on those — and the remaining bits say what the
/// reply body contains. An ordinary io message never returns a negative
/// status, which is what makes the two cases distinguishable.
#[allow(dead_code)]
pub mod connect_ret {
    /// Always set on a continuation reply.
    pub const FLAG: u32 = 0x8000_0000;
    /// The reply's `umask` field is valid.
    pub const UMASK: u32 = 0x0002_0000;
    /// No controlling terminal is defined.
    pub const NOCTTY: u32 = 0x0004_0000;
    /// The reply's `chroot_len` field is valid.
    pub const CHROOT: u32 = 0x0008_0000;
    /// Connect to the named server and send a fresh message.
    pub const MSG: u32 = 0x0010_0000;
    /// Not fully resolved: follow the link the reply carries.
    pub const LINK: u32 = 0x0001_0000;
    /// The reply is an `_io_connect_ftype_reply`.
    pub const FTYPE: u32 = 0x0000_8000;
    pub const TYPE_MASK: u32 = 0x0001_e000;
}

/// `_io_connect::extra_type` values.
#[allow(dead_code)]
pub mod connect_extra {
    pub const NONE: u8 = 0;
    // LINK is release-specific: version::CONNECT_EXTRA_LINK.
    pub const SYMLINK: u8 = 2;
    pub const MQUEUE: u8 = 3;
    pub const PHOTON: u8 = 4;
    pub const SOCKET: u8 = 5;
    pub const SEM: u8 = 6;
    pub const RESMGR_LINK: u8 = 7;
    pub const PROC_SYMLINK: u8 = 8;
    pub const RENAME: u8 = 9;
    pub const MOUNT: u8 = 10;
    pub const MOUNT_OCB: u8 = 11;
    pub const TYMEM: u8 = 12;
}

/// File type filters carried in `_io_connect::file_type`.
#[allow(dead_code)]
pub mod ftype {
    pub const ANY: u32 = 0;
    pub const FILE: u32 = 1;
    pub const LINK: u32 = 2;
    pub const SYMLINK: u32 = 3;
    pub const PIPE: u32 = 4;
    pub const SHMEM: u32 = 5;
    pub const MQUEUE: u32 = 6;
    pub const SOCKET: u32 = 7;
    pub const SEM: u32 = 8;
    pub const PHOTON: u32 = 9;
    pub const DUMPER: u32 = 10;
    pub const MOUNT: u32 = 11;
    pub const NAME: u32 = 12;
    pub const TYMEM: u32 = 13;
}

/// `_io_connect::eflag` bits (`_IO_CONNECT_EFLAG_*`).
#[allow(dead_code)]
pub mod eflag {
    /// The path referenced a directory.
    pub const DIR: u8 = 0x01;
    /// The last component was `.` or `..`.
    pub const DOT: u8 = 0x02;
    /// The last component was `..`.
    pub const DOTDOT: u8 = 0x04;
}

/// `struct _io_connect`, the request that opens (or unlinks, renames, ...) a
/// path. The NUL-terminated path follows the 40-byte header.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoConnect {
    pub type_: u16,
    pub subtype: u16,
    pub file_type: u32,
    pub reply_max: u16,
    pub entry_max: u16,
    pub key: u32,
    pub handle: u32,
    pub ioflag: u32,
    pub mode: u32,
    pub sflag: u16,
    pub access: u16,
    /// Directory descriptor for `*at()` resolution, `0xffff` for none.
    pub dirfd: u16,
    /// Length of the path that follows, including its NUL terminator.
    pub path_len: u16,
    pub eflag: u8,
    pub extra_type: u8,
    pub extra_len: u16,
}

/// The header of an `_IO_CONNECT` redirect reply.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoConnectLinkReply {
    pub reserved1: u32,
    pub file_type: u32,
    pub eflag: u8,
    pub reserved2: [u8; 1],
    pub chroot_len: u16,
    pub umask: u32,
    /// Number of `IoConnectEntry` records following this header.
    pub nentries: u16,
    /// Length of the (possibly rewritten) path following the entry table.
    pub path_len: u16,
}

/// One candidate manager for a path, following an [`IoConnectLinkReply`].
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoConnectEntry {
    pub nd: u32,
    pub pid: pid_t,
    pub chid: i32,
    pub handle: u32,
    pub key: u32,
    pub file_type: u32,
    /// How much of the path this manager's mountpoint consumed.
    pub prefix_len: u16,
    pub zero: [u16; 3],
}

/// `struct _io_read` / `struct _io_write`; both have the same layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoReadWrite {
    pub type_: u16,
    pub combine_len: u16,
    pub nbytes: u32,
    pub xtype: u32,
    /// Zero for the 32-bit message types, the high half of the length for
    /// `READ64`/`WRITE64`.
    pub zero: u32,
}

/// `struct _io_close`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoClose {
    pub type_: u16,
    pub combine_len: u16,
}

/// `struct _io_lseek`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoLseek {
    pub type_: u16,
    pub combine_len: u16,
    pub whence: i16,
    pub flags: u16,
    pub offset: u64,
}

/// `struct _io_stat`. The reply is a QNX [`Stat`].
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoStat {
    pub type_: u16,
    pub combine_len: u16,
    pub zero: u32,
}

/// `struct _io_chmod`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoChmod {
    pub type_: u16,
    pub combine_len: u16,
    pub mode: u32,
}

/// `struct _io_chown`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoChown {
    pub type_: u16,
    pub combine_len: u16,
    pub gid: i32,
    pub uid: i32,
}

/// `struct _io_sync`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoSync {
    pub type_: u16,
    pub combine_len: u16,
    pub flag: u32,
}

/// `struct _io_space`, used for `ftruncate` and `posix_fallocate`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoSpace {
    pub type_: u16,
    pub combine_len: u16,
    pub subtype: u16,
    pub whence: i16,
    pub start: u64,
    pub len: u64,
}

/// `_io_space` subtypes: the `fcntl` space commands `F_GROWSP`, `F_ALLOCSP`
/// and `F_FREESP` (`_F_OFF + 9/10/11`). The vendor `ftruncate` sends
/// `F_FREESP` (0x6f) and `posix_fallocate` `F_ALLOCSP` (0x6e). An earlier
/// revision sent 1 and 2, which no resource manager accepts.
#[allow(dead_code)]
pub mod space {
    pub const GROW: u16 = 109;
    pub const ALLOC: u16 = 110;
    pub const FREE: u16 = 111;
}

/// `struct _io_devctl`. Payload follows the header in both directions.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoDevctl {
    pub type_: u16,
    pub combine_len: u16,
    pub dcmd: i32,
    pub nbytes: u32,
    pub zero: i32,
}

/// `struct _io_dup`, whose body is a `_msg_info32` identifying the caller.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoDup {
    pub type_: u16,
    pub combine_len: u16,
    pub info: MsgInfo32,
    pub reserved: u32,
    pub key: u32,
}

/// `struct _msg_info32`, the 32-bit-compatible message origin record embedded
/// in resource-manager messages.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MsgInfo32 {
    pub nd: u32,
    pub srcnd: u32,
    pub pid: pid_t,
    pub tid: i32,
    pub chid: i32,
    pub scoid: i32,
    pub coid: i32,
    pub msglen: i32,
    pub srcmsglen: i32,
    pub dstmsglen: i32,
    pub priority: i16,
    pub flags: i16,
    pub type_id: u32,
}

/// QNX's `struct stat`, which shares no layout with relibc's and must be
/// converted at the boundary.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Stat {
    pub st_ino: u64,
    pub st_size: i64,
    pub st_dev: u32,
    pub st_rdev: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub old_st_mtime: i32,
    pub old_st_atime: i32,
    pub old_st_ctime: i32,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_blocksize: i32,
    pub st_nblocks: i32,
    pub st_blksize: i32,
    pub st_blocks: i64,
    /// `st_mtim`, overlaid with the 64-bit `st_mtime` in the vendor header.
    pub st_mtim: QnxTimespec,
    pub st_atim: QnxTimespec,
    pub st_ctim: QnxTimespec,
}

/// QNX's `struct timespec` as embedded in [`Stat`], where each field occupies
/// eight bytes.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct QnxTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// QNX's `struct dirent`, returned in `_IO_READ` replies on a directory.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct QnxDirent {
    pub d_ino: u64,
    pub d_offset: i64,
    pub d_reclen: i16,
    pub d_namelen: i16,
    // `char d_name[]` follows.
}

/// Process-manager message types, sent to [`SYSMGR_COID`].
///
/// [`SYSMGR_COID`]: super::neutrino::SYSMGR_COID
#[allow(dead_code)]
pub mod proc {
    pub const SPAWN: u16 = 0x10;
    pub const WAIT: u16 = 0x11;
    pub const FORK: u16 = 0x12;
    pub const GETSETID: u16 = 0x13;
    pub const SETPGID: u16 = 0x14;
    pub const UMASK: u16 = 0x15;
    pub const SESSION: u16 = 0x17;
    pub const DAEMON: u16 = 0x18;
    pub const EVENT: u16 = 0x19;
    pub const RESOURCE: u16 = 0x1a;
    pub const POSIX_SPAWN: u16 = 0x1b;
    pub const POSIX_SPAWNP: u16 = 0x1c;
    pub const VALUE: u16 = 0x1d;
    pub const TIMER_TOLERANCE: u16 = 0x1e;
    pub const MAC: u16 = 0x20;
    pub const SERVMON: u16 = 0x21;
}

/// `struct _proc_getsetid`, the message behind `getuid`/`setuid` and friends.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcGetSetId {
    pub type_: u16,
    pub subtype: u16,
    pub pid: pid_t,
    pub eid: i32,
    pub rid: i32,
    pub ngroups: i32,
}

/// `_proc_getsetid` subtypes, in `sys/procmsg.h` enum order.
#[allow(dead_code)]
pub mod getsetid {
    pub const GETID: u16 = 0;
    pub const SETUID: u16 = 1;
    pub const SETGID: u16 = 2;
    pub const SETEUID: u16 = 3;
    pub const SETEGID: u16 = 4;
    pub const SETREUID: u16 = 5;
    pub const SETREGID: u16 = 6;
    pub const SETGROUPS: u16 = 7;
    pub const GETID_NO_CRED: u16 = 8;
}

/// `struct _proc_getsetid_reply`. The credential block that follows is
/// variable-length; callers that only need pids can ignore it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcGetSetIdReply {
    pub zero: u32,
    pub pgrp: pid_t,
    pub ppid: pid_t,
    pub sid: pid_t,
    pub orig_ppid: pid_t,
    pub reserved: [u32; 2],
}

/// `struct _proc_umask`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcUmask {
    pub type_: u16,
    pub subtype: u16,
    pub umask: u32,
    pub pid: pid_t,
}

/// `_proc_umask` subtypes.
#[allow(dead_code)]
pub mod umask {
    pub const SET: u16 = 0;
    pub const GET: u16 = 1;
}

/// `struct _proc_wait`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcWait {
    pub type_: u16,
    pub idtype: i16,
    pub options: i32,
    pub id: i32,
}

/// `idtype_t` (`<sys/wait.h>`).
pub mod idtype {
    pub const ALL: i16 = 0;
    pub const PID: i16 = 1;
    pub const PGID: i16 = 2;
}

/// `_PROC_WAIT` option bits (`<sys/wait.h>`), which relibc numbers as Linux
/// does.
pub mod wait {
    pub const EXITED: i32 = 0x01;
    pub const TRAPPED: i32 = 0x02;
    pub const STOPPED: i32 = 0x04;
    pub const CONTINUED: i32 = 0x08;
    pub const NOHANG: i32 = 0x40;
    pub const NOWAIT: i32 = 0x80;
}

/// `struct _proc_session`, which backs `setsid`, `getsid` and `setpgid`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcSession {
    pub type_: u16,
    pub xid: u16,
    pub sid: pid_t,
    pub id: i32,
    pub event: u32,
}

/// `_proc_session` transaction ids.
#[allow(dead_code)]
pub mod session {
    pub const GETSID: u16 = 0;
    pub const SETSID: u16 = 1;
    pub const GETPGRP: u16 = 2;
    pub const SETPGRP: u16 = 3;
}

/// Memory-manager message types, sent to [`SYSMGR_COID`].
///
/// [`SYSMGR_COID`]: super::neutrino::SYSMGR_COID
#[allow(dead_code)]
pub mod mem {
    pub const MAP: u16 = 0x40;
    pub const CTRL: u16 = 0x41;
    pub const INFO: u16 = 0x42;
    pub const OFFSET: u16 = 0x43;
    pub const DEBUG_INFO: u16 = 0x44;
    pub const SWAP: u16 = 0x45;
    pub const PMEM_ADD: u16 = 0x46;
    pub const PEER: u16 = 0x47;
}

/// `struct _mem_map`, the request behind `mmap`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemMap {
    pub type_: u16,
    pub zero: u16,
    pub reserved1: u32,
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub flags: u32,
    pub fd: i32,
    pub preload: u32,
    pub align: u64,
    pub offset: i64,
    pub shmem_handle: u64,
    pub elf_pvaddr: u64,
}

/// `struct _mem_map_reply`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemMapReply {
    pub real_size: u64,
    pub real_addr: u64,
    pub addr: u64,
}

/// `_mem_ctrl` subtypes, in `sys/memmsg.h` enum order.
#[allow(dead_code)]
pub mod mem_ctrl {
    pub const UNMAP: u16 = 0;
    pub const PROTECT: u16 = 1;
    pub const SYNC: u16 = 2;
    pub const LOCKALL: u16 = 3;
    pub const UNLOCKALL: u16 = 4;
    pub const LOCK: u16 = 5;
    pub const UNLOCK: u16 = 6;
    pub const ADVISE: u16 = 7;
}

/// `struct _mem_ctrl`, the request behind `munmap`, `mprotect` and `msync`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemCtrl {
    pub type_: u16,
    pub subtype: u16,
    pub flags: u32,
    pub addr: u64,
    pub len: u64,
}

/// `struct _proc_fork`.
///
/// A `frame` of zero lets procnto capture the calling thread's register state
/// itself, which is what the vendor `_fork` does.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcFork {
    pub type_: u16,
    pub zero: u16,
    pub flags: u32,
    pub frame: u64,
}

/// `_proc_fork::flags`.
///
/// The vendor `fork()` is `_fork(1, 0)`, so bit zero is what distinguishes a
/// POSIX `fork()` — which must run the `pthread_atfork` handlers — from the
/// lower-level primitive.
#[allow(dead_code)]
pub mod fork_flags {
    pub const ATFORK: u32 = 0x1;
}

/// `struct __msg_inheritance`, the credential and scheduling block embedded in
/// a spawn request.
///
/// The layout is pinned by the surrounding `_proc_spawn`, whose DWARF places
/// `parms` at offset 8 and `nfds` at 96 — so this is exactly 88 bytes. QNX's
/// `sigset_t` is `_Uint32t __bits[2]`, and `struct __sched_param` is 48 bytes,
/// which accounts for all of them.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MsgInheritance {
    /// `SPAWN_*` flags.
    pub flags: u32,
    pub pgroup: pid_t,
    pub sigmask: [u32; 2],
    pub sigdefault: [u32; 2],
    pub sigignore: [u32; 2],
    pub stack_max: u32,
    pub policy: u32,
    pub param: SchedParam,
}

/// `struct __sched_param` as embedded in a spawn request.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SchedParam {
    pub sched_priority: i32,
    pub sched_curpriority: i32,
    /// The `__ss_un` union: the sporadic-scheduling parameters, unused for
    /// the round-robin and FIFO policies.
    pub ss_un: [u8; 40],
}

impl Default for SchedParam {
    fn default() -> Self {
        Self {
            sched_priority: 0,
            sched_curpriority: 0,
            ss_un: [0; 40],
        }
    }
}

/// `struct inheritance::flags` values (`SPAWN_*`, from `<spawn.h>`). An earlier
/// revision numbered these sequentially, so `SPAWN_EXEC` went out as
/// `SPAWN_SETSIGIGN` and `execve` spawned a child instead of replacing the
/// caller.
#[allow(dead_code)]
pub mod spawn_inherit {
    pub const SETGROUP: u32 = 0x0000_0001;
    pub const SETSIGMASK: u32 = 0x0000_0002;
    pub const SETSIGDEF: u32 = 0x0000_0004;
    pub const SETSIGIGN: u32 = 0x0000_0008;
    pub const TCSETPGROUP: u32 = 0x0000_0080;
    pub const SETSID: u32 = 0x0000_0200;
    pub const EXPLICIT_SCHED: u32 = 0x0000_0400;
    pub const EXPLICIT_CPU: u32 = 0x0000_0800;
    pub const NOZOMBIE: u32 = 0x0000_2000;
    pub const DEBUG: u32 = 0x0000_4000;
    pub const HOLD: u32 = 0x0000_8000;
    pub const EXEC: u32 = 0x0001_0000;
    pub const SEARCH_PATH: u32 = 0x0002_0000;
    pub const CHECK_SCRIPT: u32 = 0x0004_0000;
    pub const ALIGN_DEFAULT: u32 = 0x0000_0000;
    pub const ALIGN_FAULT: u32 = 0x0100_0000;
    pub const ALIGN_NOFAULT: u32 = 0x0200_0000;
}

/// `struct _proc_spawn`. The variable-length tail is, in order: the descriptor
/// map, the search path, the program path, then the argument and environment
/// strings.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcSpawn {
    pub type_: u16,
    pub subtype: u16,
    pub searchlen: u16,
    pub pathlen: u16,
    pub parms: MsgInheritance,
    pub nfds: u16,
    pub nargv: u16,
    pub narge: u16,
    pub reserved: u16,
    /// Total bytes of `argv` plus `arge`, including their NUL terminators.
    pub nbytes: u32,
}

/// `_proc_spawn::parms.flags` (`_PROC_SPAWN_FD_*`).
#[allow(dead_code)]
pub mod spawn_flags {
    /// Pass only the descriptors in the map rather than all of them.
    pub const FD_LIST: u32 = 0x0001;
    /// Do not pass descriptors marked close-on-exec.
    pub const FD_NOCLOEXEC: u32 = 0x0002;
    /// Start allocating the child's descriptors from the map's base.
    pub const FD_BASE: u32 = 0x0004;
}

/// `struct _io_utime`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoUtime {
    pub type_: u16,
    pub combine_len: u16,
    pub cur_flag: i32,
}

/// `struct _io_utime64`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoUtime64 {
    pub type_: u16,
    pub combine_len: u16,
    pub zero: i32,
    pub atime_s: i64,
    pub atime_ns: i64,
    pub mtime_s: i64,
    pub mtime_ns: i64,
}

/// `struct _io_openfd`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoOpenfd {
    pub type_: u16,
    pub combine_len: u16,
    pub ioflag: u32,
    pub sflag: u16,
    pub xtype: u16,
    pub info: MsgInfo32,
    pub reserved2: u32,
    pub key: u32,
}

/// `struct _io_fdinfo`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoFdinfo {
    pub type_: u16,
    pub combine_len: u16,
    pub flags: u32,
    pub path_len: u32,
    pub reserved: u32,
}

/// `struct _fdinfo`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FdInfo {
    pub mode: u32,
    pub ioflag: u32,
    pub offset: u64,
    pub size: u64,
    pub flags: u32,
    pub sflag: u16,
    pub count: u16,
    pub rcount: u16,
    pub wcount: u16,
    pub rlocks: u16,
    pub wlocks: u16,
    pub zero: [u32; 6],
}

/// `struct _io_fdinfo_reply` without the trailing path.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoFdinfoReply {
    pub zero: [u32; 2],
    pub info: FdInfo,
}

/// `struct _io_lock`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoLock {
    pub type_: u16,
    pub combine_len: u16,
    pub subtype: u32,
    pub nbytes: u32,
}

/// QNX `F_*LK` commands used as `_io_lock::subtype` (`_F_OFF + 14/6/7`).
#[allow(dead_code)]
pub mod lock_cmd {
    pub const GETLK: u32 = 0x72;
    pub const SETLK: u32 = 0x6a;
    pub const SETLKW: u32 = 0x6b;
}

/// QNX `l_type` values for POSIX record locks (`fcntl` `F_SETLK` and
/// friends): `F_RDLCK`, `F_WRLCK`, `F_UNLCK` from `<fcntl.h>`.
#[allow(dead_code)]
pub mod lock_type {
    pub const RDLCK: i16 = 1;
    pub const WRLCK: i16 = 2;
    pub const UNLCK: i16 = 3;
}

/// `l_type` values the vendor `flock()` sends for BSD whole-file locks, owned
/// by the open file description rather than the process: shared 0x81,
/// exclusive 0x84, unlock 0x83. These are not `<fcntl.h>` values and must
/// not be used for `fcntl` record locks, which an earlier revision did.
#[allow(dead_code)]
pub mod flock_type {
    pub const SH: i16 = 0x81;
    pub const EX: i16 = 0x84;
    pub const UN: i16 = 0x83;
}

/// QNX `struct flock` as it travels in `_IO_LOCK`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct QnxFlock {
    pub l_type: i16,
    pub l_whence: i16,
    /// Explicit, so no stack bytes travel to the resource manager.
    pub pad: u32,
    pub l_start: i64,
    pub l_len: i64,
    pub l_pid: i32,
    pub l_sysid: u32,
}

/// `struct _io_msg`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IoMsg {
    pub type_: u16,
    pub combine_len: u16,
    pub mgrid: u16,
    pub subtype: u16,
}

/// `_IOMGR_*` manager ids from `sys/iomgr.h`.
#[allow(dead_code)]
pub mod iomgr {
    pub const FSYS: u16 = 2;
    pub const TCPIP: u16 = 6;
}

/// `_IO_SOCK_*` subtypes from `sys/sockmsg.h`.
#[allow(dead_code)]
pub mod sock_sub {
    pub const SOPT2: u16 = 5;
    pub const GOPT2: u16 = 6;
}

/// `struct __msg_statvfs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsgStatvfs {
    pub f_bsize: u32,
    pub f_frsize: u32,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_favail: u64,
    pub f_fsid: u32,
    pub f_basetype: [u8; 16],
    pub f_flag: u32,
    pub f_namemax: u32,
    pub f_filler: [u32; 21],
}

impl Default for MsgStatvfs {
    fn default() -> Self {
        Self {
            f_bsize: 0,
            f_frsize: 0,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_basetype: [0; 16],
            f_flag: 0,
            f_namemax: 0,
            f_filler: [0; 21],
        }
    }
}

/// `_proc_event` subtypes.
#[allow(dead_code)]
pub mod event {
    pub const NOTIFY: u16 = 0;
    pub const TRIGGER: u16 = 1;
}

/// `PROCMGR_EVENT_*` flags.
#[allow(dead_code)]
pub mod event_flag {
    pub const SYNC: u32 = 0x0000_0001;
}

/// `struct _proc_event`. The trailing sigevent is unused for `TRIGGER`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcEvent {
    pub type_: u16,
    pub subtype: u16,
    pub flags: u32,
    pub event: [u8; 32],
}

impl Default for ProcEvent {
    fn default() -> Self {
        Self {
            type_: 0,
            subtype: 0,
            flags: 0,
            event: [0; 32],
        }
    }
}

/// `_proc_resource` subtypes.
#[allow(dead_code)]
pub mod resource {
    pub const USAGE: u16 = 0;
    pub const GETLIMIT: u16 = 1;
    pub const SETLIMIT: u16 = 2;
}

/// `struct _proc_resource_usage`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcResourceUsage {
    pub type_: u16,
    pub subtype: u16,
    pub pid: pid_t,
    pub who: u32,
}

/// `struct _proc_resource_getlimit` for a single resource.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcResourceGetlimit {
    pub type_: u16,
    pub subtype: u16,
    pub pid: pid_t,
    pub count: u32,
    pub reserved: u32,
    pub resource: u32,
}

/// One `_proc_resource_setlimit` entry plus its header.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProcResourceSetlimit {
    pub type_: u16,
    pub subtype: u16,
    pub pid: pid_t,
    pub count: u32,
    pub reserved: u32,
    pub resource: u32,
    pub reserved2: u32,
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

/// `_SYS_CONF` / `_sys_conf`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SysConf {
    pub type_: u16,
    pub subtype: u16,
    pub cmd: i32,
    pub name: i32,
    pub spare: i32,
    pub value: i64,
}

/// `_sys_conf` subtypes.
#[allow(dead_code)]
pub mod sys_conf {
    pub const GET: u16 = 0;
    pub const SET: u16 = 1;
    /// `_CONF_STR`: the value is a string that follows the reply header.
    pub const CMD_STR: i32 = 0x0010_0000;
}

/// `struct _sys_conf_reply` without the trailing string.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SysConfReply {
    pub zero: [u32; 3],
    pub match_: i32,
    pub value: i64,
}

/// Maximum supplementary groups a `_PROC_GETSETID` reply will carry.
pub const PROC_NGROUPS_MAX: usize = 384;
