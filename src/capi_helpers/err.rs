use std::io;
use std::sync::PoisonError;

use libnx_rs::LibnxError;
use scsi::ScsiError;
use mbr_nostd::MbrError;
pub const SUCCESS : u32 = 0;

pub const NX_FATDRIVE_ERR_MODULE : u32 = 0xFA;


pub const NX_FATDRIVE_ERR_NOT_IMPLEMENTED : u32 = 0x1FA;
pub const NX_FATDRIVE_ERR_NOT_INITIALIZED : u32 = 0x2FA;
pub const NX_FATDRIVE_ERR_DRIVE_NOT_FOUND : u32 = 0x3FA;
pub const NX_FATDRIVE_ERR_POISSENED_MUTEX : u32 = 0x4FA;
pub const NX_FATDRIVE_ERR_DRIVE_DISCONNECTED : u32 = 0x6FA;

pub const NX_FATDRIVE_ERR_STDIO_PREFIX : u32 = 0x2_0000;

pub const NX_FATDRIVE_ERR_FS_PREFIX : u32 = 0x4_0000;
pub const NX_FATDRIVE_ERR_FILE_NOT_FOUND : u32 = ( (NX_FATDRIVE_ERR_FS_PREFIX + 1) << 8 ) + NX_FATDRIVE_ERR_MODULE;

pub const NX_FATDRIVE_ERR_SCSI_PREFIX : u32 = 0x5_0000;
pub const NX_FATDRIVE_ERR_MBR_PREFIX : u32 = 0x6_0000;



pub const NX_FATDRIVE_ERR_UNKNOWN : u32 = 0xFFFFFE00 + NX_FATDRIVE_ERR_MODULE;


pub trait LibnxErrMapper {
    fn map(err : Self) -> u32;
}

impl LibnxErrMapper for io::Error {
    fn map(err : io::Error) -> u32 {
        let offset : u32 = match err.kind() {
                io::ErrorKind::NotFound => 1,
                io::ErrorKind::PermissionDenied => 2,
                io::ErrorKind::ConnectionRefused => 3,
                io::ErrorKind::ConnectionReset => 4,
                io::ErrorKind::NotConnected => 5,
                io::ErrorKind::AddrInUse => 6,
                io::ErrorKind::AddrNotAvailable => 7,
                io::ErrorKind::BrokenPipe => 8,
                io::ErrorKind::AlreadyExists => 9,
                io::ErrorKind::WouldBlock => 10,
                io::ErrorKind::InvalidInput => 11,
                io::ErrorKind::InvalidData => 12,
                io::ErrorKind::TimedOut => 13,
                io::ErrorKind::WriteZero => 14,
                io::ErrorKind::Interrupted => 15,
                io::ErrorKind::Other => 16,
                io::ErrorKind::UnexpectedEof => 17,
                io::ErrorKind::ConnectionAborted => 18,
                _ => 0xFFFF,
        };
        ((NX_FATDRIVE_ERR_STDIO_PREFIX + offset) << 8) + NX_FATDRIVE_ERR_MODULE
    }
}

impl LibnxErrMapper for u32 {
    fn map(raw : u32) -> u32 {
        raw
    }
}

impl <T> LibnxErrMapper for PoisonError<T> {
    fn map(_err : PoisonError<T>) -> u32 {
        NX_FATDRIVE_ERR_POISSENED_MUTEX 
    }
}

impl LibnxErrMapper for LibnxError {
    fn map(err : LibnxError) -> u32 {
        err.error_code.unwrap_or(NX_FATDRIVE_ERR_UNKNOWN ) 
    }
}

impl LibnxErrMapper for ScsiError {
    fn map(err : ScsiError) -> u32 {
        let desc : u32 = match err.cause {
            scsi::ErrorCause::ParseError => 0x1000,
            scsi::ErrorCause::NonBlocksizeMultipleLengthError{..} => 0x2000,
            scsi::ErrorCause::UsbTransferError{direction} => {
                match direction {
                    scsi::UsbTransferDirection::In => 0x3001,
                    scsi::UsbTransferDirection::Out => 0x3002,
                    _ => 0x3003
                }
            },
            scsi::ErrorCause::FlagError {..} => 0x4000,
            scsi::ErrorCause::BufferTooSmallError{..} => 0x5000,
            scsi::ErrorCause::UnsupportedOperationError => 0x6000,
            scsi::ErrorCause::InvalidDeviceError => 0x7000,
        };
        ((desc + NX_FATDRIVE_ERR_SCSI_PREFIX) << 8) + NX_FATDRIVE_ERR_MODULE
    }
}

impl LibnxErrMapper for MbrError {
    fn map(err : MbrError) -> u32 {
        let desc : u32 = match err.cause {
            mbr_nostd::ErrorCause::UnsupportedPartitionError{tag : tag} => 0x1000 + ((tag as u32) << 8),
            mbr_nostd::ErrorCause::InvalidMBRSuffix{actual : actual} => 0x2000 + ((actual[0] as u32) << 8) + ((actual[1] as u32) << 4),
            mbr_nostd::ErrorCause::BufferWrongSizeError{..} => 0x3000, 
        };
        ((desc + NX_FATDRIVE_ERR_MBR_PREFIX) << 8) + NX_FATDRIVE_ERR_MODULE
    }
}

