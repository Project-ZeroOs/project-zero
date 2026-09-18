//! Project Zero - Stage 3I Syscall Numbers and Error Definitions

pub const SYS_EXIT: u64            = 1;
pub const SYS_YIELD: u64           = 2;
pub const SYS_CHANNEL_CREATE: u64  = 3;
pub const SYS_CHANNEL_SEND: u64    = 4;
pub const SYS_CHANNEL_RECEIVE: u64 = 5;
pub const SYS_CHANNEL_CLOSE: u64   = 6;
pub const SYS_SHM_CREATE: u64      = 7;
pub const SYS_SHM_MAP: u64         = 8;
pub const SYS_SHM_UNMAP: u64       = 9;
pub const SYS_CAP_DERIVE: u64      = 10;
pub const SYS_FILE_OPEN: u64       = 11;
pub const SYS_FILE_READ: u64       = 12;
pub const SYS_FILE_WRITE: u64      = 13;
pub const SYS_FILE_CLOSE: u64      = 14;
pub const SYS_FILE_STAT: u64       = 15;
pub const SYS_FILE_SYNC: u64       = 16;
pub const SYS_DEV_QUERY: u64       = 17;
pub const SYS_DEV_MAP_MMIO: u64    = 18;
pub const SYS_DEV_DMA_ALLOC: u64   = 19;
pub const SYS_DEV_RESET: u64       = 20;
pub const SYS_DEV_BIND_IRQ: u64    = 21;
pub const SYS_NET_SOCKET: u64      = 22;
pub const SYS_NET_BIND: u64        = 23;
pub const SYS_NET_LISTEN: u64      = 24;
pub const SYS_NET_ACCEPT: u64      = 25;
pub const SYS_NET_CONNECT: u64     = 26;
pub const SYS_NET_SEND: u64        = 27;
pub const SYS_NET_RECV: u64        = 28;
pub const SYS_NET_CLOSE: u64       = 29;
pub const SYS_NET_QUERY: u64       = 30;
pub const SYS_NET_CONFIG: u64      = 31;

/// Authoritative ZeroOS Syscall Error Codes
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError {
    Success             = 0,
    InvalidSyscall      = -1,  // -ENOSYS: Syscall number unrecognized
    InvalidArgument     = -2,  // -EINVAL: Malformed parameters or flags
    BadHandle           = -3,  // -EBADF: Handle not occupied or stale generation
    PermissionDenied    = -4,  // -EACCES: Lacking required capability right
    BadAddress          = -5,  // -EFAULT: Non-user, non-canonical, or unmapped pointer
    OutOfMemory         = -6,  // -ENOMEM: Kernel table or PMM frames exhausted
    ResourceBusy        = -7,  // -EBUSY: Channel ring full or resource pinned
    NotFound            = -8,  // -ENOENT: Process/thread/object not found
    WouldBlock          = -9,  // -EAGAIN: Non-blocking call would block
    PeerClosed          = -10, // -EPIPE: Channel peer endpoint closed
    NoSpace             = -11, // -ENOSPC: Volume or block bitmap full
    FileExists          = -12, // -EEXIST: File already exists
    IoError             = -13, // -EIO: Device I/O error or timeout
    NotADirectory       = -14, // -ENOTDIR: Expected directory capability
    IsADirectory        = -15, // -EISDIR: Is a directory
    TableFull           = -16, // -ENFILE: Open storage object table full
    ResourceConflict    = -17, // -EEXIST/-EBUSY: Hardware resource overlap or conflict
    DeviceFault         = -18, // -EFAULT/-EIO: Hardware controller faulted or unresponsive
    NetworkDown         = -19, // -ENETDOWN: Interface or physical device down/faulted
    NetworkUnreachable  = -20, // -ENETUNREACH: No route to destination host
    ConnectionRefused   = -21, // -ECONNREFUSED: Peer rejected connection (TCP RST)
    ConnectionReset     = -22, // -ECONNRESET: Established connection reset by peer
    ConnectionAborted   = -23, // -ECONNABORTED: Connection aborted locally
    AlreadyConnected    = -24, // -EISCONN: Socket already connected
    NotConnected        = -25, // -ENOTCONN: Operation requires connected socket
    AddressInUse        = -26, // -EADDRINUSE: Local port/IP binding collision
    AddressNotAvailable = -27, // -EADDRNOTAVAIL: Requested IP address not assigned
    TimedOut            = -28, // -ETIMEDOUT: Connection or transmission timed out
    MessageTooLarge     = -29, // -EMSGSIZE: Packet exceeds MTU (DF=1)
}

impl SyscallError {
    #[inline(always)]
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}
