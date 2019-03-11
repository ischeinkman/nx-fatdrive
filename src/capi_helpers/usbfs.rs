
use libnx_rs::LibnxError;
use libnx_rs::usbhs::InterfaceAvailableEvent;
use libnx_rs::usbhs::{Interface, InterfaceFilter, ClientInterfaceSession, InterfaceInfo, UsbHsContext};
use mbr_nostd::{PartitionTableEntry, PartitionTable};
use scsi::scsi::ScsiBlockDevice;
use fatfs::{Dir, DirEntry, File, FileSystem, ReadWriteSeek};
use vecwrapper::VecNewtype;
use buf_scsi::OffsetScsiDevice;
use std::collections::HashMap;
use std::convert::AsRef;
use std::io::{ErrorKind, Read, Write, Seek, SeekFrom};
use std::path::{Component, Components, Path};
use std::sync::{Arc, Mutex, MutexGuard};
use std::slice;
use std::ffi::{CStr, CString};
use std::ptr;

use std::time::Duration;
use usb_comm::UsbClient;
fn wait_for_usb_drive(ctx : &mut UsbHsContext, timeout : u64) -> Result<ClientInterfaceSession, LibnxError> {
    let filter : InterfaceFilter = InterfaceFilter::new()
        .with_interface_class(8)
        .with_interface_subclass(6)
        .with_interface_protocol(80);
    
    let evt = InterfaceAvailableEvent::create(true, 0, filter)?;
    evt.wait(timeout)?;
    let mut interfaces = ctx.query_available_interfaces(filter, 3)?;
    let mut iface = interfaces.pop().ok_or(LibnxError::from_raw(NX_FATDRIVE_ERR_DRIVE_NOT_FOUND))?;
    ctx.acquire_interface(&iface)
}

fn parse_drive(ctx : &mut UsbHsContext, session : ClientInterfaceSession) -> Result<ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype>, LibnxError> {
    let (read_ep, write_ep) = UsbClient::retrieve_iface_endpoints(&session.interface())?;
    let client = UsbClient::new(session, read_ep, write_ep)?;
    scsi::scsi::ScsiBlockDevice::new(client, VecNewtype::new(), VecNewtype::new(), VecNewtype::new()).map_err(|e| LibnxError::from_raw(LibnxErrMapper::map(e)))
}

fn open_partition(mut scsi_wrapper : ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype>, idx : usize) -> Result<OffsetScsiDevice, LibnxError> {

    let mut mbr_buff = VecNewtype::with_fake_capacity(512.max(scsi_wrapper.block_size() as usize));
    let mut mbr_read_count = 0;
    while mbr_buff.inner.len() < 512 {
        let _bt = scsi_wrapper.read(mbr_buff.inner.len() as u32, &mut mbr_buff).map_err(|e| LibnxError::from_raw(LibnxErrMapper::map(e)))?;
        mbr_read_count += 1;
    }

    let mbr_entry = mbr_nostd::MasterBootRecord::from_bytes(&mut mbr_buff.inner).map_err(LibnxErrMapper::map).map_err(LibnxError::from_raw)?;


    let ent : &PartitionTableEntry = &mbr_entry.partition_table_entries()[idx];
    let raw_offset : usize = (ent.logical_block_address * scsi_wrapper.block_size()) as usize; 

    Ok(OffsetScsiDevice::new(scsi_wrapper, raw_offset))
}




pub struct UsbFsServiceContext {
    usb_hs_ctx : UsbHsContext, 
    device_info : InterfaceInfo,
}


// -----------------------------------
#[macro_use]
use capi_helpers::*;
lazy_static! {
    static ref usb_hs_ctx_ptr : Mutex<usize> = Mutex::new(ptr::null_mut::<UsbFsServiceContext>() as usize);
    static ref fs_ptr : Mutex<usize> = Mutex::new(ptr::null_mut::<FileSystem<OffsetScsiDevice>>() as usize);
    static ref id_store_ptr : Mutex<usize> = Mutex::new(ptr::null_mut::<IdStore>() as usize);
}

pub unsafe fn get_usb_hs_ctx<'a>() -> Result<(&'a mut UsbHsContext, MutexGuard<'a, usize>), u32> {
    get_service_ctx().map(|(ctx, guard)| (&mut ctx.usb_hs_ctx, guard))
}

