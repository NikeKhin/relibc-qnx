//! Terminal attributes: QNX's `struct termios` and its translation from
//! relibc's, which is Linux's.
//!
//! The mode flags are numbered alike, both following System V, so the bits
//! relibc names cross unchanged; any others keep the terminal's current value.
//! The control characters sit in different slots, and QNX keeps each speed
//! as a plain baud rate where Linux keeps a `B*` code.

use crate::{
    error::{Errno, Result},
    header::termios::{self as re, speed_t, tcflag_t, termios},
};

pub const NCCS: usize = 40;

/// `struct termios`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_cc: [u8; NCCS],
    pub reserved: [u32; 3],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Termios {
    pub const ZERO: Self = Self {
        c_iflag: 0,
        c_oflag: 0,
        c_cflag: 0,
        c_lflag: 0,
        c_cc: [0; NCCS],
        reserved: [0; 3],
        c_ispeed: 0,
        c_ospeed: 0,
    };
}

/// `c_cc` slots.
pub mod v {
    pub const INTR: usize = 0;
    pub const QUIT: usize = 1;
    pub const ERASE: usize = 2;
    pub const KILL: usize = 3;
    pub const EOF: usize = 4;
    pub const EOL: usize = 5;
    pub const EOL2: usize = 6;
    pub const START: usize = 8;
    pub const STOP: usize = 9;
    pub const SUSP: usize = 10;
    pub const REPRINT: usize = 12;
    pub const DISCARD: usize = 13;
    pub const WERASE: usize = 14;
    pub const LNEXT: usize = 15;
    pub const MIN: usize = 16;
    pub const TIME: usize = 17;
}

/// The standard speeds QNX names (`B*` is the baud rate itself).
pub mod b {
    pub const B0: u32 = 0;
    pub const B50: u32 = 50;
    pub const B75: u32 = 75;
    pub const B110: u32 = 110;
    pub const B134: u32 = 134;
    pub const B150: u32 = 150;
    pub const B200: u32 = 200;
    pub const B300: u32 = 300;
    pub const B600: u32 = 600;
    pub const B1200: u32 = 1200;
    pub const B1800: u32 = 1800;
    pub const B2400: u32 = 2400;
    pub const B4800: u32 = 4800;
    pub const B9600: u32 = 9600;
    pub const B19200: u32 = 19200;
    pub const B38400: u32 = 38400;
    pub const B57600: u32 = 57600;
    pub const B115200: u32 = 115_200;
}

/// Terminal `devctl`s (`<sys/dcmd_chr.h>`).
pub mod chr {
    pub const TCFLOW: u32 = 0x8004_5406;
    pub const TCFLUSH: u32 = 0x8004_7410;
    pub const TCGETATTR: u32 = 0x404c_7413;
    pub const TCSETATTR: u32 = 0x804c_7414;
    pub const TCSETATTRD: u32 = 0x804c_7415;
    pub const TCSETATTRF: u32 = 0x804c_7416;
    pub const TCDRAIN: u32 = 0x0000_745e;
    pub const SETSIZE: u32 = 0x8008_7467;
    pub const GETSIZE: u32 = 0x4008_7468;
    pub const OSCHARS: u32 = 0x4004_7473;
    pub const TCSETPGRP: u32 = 0x8004_7476;
    pub const TCGETPGRP: u32 = 0x4004_7477;
    pub const TCGETSID: u32 = 0x4004_0307;
    pub const ISCHARS: u32 = 0x4004_667f;
}

/// The terminal ioctls without a `DCMD_CHR_*` name (`<sys/ioctl.h>`).
pub mod tioc {
    pub const SCTTY: u32 = 0x0000_7461;
    pub const NOTTY: u32 = 0x0000_7471;
    pub const CBRK: u32 = 0x0000_747a;
    pub const SBRK: u32 = 0x0000_747b;
}

const IFLAG: tcflag_t = (re::IGNBRK
    | re::BRKINT
    | re::IGNPAR
    | re::PARMRK
    | re::INPCK
    | re::ISTRIP
    | re::INLCR
    | re::IGNCR
    | re::ICRNL
    | re::IUCLC
    | re::IXON
    | re::IXANY
    | re::IXOFF
    | re::IMAXBEL
    | re::IUTF8) as tcflag_t;
const OFLAG: tcflag_t = (re::OPOST
    | re::OLCUC
    | re::ONLCR
    | re::OCRNL
    | re::ONOCR
    | re::ONLRET
    | re::OFILL
    | re::OFDEL
    | re::VTDLY) as tcflag_t;
const CFLAG: tcflag_t = (re::CSIZE
    | re::CSTOPB
    | re::CREAD
    | re::PARENB
    | re::PARODD
    | re::HUPCL
    | re::CLOCAL) as tcflag_t;