pub mod errno {
    pub const EPERM :i32 = 1;   /* Not owner */
    pub const ENOENT :i32 = 2;   /* No such file or directory */
    pub const ESRCH :i32 = 3;   /* No such process */
    pub const EINTR :i32 = 4;   /* Interrupted system call */
    pub const EIO :i32 = 5;   /* I/O error */
    pub const ENXIO :i32 = 6;   /* No such device or address */
    pub const E2BIG :i32 = 7;   /* Arg list too long */
    pub const ENOEXEC :i32 = 8;   /* Exec format error */
    pub const EBADF :i32 = 9;   /* Bad file number */
    pub const ECHILD :i32 = 10;   /* No children */
    pub const EAGAIN :i32 = 11;   /* No more processes */
    pub const ENOMEM :i32 = 12;   /* Not enough space */
    pub const EACCES :i32 = 13;   /* Permission denied */
    pub const EFAULT :i32 = 14;   /* Bad address */
    pub const ENOTBLK :i32 = 15;   /* Block device required */
    pub const EBUSY :i32 = 16;   /* Device or resource busy */
    pub const EEXIST :i32 = 17;   /* File exists */
    pub const EXDEV :i32 = 18;   /* Cross-device link */
    pub const ENODEV :i32 = 19;   /* No such device */
    pub const ENOTDIR :i32 = 20;   /* Not a directory */
    pub const EISDIR :i32 = 21;   /* Is a directory */
    pub const EINVAL :i32 = 22;   /* Invalid argument */
    pub const ENFILE :i32 = 23;   /* Too many open files in system */
    pub const EMFILE :i32 = 24;   /* File descriptor value too large */
    pub const ENOTTY :i32 = 25;   /* Not a character device */
    pub const ETXTBSY :i32 = 26;   /* Text file busy */
    pub const EFBIG :i32 = 27;   /* File too large */
    pub const ENOSPC :i32 = 28;   /* No space left on device */
    pub const ESPIPE :i32 = 29;   /* Illegal seek */
    pub const EROFS :i32 = 30;   /* Read-only file system */
    pub const EMLINK :i32 = 31;   /* Too many links */
    pub const EPIPE :i32 = 32;   /* Broken pipe */
    pub const EDOM :i32 = 33;   /* Mathematics argument out of domain of function */
    pub const ERANGE :i32 = 34;   /* Result too large */
    pub const ENOMSG :i32 = 35;   /* No message of desired type */
    pub const EIDRM :i32 = 36;   /* Identifier removed */
    pub const ECHRNG :i32 = 37;   /* Channel number out of range */
    pub const EL2NSYNC :i32 = 38;   /* Level 2 not synchronized */
    pub const EL3HLT :i32 = 39;   /* Level 3 halted */
    pub const EL3RST :i32 = 40;   /* Level 3 reset */
    pub const ELNRNG :i32 = 41;   /* Link number out of range */
    pub const EUNATCH :i32 = 42;   /* Protocol driver not attached */
    pub const ENOCSI :i32 = 43;   /* No CSI structure available */
    pub const EL2HLT :i32 = 44;   /* Level 2 halted */
    pub const EDEADLK :i32 = 45;   /* Deadlock */
    pub const ENOLCK :i32 = 46;   /* No lock */
    pub const EBADE :i32 = 50;   /* Invalid exchange */
    pub const EBADR :i32 = 51;   /* Invalid request descriptor */
    pub const EXFULL :i32 = 52;   /* Exchange full */
    pub const ENOANO :i32 = 53;   /* No anode */
    pub const EBADRQC :i32 = 54;   /* Invalid request code */
    pub const EBADSLT :i32 = 55;   /* Invalid slot */
    pub const EDEADLOCK :i32 = 56;   /* File locking deadlock error */
    pub const EBFONT :i32 = 57;   /* Bad font file fmt */
    pub const ENOSTR :i32 = 60;   /* Not a stream */
    pub const ENODATA :i32 = 61;   /* No data (for no delay io) */
    pub const ETIME :i32 = 62;   /* Stream ioctl timeout */
    pub const ENOSR :i32 = 63;   /* No stream resources */
    pub const ENONET :i32 = 64;   /* Machine is not on the network */
    pub const ENOPKG :i32 = 65;   /* Package not installed */
    pub const EREMOTE :i32 = 66;   /* The object is remote */
    pub const ENOLINK :i32 = 67;   /* Virtual circuit is gone */
    pub const EADV :i32 = 68;   /* Advertise error */
    pub const ESRMNT :i32 = 69;   /* Srmount error */
    pub const ECOMM :i32 = 70;   /* Communication error on send */
    pub const EPROTO :i32 = 71;   /* Protocol error */
    pub const EMULTIHOP :i32 = 74;   /* Multihop attempted */
    pub const ELBIN :i32 = 75;   /* Inode is remote (not really error) */
    pub const EDOTDOT :i32 = 76;   /* Cross mount point (not really error) */
    pub const EBADMSG :i32 = 77;   /* Bad message */
    pub const EFTYPE :i32 = 79;   /* Inappropriate file type or format */
    pub const ENOTUNIQ :i32 = 80;   /* Given log. name not unique */
    pub const EBADFD :i32 = 81;   /* f.d. invalid for this operation */
    pub const EREMCHG :i32 = 82;   /* Remote address changed */
    pub const ELIBACC :i32 = 83;   /* Can't access a needed shared lib */
    pub const ELIBBAD :i32 = 84;   /* Accessing a corrupted shared lib */
    pub const ELIBSCN :i32 = 85;   /* .lib section in a.out corrupted */
    pub const ELIBMAX :i32 = 86;   /* Attempting to link in too many libs */
    pub const ELIBEXEC :i32 = 87;   /* Attempting to exec a shared library */
    pub const ENOSYS :i32 = 88;   /* Function not implemented */
    pub const ENMFILE :i32 = 89;   /* No more files */
    pub const ENOTEMPTY :i32 = 90;   /* Directory not empty */
    pub const ENAMETOOLONG :i32 = 91;   /* File or path name too long */
    pub const ELOOP :i32 = 92;   /* Too many symbolic links */
    pub const EOPNOTSUPP :i32 = 95;   /* Operation not supported on socket */
    pub const EPFNOSUPPORT :i32 = 96;   /* Protocol family not supported */
    pub const ECONNRESET :i32 = 104;   /* Connection reset by peer */
    pub const ENOBUFS :i32 = 105;   /* No buffer space available */
    pub const EAFNOSUPPORT :i32 = 106;   /* Address family not supported by protocol family */
    pub const EPROTOTYPE :i32 = 107;   /* Protocol wrong type for socket */
    pub const ENOTSOCK :i32 = 108;   /* Socket operation on non-socket */
    pub const ENOPROTOOPT :i32 = 109;   /* Protocol not available */
    pub const ESHUTDOWN :i32 = 110;   /* Can't send after socket shutdown */
    pub const ECONNREFUSED :i32 = 111;   /* Connection refused */
    pub const EADDRINUSE :i32 = 112;   /* Address already in use */
    pub const ECONNABORTED :i32 = 113;   /* Software caused connection abort */
    pub const ENETUNREACH :i32 = 114;   /* Network is unreachable */
    pub const ENETDOWN :i32 = 115;   /* Network interface is not configured */
    pub const ETIMEDOUT :i32 = 116;   /* Connection timed out */
    pub const EHOSTDOWN :i32 = 117;   /* Host is down */
    pub const EHOSTUNREACH :i32 = 118;   /* Host is unreachable */
    pub const EINPROGRESS :i32 = 119;   /* Connection already in progress */
    pub const EALREADY :i32 = 120;   /* Socket already connected */
    pub const EDESTADDRREQ :i32 = 121;   /* Destination address required */
    pub const EMSGSIZE :i32 = 122;   /* Message too long */
    pub const EPROTONOSUPPORT :i32 = 123;   /* Unknown protocol */
    pub const ESOCKTNOSUPPORT :i32 = 124;   /* Socket type not supported */
    pub const EADDRNOTAVAIL :i32 = 125;   /* Address not available */
    pub const ENETRESET :i32 = 126;   /* Connection aborted by network */
    pub const EISCONN :i32 = 127;   /* Socket is already connected */
    pub const ENOTCONN :i32 = 128;   /* Socket is not connected */
    pub const ETOOMANYREFS :i32 = 129;
    pub const EPROCLIM :i32 = 130;
    pub const EUSERS :i32 = 131;
    pub const EDQUOT :i32 = 132;
    pub const ESTALE :i32 = 133;
    pub const ENOTSUP :i32 = 134;   /* Not supported */
    pub const ENOMEDIUM :i32 = 135;   /* No medium (in tape drive) */
    pub const ENOSHARE :i32 = 136;   /* No such host or network path */
    pub const ECASECLASH :i32 = 137;   /* Filename exists with different case */
    pub const EILSEQ :i32 = 138;   /* Illegal byte sequence */
    pub const EOVERFLOW :i32 = 139;   /* Value too large for defined data type */
    pub const ECANCELED :i32 = 140;   /* Operation canceled */
    pub const ENOTRECOVERABLE :i32 = 141;   /* State not recoverable */
    pub const EOWNERDEAD :i32 = 142;   /* Previous owner died */
    pub const ESTRPIPE :i32 = 143;   /* Streams pipe error */
}

#[macro_export]
macro_rules! err_wrap {
    ($inner:expr) => {{
        match $inner {
            Ok(a) => a,
            Err(e) => {
                return LibnxErrMapper::map(e);
            }
        }
    }};
}