pub unsafe fn get_service_ctx<'a>() -> Result<(&'a mut UsbFsServiceContext, MutexGuard<'a, usize>), u32> {
    let usb_hs_ptr_guard = usb_hs_ctx_ptr.lock().map_err(LibnxErrMapper::map)?;
    let usb_hs_ptr_raw = *usb_hs_ptr_guard;
    let usb_hs = match (usb_hs_ptr_raw as *mut UsbFsServiceContext).as_mut() {
        Some(r) => r, 
        None => {
            return Err(NX_FATDRIVE_ERR_NOT_INITIALIZED);
        }
    };

    return Ok((usb_hs, usb_hs_ptr_guard))

}
pub unsafe fn get_filesystem<'a>() -> Result<(&'a mut FileSystem<OffsetScsiDevice>, MutexGuard<'a, usize>), u32> {
    let fs_ptr_guard = fs_ptr.lock().map_err(LibnxErrMapper::map)?;
    let fs_ptr_raw : usize = *fs_ptr_guard;
    let fs = match (fs_ptr_raw as *mut FileSystem<OffsetScsiDevice>).as_mut() {
        Some(r) => r, 
        None => {
            return Err(NX_FATDRIVE_ERR_NOT_INITIALIZED);
        }
    };

    return Ok((fs, fs_ptr_guard))
}

unsafe fn get_id_store<'a>() -> Result<(&'a mut IdStore, MutexGuard<'a, usize>), u32> {
    let id_store_guard = id_store_ptr.lock().map_err(LibnxErrMapper::map)?;
    let id_ptr_raw : usize = *id_store_guard;
    let id_store = match (id_ptr_raw as *mut IdStore).as_mut() {
        Some(r) => r,
        None => {
            return Err(NX_FATDRIVE_ERR_NOT_INITIALIZED);
        }
    };

    Ok((id_store, id_store_guard))
}


//---------------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn usbFsIsInitialized() -> u32 {
    let mut id_store_guard = err_wrap!(id_store_ptr.lock());
    let mut usb_hs_ptr_guard = err_wrap!(usb_hs_ctx_ptr.lock());
    let mut fs_ptr_guard = err_wrap!(fs_ptr.lock());
    if *id_store_guard != 0 && *usb_hs_ptr_guard != 0 && *fs_ptr_guard != 0 {
        SUCCESS
    }
    else {
        NX_FATDRIVE_ERR_NOT_INITIALIZED
    }
}

#[no_mangle]
pub unsafe extern "C" fn usbFsInitialize() -> u32 {
    if usbFsIsInitialized() == 0 {
        return 0;
    }

    let mut usb_hs_ctx = err_wrap!(UsbHsContext::initialize());
    let mut session = err_wrap!(wait_for_usb_drive(&mut usb_hs_ctx, 1000));
    let mut ctx = UsbFsServiceContext { 
        usb_hs_ctx, 
        device_info : session.interface().info(),
    };
    let mut inner_device = err_wrap!(parse_drive(&mut ctx.usb_hs_ctx, session));

    let ctx_ptr_nval = Box::into_raw(Box::new(ctx));
    let mut usb_hs_ptr_guard = err_wrap!(usb_hs_ctx_ptr.lock());
    *usb_hs_ptr_guard = ctx_ptr_nval as usize;

    let mut partition = err_wrap!(open_partition(inner_device, 0));
    let mut fs = err_wrap!(fatfs::FileSystem::new(partition, fatfs::FsOptions::new()));
    let fs_ptr_nval = Box::into_raw(Box::new(fs));
    let mut fs_ptr_guard = err_wrap!(fs_ptr.lock());
    *fs_ptr_guard = fs_ptr_nval as usize;

    let mut store = IdStore::new();
    let id_store_ptr_nval = Box::into_raw(Box::new(store));
    let mut id_store_guard = err_wrap!(id_store_ptr.lock());
    *id_store_guard = id_store_ptr_nval as usize;
    
    return SUCCESS;
}

use std::ops::Drop;
use std::mem::drop;

