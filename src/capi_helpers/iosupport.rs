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
use std::mem;
use std::os::raw::c_void;
use capi_helpers::*;
use self::iosupport_bindings::*;
use std::cell::RefCell;
use std::time::Duration;
use usb_comm::UsbClient;

struct NewlibContext {
    usb_hs_ctx : Option<UsbHsContext>,
    client_state : ClientState, 
    current_working_directory : Option<String>,
}

enum ClientState {
    Uninitialized, 
    Acquired {
        iface : Interface,
        client : ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype>,
    },
    Opened {
        iface : Interface,
        fs : FileSystem<OffsetScsiDevice>,
        partition_in_use : PartitionTableEntry,
    }
}

struct FileStruct {
    fatfl : fatfs::DirEntry<'static, OffsetScsiDevice>,
    offset : u64, 
}

struct DirStruct {
    dir : fatfs::DirEntry<'static, OffsetScsiDevice>,
    index : usize, 
}

impl NewlibContext {

    pub unsafe fn get_global() -> Result<&'static mut NewlibContext, u32> {
        let device_ptr = GetDeviceOpTab(device_name as *const _ as *const u8);
        let ctx_ptr = match device_ptr.as_ref() {
            Some(d) => d.deviceData, 
            None => {
                return Err(NX_FATDRIVE_ERR_NOT_INITIALIZED);
            }
        };
        let g_context : &'static mut NewlibContext = match (ctx_ptr as *mut NewlibContext).as_mut() {
            Some(c) => c, 
            None => {
                return Err(NX_FATDRIVE_ERR_UNKNOWN);
            }
        };
        Ok(g_context)
    }

    const fn new() -> NewlibContext {
        NewlibContext {
            usb_hs_ctx : None, 
            client_state : ClientState::Uninitialized, 
            current_working_directory : None, 
        }
    }

    pub fn init_usb_hs_ctx(&mut self) -> Result<(), u32> {
        if self.usb_hs_ctx.is_some() {
            return Ok(())
        }
        let newctx = UsbHsContext::initialize().map_err(LibnxErrMapper::map)?;
        self.usb_hs_ctx = Some(newctx);
        Ok(())
    }

    pub fn wait_for_usb_drive(&mut self, timeout : u64) -> Result<(), u32> {
        match self.client_state {
            ref Uninitialized => {

            },
            _ => {
                return Ok(());
            },
        };
        let mut ctx = self.usb_hs_ctx.as_mut().ok_or(NX_FATDRIVE_ERR_NOT_INITIALIZED)?;
        let filter : InterfaceFilter = InterfaceFilter::new()
            .with_interface_class(8)
            .with_interface_subclass(6)
            .with_interface_protocol(80);
        
        let evt = InterfaceAvailableEvent::create(true, 0, filter).map_err(LibnxErrMapper::map)?;
        evt.wait(timeout).map_err(LibnxErrMapper::map)?;
        let mut interfaces = ctx.query_available_interfaces(filter, 3).map_err(LibnxErrMapper::map)?;
        let iface = interfaces.pop().ok_or(NX_FATDRIVE_ERR_DRIVE_NOT_FOUND)?;
        let mut session = ctx.acquire_interface(&iface).map_err(LibnxErrMapper::map)?;

        let (read_ep, write_ep) = UsbClient::retrieve_iface_endpoints(&session.interface()).map_err(LibnxErrMapper::map)?;
        let client = UsbClient::new(session, read_ep, write_ep).map_err(LibnxErrMapper::map)?;
        let mut scsi_wrapper = ScsiBlockDevice::new(client, VecNewtype::new(), VecNewtype::new(), VecNewtype::new()).map_err(LibnxErrMapper::map)?;
        
        self.client_state = ClientState::Acquired {
            iface,
            client: scsi_wrapper,
        };
        Ok(())
    }

    pub fn get_partitions(&mut self) -> Result<Vec<PartitionTableEntry>, u32>{
        let mut scsi_wrapper : &mut ScsiBlockDevice<UsbClient, VecNewtype, VecNewtype, VecNewtype> = match self.client_state {
            ClientState::Acquired {ref mut client, ..} => client, 
            ClientState::Opened {..} => {
                return Err(NX_FATDRIVE_ERR_UNKNOWN)
            },
            ClientState::Uninitialized => {
                return Err(NX_FATDRIVE_ERR_NOT_INITIALIZED)
            }
        };
        let mut mbr_buff = VecNewtype::with_fake_capacity(512.max(scsi_wrapper.block_size() as usize));
        let mut mbr_read_count = 0;
        while mbr_buff.inner.len() < 512 {
            let _bt = scsi_wrapper.read(mbr_buff.inner.len() as u32, &mut mbr_buff).map_err(LibnxErrMapper::map)?;
            mbr_read_count += 1;
        }

        let mbr_entry = mbr_nostd::MasterBootRecord::from_bytes(&mut mbr_buff.inner).map_err(LibnxErrMapper::map)?;
        Ok(mbr_entry.partition_table_entries().iter().map(|e| e.clone()).collect())
    }

    pub fn open_partition(&mut self, idx : usize) -> Result<(), u32> {
        let partition_list = self.get_partitions()?;
        let (mut scsi_wrapper, iface) = match std::mem::replace(&mut self.client_state, ClientState::Uninitialized) {
            ClientState::Acquired {client, iface} => (client, iface),
            _ => {
                return Err(NX_FATDRIVE_ERR_UNKNOWN);
            }
        };
        let ent = partition_list.get(idx).ok_or(NX_FATDRIVE_ERR_UNKNOWN)?.clone();
        let raw_offset : usize = (ent.logical_block_address * scsi_wrapper.block_size()) as usize; 

        let mut device = OffsetScsiDevice::new(scsi_wrapper, raw_offset);
        let mut fs = fatfs::FileSystem::new(device, fatfs::FsOptions::new()).map_err(LibnxErrMapper::map)?;

        self.client_state = ClientState::Opened {
            iface,
            partition_in_use : ent, 
            fs,
        };

        Ok(())
    }
}

