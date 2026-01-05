//! `devctl` command encoding.
//!
//! A QNX `devctl` command packs the transfer direction, the payload size, a
//! device class and a per-class subcode into one 32-bit word. Almost every
//! non-stream device operation — and, as it turns out, most of the socket API —
//! is a `devctl` with the right command word, so encoding these correctly is
//! what makes those callable at all.
//!
//! Decoding one of the vendor's own values confirms the layout: `bind()` in
//! `libsocket.so` issues `devctl(fd, 0x80100601, ...)`, which is direction
//! `TO`, a 16-byte payload (the `sockaddr`), class 6, subcode 1.

/// Payload travels from the caller to the device.
pub const TO: u32 = 0x8000_0000;
/// Payload travels from the device to the caller.
pub const FROM: u32 = 0x4000_0000;
/// Payload travels in both directions.
pub const TOFROM: u32 = TO | FROM;

/// Builds a `devctl` command word.
pub const fn cmd(dir: u32, class: u32, subcode: u32, size: usize) -> u32 {
    dir | (((size as u32) & 0x3fff) << 16) | ((class & 0xff) << 8) | (subcode & 0xff)
}

/// The payload size a command word declares.
pub const fn cmd_size(cmd: u32) -> usize {
    ((cmd >> 16) & 0x3fff) as usize
}

/// Device classes (`_DCMD_*`, from `<devctl.h>`).
#[allow(dead_code)]
pub mod class {
    /// Operations every resource manager understands.
    pub const ALL: u32 = 0x01;
    /// Filesystems (`_DCMD_FSYS`); block devices share it (`_DCMD_BLK`).
    pub const FSYS: u32 = 0x02;
    pub const BLK: u32 = FSYS;
    /// Character devices.
    pub const CHR: u32 = 0x03;
    /// Network interfaces.
    pub const NET: u32 = 0x04;
    /// Miscellaneous.
    pub const MISC: u32 = 0x05;
    /// The TCP/IP stack (`_DCMD_IP`): every socket connection-management call
    /// in the vendor `libsocket.so` is a class-6 `devctl`.
    pub const IP: u32 = 0x06;
    pub const MIXER: u32 = 0x07;
    /// `/proc` and process control.
    pub const PROC: u32 = 0x08;
    /// Memory.
    pub const MEM: u32 = 0x09;
}

/// Class-independent commands from `sys/dcmd_all.h`.
#[allow(dead_code)]
pub mod all {
    use super::{FROM, TO, class, cmd};

    pub const GETFLAGS: u32 = cmd(FROM, class::ALL, 1, 4);
    pub const SETFLAGS: u32 = cmd(TO, class::ALL, 2, 4);
    pub const GETMOUNTFLAGS: u32 = cmd(FROM, class::ALL, 3, 4);
    pub const GETOWN: u32 = cmd(FROM, class::ALL, 4, 4);
    pub const SETOWN: u32 = cmd(TO, class::ALL, 5, 4);
}

/// Filesystem commands from `sys/dcmd_blk.h`.
#[allow(dead_code)]
pub mod fsys {
    use super::{FROM, class, cmd};

    /// `DCMD_FSYS_STATVFS`: class FSYS, subcode 13, 168-byte `__msg_statvfs`.
    pub const STATVFS: u32 = cmd(FROM, class::FSYS, 13, 168);
}