#[no_mangle]
pub unsafe extern "C" fn usbFsExit() {
    use std::os::unix::fs::OpenOptionsExt;
    use std::fs::OpenOptions;
    let mut outfile = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(0x0080)
        .open("UsbfsLog.txt"){
            Ok(f) => f,
            Err(_) => {
                return;
            }
        };
    outfile.write_fmt(format_args!("Entered USBFS exit.\n"));
    outfile.flush();

    let mut id_store_guard = match id_store_ptr.lock().map_err(LibnxErrMapper::map) {
        Ok(p) => p, 
        Err(_) => {
            return;
        }
    };
    outfile.write_fmt(format_args!("Got ID store ptr of {}", *id_store_guard));
    outfile.flush();
    if *id_store_guard != 0 {
        let id_store_ptr_inner = (*id_store_guard) as *mut IdStore;
        let mut id_store_box = Box::from_raw(id_store_ptr_inner);
        *id_store_guard = 0;
        drop(id_store_box);
    }
    outfile.write_fmt(format_args!("Unmounted ID store."));
    outfile.flush();

    let mut fs_ptr_guard = match fs_ptr.lock().map_err(LibnxErrMapper::map) {
        Ok(p) => p, 
        Err(_) => {
            return;
        }
    };
    outfile.write_fmt(format_args!("Got FS ptr of {}", *fs_ptr_guard));
    outfile.flush();
    if *fs_ptr_guard != 0 {
        let fs_ptr_inner = (*fs_ptr_guard) as *mut FileSystem<OffsetScsiDevice>;
        let mut fs_box = Box::from_raw(fs_ptr_inner);
        *fs_ptr_guard = 0;
        drop(fs_box);
    }
    outfile.write_fmt(format_args!("Unmounted FS ptr."));
    outfile.flush();

    let mut usb_hs_ptr_guard = match usb_hs_ctx_ptr.lock().map_err(LibnxErrMapper::map) {
        Ok(p) => p, 
        Err(_) => {
            return;
        }
    };
    outfile.write_fmt(format_args!("Got USBHS_CTX ptr of {}", *usb_hs_ptr_guard));
    outfile.flush();
    if *usb_hs_ptr_guard != 0 {
        let usb_hs_ptr_inner = (*usb_hs_ptr_guard) as *mut UsbFsServiceContext;
        let mut usb_hs_box = Box::from_raw(usb_hs_ptr_inner);
        *usb_hs_ptr_guard = 0;
        drop(usb_hs_box);
    }
    outfile.write_fmt(format_args!("Unmounted USBHS_CTX"));
    outfile.flush();
}