use std::default::Default;
pub fn stat_dirent<'a, T : Read + Write + Seek>(ent : &DirEntry<'a, T>) -> stat {
    let mut retval = stat::default();
    retval.st_nlink = 1; //Do not support symlinks 


    const BLOCK_SIZE : u64 = 512; //TODO: Get from device
    retval.st_blksize = BLOCK_SIZE;
    retval.st_size = ent.len();
    retval.st_blocks = 1 + ent.len()/BLOCK_SIZE;

    // Only valid modes are RW or R for everyone; set the X bit as well since some 
    // might interpret opening a directory as "executing" it.
    let read_bits = stat::OWNER_READ | stat::GROUP_READ | stat::OTHER_READ;
    let write_bits = if ent.attributes().contains(fatfs::FileAttributes::READ_ONLY) { 0 } else { stat::OWNER_WRITE | stat::GROUP_WRITE | stat::OTHER_WRITE };
    let exec_bits = stat::OWNER_EXEC | stat::GROUP_EXEC | stat::OTHER_EXEC;
    let type_bits = if ent.is_dir() { stat::DIRECTORY } else { stat::FILE };
    retval.st_mode = read_bits | write_bits | exec_bits | type_bits;

    let atime = fatfs::DateTime {
        date : ent.accessed() ,
        time : fatfs::Time { hour : 12, min : 0,  sec : 0, millis : 0},
    };
    retval.st_atime = fat_time_to_unix(atime);
    retval.st_ctime = fat_time_to_unix(ent.created());
    retval.st_mtime = fat_time_to_unix(ent.modified());

    retval
} 

fn fat_time_to_unix(fat_time : fatfs::DateTime) -> u64 {
    unimplemented!()
}

fn path_to_dirent<'a, T : Read + Write + Seek>(fs : &'a FileSystem<T>, path : &str) -> DirEntry<'a, T> {
    unimplemented!()
}

#[no_mangle]
pub unsafe extern "C" fn _fatdrive_diropen_r(r : *mut _reent, dir_state_ptr : *mut DIR_ITER, path_ptr : *const u8) -> *mut DIR_ITER {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return ptr::null_mut();
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return ptr::null_mut();
        }
    };

    let fs : &'static mut FileSystem<OffsetScsiDevice> = match &mut ctx.client_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return ptr::null_mut();
        }
    };

    let ent = path_to_dirent(fs, path);
    if !ent.is_dir() {
        (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT;
        return ptr::null_mut();
    }
    let nstruct = DirStruct {
        index : 0, 
        dir : ent,
    };
    
    let state : &mut DIR_ITER = match dir_state_ptr.as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT;
            return ptr::null_mut();
        }
    };

    if state.dirStruct.is_null(){
        (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT;
        return ptr::null_mut();
    };

    let dir_struct_ptr = state.dirStruct as *mut DirStruct;
    *dir_struct_ptr = nstruct;
    return dir_state_ptr;
}

