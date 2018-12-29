#![allow(dead_code)]
#![crate_type = "staticlib"]

extern crate fatfs;
extern crate libc;
extern crate libnx_rs;
use libnx_rs::LibnxError;
use libnx_rs::usbhs::InterfaceAvailableEvent;
extern crate mbr_nostd;
use mbr_nostd::{PartitionTableEntry, PartitionTable};
extern crate scsi;
use scsi::scsi::ScsiBlockDevice;

use fatfs::{Dir, DirEntry, File, FileSystem, ReadWriteSeek};

#[macro_use]
extern crate lazy_static;
pub mod buf_scsi;
pub mod usb_comm;
pub mod vecwrapper;
use vecwrapper::VecNewtype;

mod capi_helpers;
pub use capi_helpers::*;
use buf_scsi::OffsetScsiDevice;
use usb_comm::UsbClient;
mod aligned_slice;

use libnx_rs::usbhs::{Interface, InterfaceFilter, ClientInterfaceSession, InterfaceInfo, UsbHsContext};

use std::collections::HashMap;
use std::convert::AsRef;
use std::io::{ErrorKind, Read, Write, Seek, SeekFrom};
use std::path::{Component, Components, Path};
use std::sync::{Arc, Mutex, MutexGuard};
use std::slice;

use std::ffi::{CStr, CString};
use std::ptr;

use std::time::Duration;