#[no_mangle]
pub unsafe extern "C" fn usbFsIsReady() -> u32 {
    let has_initted = usbFsIsInitialized();
    if has_initted != SUCCESS {
        return has_initted;
    }

    let (mut ctx, ctx_guard) = err_wrap!(get_service_ctx());
    let mut device_is_connected = false;
    for iface in err_wrap!(ctx.usb_hs_ctx.query_acquired_interfaces(4)) {
        if iface.info() == ctx.device_info {
            device_is_connected = true;
        }
    }
    if !device_is_connected {
        return NX_FATDRIVE_ERR_DRIVE_DISCONNECTED;
    }

    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsGetMountStatus(status: *mut u64) -> u32 {
    return NX_FATDRIVE_ERR_NOT_IMPLEMENTED;
}

#[no_mangle]
pub unsafe extern "C" fn usbFsOpenFile(fileid: *mut u64, filepath: *const u8, _mode: u64) -> u32 {
    let path : &str = match CStr::from_ptr(filepath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };

    let (id_store, _guard) = err_wrap!(get_id_store());
    let new_id = err_wrap!(id_store.open_file(path));

    *fileid = new_id;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsCloseFile(fileid: u64) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    err_wrap!(id_store.close_file(fileid));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsReadFile(
    fileid: u64,
    buffer: *mut u8,
    size: usize,
    retsize: *mut usize,
) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let mut buf_slice = slice::from_raw_parts_mut(buffer, size);
    let file = err_wrap!(id_store.get_file_handle(fileid));
    let sz = err_wrap!(file.read(&mut buf_slice));
    *retsize = sz;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsWriteFile(
    fileid: u64,
    buffer: *mut u8,
    size: usize,
    retsize: *mut usize,
) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let mut buf_slice = slice::from_raw_parts_mut(buffer, size);
    let file = err_wrap!(id_store.get_file_handle(fileid));
    let sz = err_wrap!(file.write(&mut buf_slice));
    *retsize = sz;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsSeekFile(fileid: u64, pos: u64, whence: u64, retpos: *mut u64) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let file = err_wrap!(id_store.get_file_handle(fileid));

    let sf = match whence {
        0 => SeekFrom::Start(pos),
        1 => {
            let rel : i64 = if pos > i64::max_value() as u64 {
                -1 * ((u64::max_value() - pos) as i64) //assume 2's complement
            } else { pos as i64 };
            SeekFrom::Current(rel)
        }, 
        2 => {
            let rel : i64 = if pos > i64::max_value() as u64 {
                -1 * ((u64::max_value() - pos) as i64) //assume 2's complement
            } else { pos as i64 };
            SeekFrom::End(rel)
        },
        _ => {
            return (NX_FATDRIVE_ERR_NOT_IMPLEMENTED << 8) + NX_FATDRIVE_ERR_MODULE;
        }
    };

    let retval = err_wrap!(file.seek(sf));
    *retpos = retval;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsSyncFile(fileid: u64) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let file = err_wrap!(id_store.get_file_handle(fileid));
    err_wrap!(file.flush());
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsTruncateFile(fileid: u64, _size: u64) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let file = err_wrap!(id_store.get_file_handle(fileid));
    err_wrap!(file.truncate());
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsDeleteFile(filepath: *const u8) -> u32 {
    let path : &str = match CStr::from_ptr(filepath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };
    let (mut id_store, _guard) = err_wrap!(get_id_store());
    if let Some(old_id) = id_store.has_file(&path.to_owned()) {
        err_wrap!(id_store.close_file(old_id));
    }
    let (fs, _guard) = err_wrap!(get_filesystem());
    err_wrap!(fs.root_dir().remove(path));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsStatFile(fileid: u64, size: *mut u64, mode: *mut u64) -> u32 {
    let (id_store, guard) = err_wrap!(get_id_store());
    let path =  err_wrap!(id_store.get_path_for_id(fileid));
    let (nsize, nmode) = err_wrap!(id_store.stat_path(path));
    *size = nsize;
    *mode = nmode;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsStatPath(path_ptr: *const u8, size: *mut u64, mode: *mut u64) -> u32 {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };
    let (id_store, guard) = err_wrap!(get_id_store());
    let (nsize, nmode) = err_wrap!(id_store.stat_path(path));
    *size = nsize;
    *mode = nmode;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsStatFilesystem(totalsize: *mut u64, freesize: *mut u64) -> u32 {
    let (fs, _guard) = err_wrap!(get_filesystem());
    let fsinfo = err_wrap!(fs.stats());
    let tsize = fsinfo.cluster_size() as u64 * fsinfo.total_clusters() as u64;
    let fsize = fsinfo.cluster_size() as u64 * fsinfo.free_clusters() as u64;
    *totalsize = tsize;
    *freesize = fsize;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsOpenDir(dirid: *mut u64, dirpath: *const u8) -> u32 {
    let path : &str = match CStr::from_ptr(dirpath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };

    let (id_store, _guard) = err_wrap!(get_id_store());
    let new_id = err_wrap!(id_store.open_dir(path));

    *dirid = new_id;
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsReadDir(
    dirid: u64,
    type_ptr: *mut u64,
    size: *mut u64,
    name: *mut u8,
    namemax: usize,
) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    let current = err_wrap!(id_store.read_next_dirent(dirid));
    match current {
        Some(ent) => {
            let bytes = ent.name.as_bytes();
            std::ptr::copy(bytes.as_ptr(), name, namemax.min(bytes.len()));
            *type_ptr = ent.type_val;
            *size = ent.size;
        },
        None => {
            *type_ptr = 0xF;
            *size = 0;
            *name = 0;
        }
    }
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsCloseDir(dirid: u64) -> u32 {
    let (id_store, _guard) = err_wrap!(get_id_store());
    err_wrap!(id_store.close_dir(dirid));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsCreateDir(dirpath: *const u8) -> u32 {
    let path : &str = match CStr::from_ptr(dirpath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };
    let (mut id_store, _guard) = err_wrap!(get_id_store());
    if let Some(old_id) = id_store.has_dir(&path.to_owned()) {
        return SUCCESS;
    }
    let (mut fs, _guard) = err_wrap!(get_filesystem());
    err_wrap!(fs.root_dir().create_dir(path));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsDeleteDir(dirpath: *const u8) -> u32 {
    let path : &str = match CStr::from_ptr(dirpath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };
    let (mut id_store, _guard) = err_wrap!(get_id_store());
    if let Some(old_id) = id_store.has_dir(&path.to_owned()) {
        err_wrap!(id_store.close_dir(old_id));
    }
    let (fs, _guard) = err_wrap!(get_filesystem());
    err_wrap!(fs.root_dir().remove(path));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsCreateFile(filepath: *const u8) -> u32 {
    let path : &str = match CStr::from_ptr(filepath as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            return NX_FATDRIVE_ERR_UNKNOWN;
        }
    };
    let (mut id_store, _guard) = err_wrap!(get_id_store());
    if let Some(old_id) = id_store.has_file(&path.to_owned()) {
        return SUCCESS;
    }
    let (mut fs, _guard) = err_wrap!(get_filesystem());
    err_wrap!(fs.root_dir().create_file(path));
    SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn usbFsReadRaw(sector: u64, sectorcount: u64, buffer: *const u8) -> u32 {
    return NX_FATDRIVE_ERR_NOT_IMPLEMENTED;
}

// Register "usbhdd:" device
#[no_mangle]
pub unsafe extern "C" fn usbFsDeviceRegister() {

}

// Keep calling update periodically to check USB drive ready
// Returns 1 if status changed
#[no_mangle]
pub unsafe extern "C" fn usbFsDeviceUpdate() -> u32 {
    usbFsIsReady()
}

// If status changed, check mount status
// Returns 0 == USBFS_UNMOUNTED, 1 == USBFS_MOUNTED, 2 == USBFS_UNSUPPORTED_FS
#[no_mangle]
pub unsafe extern "C" fn usbFsDeviceGetMountStatus() -> u32 {
    if usbFsIsReady() != 0 {
        0
    }
    else {
        1
    }
}