unsafe extern "C" fn _fatdrive_open_r(r: *mut _reent, fd: *mut c_void, path_ptr: * const u8, flags: u32, mode: u32) -> i32 {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return (*r).errno;
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return e as i32;
        }
    };

    let fs : &mut FileSystem<OffsetScsiDevice> = match &mut ctx.client_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
        }
    };

    let ent = path_to_dirent(fs, path);
    if !ent.is_file() {
        (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT;
        return -1;
    }
    let nstruct = FileStruct {
        offset : 0, 
        fatfl : ent,
    };

    if fd.is_null() {
        (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT;
        return -1;
    }

    let fl_struct_ptr = fd as *const FileStruct as *mut FileStruct;
    (*fl_struct_ptr) = nstruct;
    return 0;
}

unsafe extern "C" fn _fatdrive_write_r ( r: *mut _reent, fd: *mut c_void, buff_ptr: * const u8, len: usize) -> isize {
    let fl_ctx : &mut FileStruct = match (fd as *mut FileStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_FILE_NOT_FOUND as i32;
            return -1;
        }
    };
    let base = fl_ctx.offset;
    let mut fl = fl_ctx.fatfl.to_file();
    if let Err(e) = fl.seek(SeekFrom::Start(base)).map_err(LibnxErrMapper::map) {
        (*r).errno = e as i32;
        return -1;
    }

    let buff = slice::from_raw_parts(buff_ptr, len);
    let writecount = match fl.write(buff).map_err(LibnxErrMapper::map) {
        Ok(ln) => ln as u64, 
        Err(e) => {
            (*r).errno = e as i32;
            return -1;
        }
    };
    if let Err(e) = fl.flush().map_err(LibnxErrMapper::map) {
            (*r).errno = e as i32;
            return -1;
    };
    fl_ctx.offset += writecount;
    writecount as isize
}

unsafe extern "C" fn _fatdrive_read_r ( r: *mut _reent, fd: *mut c_void, buff_ptr: * mut u8, len: usize) -> isize {
    let fl_ctx : &mut FileStruct = match (fd as *mut FileStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_FILE_NOT_FOUND as i32;
            return -1;
        }
    };
    let base = fl_ctx.offset;
    let mut fl = fl_ctx.fatfl.to_file();
    if let Err(e) = fl.seek(SeekFrom::Start(base)).map_err(LibnxErrMapper::map) {
        (*r).errno = e as i32;
        return -1;
    }

    let buff = slice::from_raw_parts_mut(buff_ptr, len);
    let readcount = match fl.read(buff).map_err(LibnxErrMapper::map) {
        Ok(ln) => ln as u64, 
        Err(e) => {
            (*r).errno = e as i32;
            return -1;
        }
    };

    fl_ctx.offset += readcount;
    readcount as isize
}

unsafe extern "C" fn _fatdrive_seek_r(r: *mut _reent, fd: *mut c_void, pos: off_t, dir: i32) -> off_t {
    let fl_ctx : &mut FileStruct = match (fd as *mut FileStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_FILE_NOT_FOUND as i32;
            return -1;
        }
    };
    let base = fl_ctx.offset;
    let mut fl = fl_ctx.fatfl.to_file();
    if let Err(e) = fl.seek(SeekFrom::Start(base)).map_err(LibnxErrMapper::map) {
        (*r).errno = e as i32;
        return -1;
    }
    
    let sk = match dir {
        1 => SeekFrom::Start(pos as u64),
        2 => SeekFrom::Current(pos),
        3 => SeekFrom::End(pos),

        _ => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
    };

    let newoff = match fl.seek(sk).map_err(LibnxErrMapper::map) {
        Ok(ln) => ln, 
        Err(e) => {
            (*r).errno = e as i32;
            return -1;
        }
    };
    fl_ctx.offset = newoff;
    newoff as off_t
}

unsafe extern "C" fn _fatdrive_rename_r( r: *mut _reent, old_path_ptr: * const u8, new_path_ptr: * const u8) -> i32 {
    let old_path : &str = match CStr::from_ptr(old_path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return (*r).errno;
        }
    };

    let new_path : &str = match CStr::from_ptr(new_path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return (*r).errno;
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return e as i32;
        }
    };
    
    let mut fs = match &mut ctx.client_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
        }
    };

    let root = fs.root_dir();
    match root.rename(old_path, &root, new_path).map_err(LibnxErrMapper::map) {
        Ok(_) => 0,
        Err(e) => {
            (*r).errno = e as i32;
            e as i32
        },
    }
}
unsafe extern "C" fn _fatdrive_chmod_r(r: *mut _reent, path: * const u8, mode: mode_t) -> i32 {
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return errno::NX_FATDRIVE_ERRNO_ENOSYS;
}
unsafe extern "C" fn _fatdrive_fchmod_r(r: *mut _reent, fd: *mut c_void, mode: mode_t) -> i32 {
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return errno::NX_FATDRIVE_ERRNO_ENOSYS;
}
unsafe extern "C" fn _fatdrive_link_r(r: *mut _reent, existing: * const u8, newLink: * const u8) -> i32{
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return errno::NX_FATDRIVE_ERRNO_ENOSYS;
}

unsafe extern "C" fn _fatdrive_unlink_r(r: *mut _reent, name: * const u8) -> i32 {
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return errno::NX_FATDRIVE_ERRNO_ENOSYS;
}
unsafe extern "C" fn _fatdrive_rmdir_r(r: *mut _reent, path_ptr: * const u8) -> i32 {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return (*r).errno;
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return e as i32;
        }
    };

    let mut fs = match &mut ctx.client_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
        }
    };

    match fs.root_dir().remove(path).map_err(LibnxErrMapper::map) {
        Ok(_) => {

        },
        Err(e) => {
            (*r).errno = e as i32;
            return e as i32;
        }
    };
    return 0;
}

