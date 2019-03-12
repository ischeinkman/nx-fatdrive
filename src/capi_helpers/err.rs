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