const LFLAG: tcflag_t = (re::ISIG
    | re::ICANON
    | re::ECHO
    | re::ECHOE
    | re::ECHOK
    | re::ECHONL
    | re::NOFLSH
    | re::TOSTOP
    | re::IEXTEN) as tcflag_t;

/// Linux's `CBAUD | CBAUDEX`: where `c_cflag` repeats the output speed code.
const CBAUD: tcflag_t = 0o010_017;

/// relibc `c_cc` slot to QNX slot.
const CC: [(usize, usize); 16] = [
    (re::VINTR, v::INTR),
    (re::VQUIT, v::QUIT),
    (re::VERASE, v::ERASE),
    (re::VKILL, v::KILL),
    (re::VEOF, v::EOF),
    (re::VTIME, v::TIME),
    (re::VMIN, v::MIN),
    (re::VSTART, v::START),
    (re::VSTOP, v::STOP),
    (re::VSUSP, v::SUSP),
    (re::VEOL, v::EOL),
    (re::VREPRINT, v::REPRINT),
    (re::VDISCARD, v::DISCARD),
    (re::VWERASE, v::WERASE),
    (re::VLNEXT, v::LNEXT),
    (re::VEOL2, v::EOL2),
];

/// relibc speed code to baud rate. Past 115200 QNX has no names, but its
/// speeds are baud rates, so drivers take the number.
const SPEEDS: [(usize, u32); 31] = [
    (re::B0, b::B0),
    (re::B50, b::B50),
    (re::B75, b::B75),
    (re::B110, b::B110),
    (re::B134, b::B134),
    (re::B150, b::B150),
    (re::B200, b::B200),
    (re::B300, b::B300),
    (re::B600, b::B600),
    (re::B1200, b::B1200),
    (re::B1800, b::B1800),
    (re::B2400, b::B2400),
    (re::B4800, b::B4800),
    (re::B9600, b::B9600),
    (re::B19200, b::B19200),
    (re::B38400, b::B38400),
    (re::B57600, b::B57600),
    (re::B115200, b::B115200),
    (re::B230400, 230_400),
    (re::B460800, 460_800),
    (re::B500000, 500_000),
    (re::B576000, 576_000),
    (re::B921600, 921_600),
    (re::B1000000, 1_000_000),
    (re::B1152000, 1_152_000),
    (re::B1500000, 1_500_000),
    (re::B2000000, 2_000_000),
    (re::B2500000, 2_500_000),
    (re::B3000000, 3_000_000),
    (re::B3500000, 3_500_000),
    (re::B4000000, 4_000_000),
];

fn baud(code: speed_t) -> Option<u32> {
    SPEEDS
        .iter()
        .find(|&&(c, _)| c == code as usize)
        .map(|&(_, baud)| baud)
}

/// The code of the nearest standard speed.
fn code(baud: u32) -> speed_t {
    SPEEDS
        .iter()
        .min_by_key(|&&(_, b)| b.abs_diff(baud))
        .map_or(0, |&(c, _)| c as speed_t)
}

pub fn from_kernel(q: &Termios) -> termios {
    let mut t = termios::default();
    let ospeed = code(q.c_ospeed);
    t.c_iflag = q.c_iflag & IFLAG;
    t.c_oflag = q.c_oflag & OFLAG;
    t.c_cflag = (q.c_cflag & CFLAG) | (ospeed & CBAUD);
    t.c_lflag = q.c_lflag & LFLAG;
    for (relibc, qnx) in CC {
        t.c_cc[relibc] = q.c_cc[qnx];
    }
    t.__c_ispeed = code(q.c_ispeed);
    t.__c_ospeed = ospeed;
    t
}

/// Applies relibc's attributes to `q`, which holds the terminal's current
/// ones so that whatever relibc cannot express is left as it was.
pub fn apply(t: &termios, q: &mut Termios) -> Result<()> {
    let ispeed = baud(t.__c_ispeed).ok_or(Errno(crate::header::errno::EINVAL))?;
    let ospeed = baud(t.__c_ospeed).ok_or(Errno(crate::header::errno::EINVAL))?;
    q.c_iflag = (q.c_iflag & !IFLAG) | (t.c_iflag & IFLAG);
    q.c_oflag = (q.c_oflag & !OFLAG) | (t.c_oflag & OFLAG);
    q.c_cflag = (q.c_cflag & !CFLAG) | (t.c_cflag & CFLAG);
    q.c_lflag = (q.c_lflag & !LFLAG) | (t.c_lflag & LFLAG);
    for (relibc, qnx) in CC {
        q.c_cc[qnx] = t.c_cc[relibc];
    }
    q.c_ispeed = ispeed;
    q.c_ospeed = ospeed;
    Ok(())
}