unsafe extern "C" fn _fatdrive_fstat_r(r : *mut _reent, fd : *mut c_void, st : *mut stat) -> i32 {
    let fl_ctx : &mut FileStruct = match (fd as *mut FileStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_FILE_NOT_FOUND as i32;
            return -1;
        }
    };
    (*st) = stat_dirent(&fl_ctx.fatfl);
    return 0;
}


unsafe extern "C" fn _fatdrive_dirreset_r(r: *mut _reent, dirState: *mut DIR_ITER) -> i32 {
    let dir_itr : &mut DIR_ITER = match dirState.as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
    };

    let dir_struct : &mut DirStruct = match (dir_itr.dirStruct as *mut DirStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
    };

    dir_struct.index = 0;
    return 0;
}

unsafe extern "C" fn _fatdrive_dirclose_r(r: *mut _reent, dirState: *mut DIR_ITER) -> i32 {
    // Currently there is no need to explicitly "close" a directory handle since all changes are
    // non-buffered; heck, the DirEntry struct doesn't even implement Drop.

    0
}

unsafe extern "C" fn _fatdrive_dirnext_r( r: *mut _reent, dirState: *mut DIR_ITER, filename_ptr: *mut u8, filestat: *mut stat) -> i32 {
    let dir_itr : &mut DIR_ITER = match dirState.as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
    };

    let dir_struct_ptr = dir_itr.dirStruct as *mut DirStruct;
    let ended = is_zeroed(dir_struct_ptr as *const DirStruct);
    if ended != 0 {
        (*r).errno = ended;
        return -1;
    }

    let mut dir_struct : &mut DirStruct = match dir_struct_ptr.as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOENT as i32;
            return -1;
        }
    };  

    let root_dir = dir_struct.dir.to_dir();

    let next_itm = match root_dir.iter().skip(dir_struct.index).next() {
        Some(Ok(p)) => {
            if let Some(stat_ref) = filestat.as_mut() {
                stat_ref.clone_from(&stat_dirent(&p));
            }
            if !filename_ptr.is_null() {
                let full_name = p.file_name();
                let name_bytes = full_name.as_bytes();
                let retlen = name_bytes.len().min(NX_FATDRIVE_NAME_MAX);
                let mut filename_slice = slice::from_raw_parts_mut(filename_ptr, retlen);
                filename_slice.copy_from_slice(&name_bytes[0 .. retlen]);
            }
            p
        },
        Some(Err(e)) => {
            (*r).errno = NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
        None => {
            mem::zeroed()
        }
    };
    dir_struct.index += 1;
    return 0;
}

fn is_zeroed<T>(struct_ptr : *const T) -> i32 {
    let struct_size = mem::size_of::<T>();
    if struct_ptr.is_null() {
        return errno::NX_FATDRIVE_ERRNO_ENOENT;
    }

    let struct_data : &[u8] = unsafe { slice::from_raw_parts(struct_ptr as *const u8, struct_size) };

    let mut zeroed_buffer : Vec<u8> = Vec::with_capacity(struct_size);
    zeroed_buffer.resize(struct_size, 0);

    let zeroed_ref : &[u8] = zeroed_buffer.as_ref();

    if zeroed_ref == struct_data {
        errno::NX_FATDRIVE_ERRNO_ENOENT
    }
    else {
        0
    }
}


unsafe extern "C" fn _fatdrive_mkdir_r(r: *mut _reent, path_ptr: * const u8, mode: u32) -> i32 {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return (*r).errno;
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return e as i32;
        }
    };

    let mut ctx_state = &mut ctx.client_state;
    let fs : &mut FileSystem<OffsetScsiDevice> = match ctx_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
        }
    };

    let retval = match fs.root_dir().create_dir(path) {
        Ok(_) => 0, 
        Err(e) => {
            (*r).errno = e.raw_os_error().unwrap_or(0xFFFFFFFF); 
            -1 
        }
    };

    return retval;
}

unsafe extern "C" fn _fatdrive_ftruncate_r(r: *mut _reent, fd: *mut ::std::os::raw::c_void, len: off_t) -> i32 {
    let fl_ctx : &mut FileStruct = match (fd as *mut FileStruct).as_mut() {
        Some(p) => p, 
        None => {
            (*r).errno = NX_FATDRIVE_ERR_FILE_NOT_FOUND as i32;
            return -1;
        }
    };

    match fl_ctx.fatfl.to_file().truncate() {
        Ok(_) => 0, 
        Err(e) => { 
            (*r).errno = e.raw_os_error().unwrap_or(0xFFFFFFFF);
            -1
        }
    }
}

unsafe extern "C" fn _fatdrive_fsync_r(r: *mut _reent, fd: *mut ::std::os::raw::c_void) -> i32 {
    //Right now we immediately flush to the output as we write, so we don't need this to do anything.
    0
}
unsafe extern "C" fn _fatdrive_close_r( r: *mut _reent, fd : *mut c_void) -> i32 {
    //Since we flush on write, we don't need any special closing code. 
    0
}
unsafe extern "C" fn _fatdrive_stat_r(r: *mut _reent, path_ptr: * const u8, st: *mut stat ) -> i32 {
    let path : &str = match CStr::from_ptr(path_ptr as *const std::os::raw::c_char).to_str() {
        Ok(s) => s,
        Err(_e) => {
            (*r).errno =  NX_FATDRIVE_ERR_UNKNOWN as i32;
            return -1;
        }
    };

    let mut ctx = match NewlibContext::get_global() {
        Ok(c) => c,
        Err(e) => {
            (*r).errno = e as i32;
            return -1;
        }
    };

    let mut fs = match &mut ctx.client_state {
        ClientState::Opened {ref mut fs, ..} => fs, 
        _ => {
            (*r).errno = NX_FATDRIVE_ERR_NOT_INITIALIZED as i32;
            return -1;
        }
    };
    let ent = path_to_dirent(fs, path);
    (*st) = stat_dirent(&ent);
    return 0;
}

unsafe extern "C" fn _fatdrive_lstat_r(r: *mut _reent, path_str: * const u8, st: *mut stat ) -> i32 {
    _fatdrive_stat_r(r, path_str, st)
}

unsafe extern "C" fn _fatdrive_chdir_r(r: *mut _reent, name: * const u8) -> i32 {
    //TODO: This
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return -1;
}
unsafe extern "C" fn _fatdrive_utimes_r(r: *mut _reent, filename: * const u8, times: *const timeval) -> i32 {
    //TODO: This
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return -1;
}
unsafe extern "C" fn _fatdrive_stat_vfs_r( r: *mut _reent, path: * const u8, buf: *mut statvfs) -> i32 {
    //TODO: This
    (*r).errno = errno::NX_FATDRIVE_ERRNO_ENOSYS;
    return -1;
}

const device_name : &[u8] = b"usbfs";

const dir_state_size : usize = mem::size_of::<DirStruct>();
const file_struct_size : usize = mem::size_of::<FileStruct>();

#[no_mangle]
unsafe extern "C" fn nxFatdriveMount() -> u32 {
    let mut ctx = Box::new(NewlibContext::new());
    err_wrap!(ctx.init_usb_hs_ctx());
    err_wrap!(ctx.wait_for_usb_drive(0x800000));
    err_wrap!(ctx.open_partition(0));
    let mut device = Box::new(nxfatdrive_devoptab());
    device.deviceData = Box::into_raw(ctx) as *mut c_void;
    let add_err = AddDevice(Box::into_raw(device));
    add_err as u32
}


#[no_mangle]
unsafe extern "C" fn nxFatdriveUnmount() -> u32 {
    let device_ptr = GetDeviceOpTab(device_name as *const _ as *const u8);
    let device = Box::from_raw(device_ptr);
    let ctx = Box::from_raw(device.deviceData as *mut NewlibContext);
    let rem_err = RemoveDevice(device_name as *const _ as *const u8);
    if rem_err != 0 {
        return rem_err as u32;
    }
    0
}

pub fn nxfatdrive_devoptab() -> devoptab_t {
    devoptab_t {
        name: device_name as *const _ as *const u8,
        structSize: file_struct_size,
        open_r: Some(_fatdrive_open_r),
        close_r: Some(_fatdrive_close_r),
        write_r: Some(_fatdrive_write_r),
        read_r: Some(_fatdrive_read_r),
        seek_r: Some(_fatdrive_seek_r),
        fstat_r: Some(_fatdrive_fstat_r),
        stat_r: Some(_fatdrive_stat_r),
        link_r: Some(_fatdrive_link_r),
        unlink_r: Some(_fatdrive_unlink_r),
        chdir_r: Some(_fatdrive_chdir_r),
        rename_r: Some(_fatdrive_rename_r),
        mkdir_r: Some(_fatdrive_mkdir_r),
        dirStateSize: dir_state_size,
        diropen_r: Some(_fatdrive_diropen_r),
        dirreset_r: Some(_fatdrive_dirreset_r),
        dirnext_r: Some(_fatdrive_dirnext_r),
        dirclose_r: Some(_fatdrive_dirclose_r),
        statvfs_r: Some(_fatdrive_stat_vfs_r),
        ftruncate_r: Some(_fatdrive_ftruncate_r),
        fsync_r: Some(_fatdrive_fsync_r),
        deviceData: ptr::null_mut(),
        chmod_r: Some(_fatdrive_chmod_r),
        fchmod_r: Some(_fatdrive_fchmod_r),
        rmdir_r: Some(_fatdrive_rmdir_r),
        lstat_r: Some(_fatdrive_lstat_r),
        utimes_r: Some(_fatdrive_utimes_r),
    }
}
mod errno {
    pub const NX_FATDRIVE_ERRNO_EPERM :i32 = 1;   /* Not owner */
    pub const NX_FATDRIVE_ERRNO_ENOENT :i32 = 2;   /* No such file or directory */
    pub const NX_FATDRIVE_ERRNO_ESRCH :i32 = 3;   /* No such process */
    pub const NX_FATDRIVE_ERRNO_EINTR :i32 = 4;   /* Interrupted system call */
    pub const NX_FATDRIVE_ERRNO_EIO :i32 = 5;   /* I/O error */
    pub const NX_FATDRIVE_ERRNO_ENXIO :i32 = 6;   /* No such device or address */
    pub const NX_FATDRIVE_ERRNO_E2BIG :i32 = 7;   /* Arg list too long */
    pub const NX_FATDRIVE_ERRNO_ENOEXEC :i32 = 8;   /* Exec format error */
    pub const NX_FATDRIVE_ERRNO_EBADF :i32 = 9;   /* Bad file number */
    pub const NX_FATDRIVE_ERRNO_ECHILD :i32 = 10;   /* No children */
    pub const NX_FATDRIVE_ERRNO_EAGAIN :i32 = 11;   /* No more processes */
    pub const NX_FATDRIVE_ERRNO_ENOMEM :i32 = 12;   /* Not enough space */
    pub const NX_FATDRIVE_ERRNO_EACCES :i32 = 13;   /* Permission denied */
    pub const NX_FATDRIVE_ERRNO_EFAULT :i32 = 14;   /* Bad address */
    pub const NX_FATDRIVE_ERRNO_ENOTBLK :i32 = 15;   /* Block device required */
    pub const NX_FATDRIVE_ERRNO_EBUSY :i32 = 16;   /* Device or resource busy */
    pub const NX_FATDRIVE_ERRNO_EEXIST :i32 = 17;   /* File exists */
    pub const NX_FATDRIVE_ERRNO_EXDEV :i32 = 18;   /* Cross-device link */
    pub const NX_FATDRIVE_ERRNO_ENODEV :i32 = 19;   /* No such device */
    pub const NX_FATDRIVE_ERRNO_ENOTDIR :i32 = 20;   /* Not a directory */
    pub const NX_FATDRIVE_ERRNO_EISDIR :i32 = 21;   /* Is a directory */
    pub const NX_FATDRIVE_ERRNO_EINVAL :i32 = 22;   /* Invalid argument */
    pub const NX_FATDRIVE_ERRNO_ENFILE :i32 = 23;   /* Too many open files in system */
    pub const NX_FATDRIVE_ERRNO_EMFILE :i32 = 24;   /* File descriptor value too large */
    pub const NX_FATDRIVE_ERRNO_ENOTTY :i32 = 25;   /* Not a character device */
    pub const NX_FATDRIVE_ERRNO_ETXTBSY :i32 = 26;   /* Text file busy */
    pub const NX_FATDRIVE_ERRNO_EFBIG :i32 = 27;   /* File too large */
    pub const NX_FATDRIVE_ERRNO_ENOSPC :i32 = 28;   /* No space left on device */
    pub const NX_FATDRIVE_ERRNO_ESPIPE :i32 = 29;   /* Illegal seek */
    pub const NX_FATDRIVE_ERRNO_EROFS :i32 = 30;   /* Read-only file system */
    pub const NX_FATDRIVE_ERRNO_EMLINK :i32 = 31;   /* Too many links */
    pub const NX_FATDRIVE_ERRNO_EPIPE :i32 = 32;   /* Broken pipe */
    pub const NX_FATDRIVE_ERRNO_EDOM :i32 = 33;   /* Mathematics argument out of domain of function */
    pub const NX_FATDRIVE_ERRNO_ERANGE :i32 = 34;   /* Result too large */
    pub const NX_FATDRIVE_ERRNO_ENOMSG :i32 = 35;   /* No message of desired type */
    pub const NX_FATDRIVE_ERRNO_EIDRM :i32 = 36;   /* Identifier removed */
    pub const NX_FATDRIVE_ERRNO_ECHRNG :i32 = 37;   /* Channel number out of range */
    pub const NX_FATDRIVE_ERRNO_EL2NSYNC :i32 = 38;   /* Level 2 not synchronized */
    pub const NX_FATDRIVE_ERRNO_EL3HLT :i32 = 39;   /* Level 3 halted */
    pub const NX_FATDRIVE_ERRNO_EL3RST :i32 = 40;   /* Level 3 reset */
    pub const NX_FATDRIVE_ERRNO_ELNRNG :i32 = 41;   /* Link number out of range */
    pub const NX_FATDRIVE_ERRNO_EUNATCH :i32 = 42;   /* Protocol driver not attached */
    pub const NX_FATDRIVE_ERRNO_ENOCSI :i32 = 43;   /* No CSI structure available */
    pub const NX_FATDRIVE_ERRNO_EL2HLT :i32 = 44;   /* Level 2 halted */
    pub const NX_FATDRIVE_ERRNO_EDEADLK :i32 = 45;   /* Deadlock */
    pub const NX_FATDRIVE_ERRNO_ENOLCK :i32 = 46;   /* No lock */
    pub const NX_FATDRIVE_ERRNO_EBADE :i32 = 50;   /* Invalid exchange */
    pub const NX_FATDRIVE_ERRNO_EBADR :i32 = 51;   /* Invalid request descriptor */
    pub const NX_FATDRIVE_ERRNO_EXFULL :i32 = 52;   /* Exchange full */
    pub const NX_FATDRIVE_ERRNO_ENOANO :i32 = 53;   /* No anode */
    pub const NX_FATDRIVE_ERRNO_EBADRQC :i32 = 54;   /* Invalid request code */
    pub const NX_FATDRIVE_ERRNO_EBADSLT :i32 = 55;   /* Invalid slot */
    pub const NX_FATDRIVE_ERRNO_EDEADLOCK :i32 = 56;   /* File locking deadlock error */
    pub const NX_FATDRIVE_ERRNO_EBFONT :i32 = 57;   /* Bad font file fmt */
    pub const NX_FATDRIVE_ERRNO_ENOSTR :i32 = 60;   /* Not a stream */
    pub const NX_FATDRIVE_ERRNO_ENODATA :i32 = 61;   /* No data (for no delay io) */
    pub const NX_FATDRIVE_ERRNO_ETIME :i32 = 62;   /* Stream ioctl timeout */
    pub const NX_FATDRIVE_ERRNO_ENOSR :i32 = 63;   /* No stream resources */
    pub const NX_FATDRIVE_ERRNO_ENONET :i32 = 64;   /* Machine is not on the network */
    pub const NX_FATDRIVE_ERRNO_ENOPKG :i32 = 65;   /* Package not installed */
    pub const NX_FATDRIVE_ERRNO_EREMOTE :i32 = 66;   /* The object is remote */
    pub const NX_FATDRIVE_ERRNO_ENOLINK :i32 = 67;   /* Virtual circuit is gone */
    pub const NX_FATDRIVE_ERRNO_EADV :i32 = 68;   /* Advertise error */
    pub const NX_FATDRIVE_ERRNO_ESRMNT :i32 = 69;   /* Srmount error */
    pub const NX_FATDRIVE_ERRNO_ECOMM :i32 = 70;   /* Communication error on send */
    pub const NX_FATDRIVE_ERRNO_EPROTO :i32 = 71;   /* Protocol error */
    pub const NX_FATDRIVE_ERRNO_EMULTIHOP :i32 = 74;   /* Multihop attempted */
    pub const NX_FATDRIVE_ERRNO_ELBIN :i32 = 75;   /* Inode is remote (not really error) */
    pub const NX_FATDRIVE_ERRNO_EDOTDOT :i32 = 76;   /* Cross mount point (not really error) */
    pub const NX_FATDRIVE_ERRNO_EBADMSG :i32 = 77;   /* Bad message */
    pub const NX_FATDRIVE_ERRNO_EFTYPE :i32 = 79;   /* Inappropriate file type or format */
    pub const NX_FATDRIVE_ERRNO_ENOTUNIQ :i32 = 80;   /* Given log. name not unique */
    pub const NX_FATDRIVE_ERRNO_EBADFD :i32 = 81;   /* f.d. invalid for this operation */
    pub const NX_FATDRIVE_ERRNO_EREMCHG :i32 = 82;   /* Remote address changed */
    pub const NX_FATDRIVE_ERRNO_ELIBACC :i32 = 83;   /* Can't access a needed shared lib */
    pub const NX_FATDRIVE_ERRNO_ELIBBAD :i32 = 84;   /* Accessing a corrupted shared lib */
    pub const NX_FATDRIVE_ERRNO_ELIBSCN :i32 = 85;   /* .lib section in a.out corrupted */
    pub const NX_FATDRIVE_ERRNO_ELIBMAX :i32 = 86;   /* Attempting to link in too many libs */
    pub const NX_FATDRIVE_ERRNO_ELIBEXEC :i32 = 87;   /* Attempting to exec a shared library */
    pub const NX_FATDRIVE_ERRNO_ENOSYS :i32 = 88;   /* Function not implemented */
    pub const NX_FATDRIVE_ERRNO_ENMFILE :i32 = 89;   /* No more files */
    pub const NX_FATDRIVE_ERRNO_ENOTEMPTY :i32 = 90;   /* Directory not empty */
    pub const NX_FATDRIVE_ERRNO_ENAMETOOLONG :i32 = 91;   /* File or path name too long */
    pub const NX_FATDRIVE_ERRNO_ELOOP :i32 = 92;   /* Too many symbolic links */
    pub const NX_FATDRIVE_ERRNO_EOPNOTSUPP :i32 = 95;   /* Operation not supported on socket */
    pub const NX_FATDRIVE_ERRNO_EPFNOSUPPORT :i32 = 96;   /* Protocol family not supported */
    pub const NX_FATDRIVE_ERRNO_ECONNRESET :i32 = 104;   /* Connection reset by peer */
    pub const NX_FATDRIVE_ERRNO_ENOBUFS :i32 = 105;   /* No buffer space available */
    pub const NX_FATDRIVE_ERRNO_EAFNOSUPPORT :i32 = 106;   /* Address family not supported by protocol family */
    pub const NX_FATDRIVE_ERRNO_EPROTOTYPE :i32 = 107;   /* Protocol wrong type for socket */
    pub const NX_FATDRIVE_ERRNO_ENOTSOCK :i32 = 108;   /* Socket operation on non-socket */
    pub const NX_FATDRIVE_ERRNO_ENOPROTOOPT :i32 = 109;   /* Protocol not available */
    pub const NX_FATDRIVE_ERRNO_ESHUTDOWN :i32 = 110;   /* Can't send after socket shutdown */
    pub const NX_FATDRIVE_ERRNO_ECONNREFUSED :i32 = 111;   /* Connection refused */
    pub const NX_FATDRIVE_ERRNO_EADDRINUSE :i32 = 112;   /* Address already in use */
    pub const NX_FATDRIVE_ERRNO_ECONNABORTED :i32 = 113;   /* Software caused connection abort */
    pub const NX_FATDRIVE_ERRNO_ENETUNREACH :i32 = 114;   /* Network is unreachable */
    pub const NX_FATDRIVE_ERRNO_ENETDOWN :i32 = 115;   /* Network interface is not configured */
    pub const NX_FATDRIVE_ERRNO_ETIMEDOUT :i32 = 116;   /* Connection timed out */
    pub const NX_FATDRIVE_ERRNO_EHOSTDOWN :i32 = 117;   /* Host is down */
    pub const NX_FATDRIVE_ERRNO_EHOSTUNREACH :i32 = 118;   /* Host is unreachable */
    pub const NX_FATDRIVE_ERRNO_EINPROGRESS :i32 = 119;   /* Connection already in progress */
    pub const NX_FATDRIVE_ERRNO_EALREADY :i32 = 120;   /* Socket already connected */
    pub const NX_FATDRIVE_ERRNO_EDESTADDRREQ :i32 = 121;   /* Destination address required */
    pub const NX_FATDRIVE_ERRNO_EMSGSIZE :i32 = 122;   /* Message too long */
    pub const NX_FATDRIVE_ERRNO_EPROTONOSUPPORT :i32 = 123;   /* Unknown protocol */
    pub const NX_FATDRIVE_ERRNO_ESOCKTNOSUPPORT :i32 = 124;   /* Socket type not supported */
    pub const NX_FATDRIVE_ERRNO_EADDRNOTAVAIL :i32 = 125;   /* Address not available */
    pub const NX_FATDRIVE_ERRNO_ENETRESET :i32 = 126;   /* Connection aborted by network */
    pub const NX_FATDRIVE_ERRNO_EISCONN :i32 = 127;   /* Socket is already connected */
    pub const NX_FATDRIVE_ERRNO_ENOTCONN :i32 = 128;   /* Socket is not connected */
    pub const NX_FATDRIVE_ERRNO_ETOOMANYREFS :i32 = 129;
    pub const NX_FATDRIVE_ERRNO_EPROCLIM :i32 = 130;
    pub const NX_FATDRIVE_ERRNO_EUSERS :i32 = 131;
    pub const NX_FATDRIVE_ERRNO_EDQUOT :i32 = 132;
    pub const NX_FATDRIVE_ERRNO_ESTALE :i32 = 133;
    pub const NX_FATDRIVE_ERRNO_ENOTSUP :i32 = 134;   /* Not supported */
    pub const NX_FATDRIVE_ERRNO_ENOMEDIUM :i32 = 135;   /* No medium (in tape drive) */
    pub const NX_FATDRIVE_ERRNO_ENOSHARE :i32 = 136;   /* No such host or network path */
    pub const NX_FATDRIVE_ERRNO_ECASECLASH :i32 = 137;   /* Filename exists with different case */
    pub const NX_FATDRIVE_ERRNO_EILSEQ :i32 = 138;   /* Illegal byte sequence */
    pub const NX_FATDRIVE_ERRNO_EOVERFLOW :i32 = 139;   /* Value too large for defined data type */
    pub const NX_FATDRIVE_ERRNO_ECANCELED :i32 = 140;   /* Operation canceled */
    pub const NX_FATDRIVE_ERRNO_ENOTRECOVERABLE :i32 = 141;   /* State not recoverable */
    pub const NX_FATDRIVE_ERRNO_EOWNERDEAD :i32 = 142;   /* Previous owner died */
    pub const NX_FATDRIVE_ERRNO_ESTRPIPE :i32 = 143;   /* Streams pipe error */
}