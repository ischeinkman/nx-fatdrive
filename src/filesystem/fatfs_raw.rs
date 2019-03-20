
use fatfs_sys::{
    FIL, DIR, FRESULT, FILINFO, FATFS,
    FA_CREATE_NEW, FA_OPEN_EXISTING,
    f_close, f_closedir, f_open, f_opendir, 
    f_sync, f_readdir,
    f_read, f_write, f_lseek, 
    f_unlink, f_mkdir,
    f_truncate, f_getfree,
};
use super::{FileOps, FileSystemOps, DirectoryOps, DirIterOps, File, Directory, DirIter, DirEntryData, FsStats};
use std::io::{Read, Write, Seek, SeekFrom, Error, ErrorKind};
use std::ffi::{CString, CStr};
use std::os::raw::c_void;
use std::ptr;

pub struct FatfsSysFileSystem {
    path : Option<CString>,
}

impl FileSystemOps for FatfsSysFileSystem {
    fn root(&mut self) -> Result<Directory, std::io::Error> {
        let mut inner = DIR::default();
        let path = "/\0";
        let err = unsafe { f_opendir(&mut inner as *mut _, path.as_ptr() as *const _)};
        let retval = FatfsSysDir::from_inner(wrap_errors(inner, err)?);
        Ok(Directory::FatfsSys(retval))
    }
    fn stats(&self) -> Result<FsStats, std::io::Error> {
        let mut raw : *mut FATFS = ptr::null_mut();
        let mut nclst = 0u32;
        let pathvar = self.path.as_ref().map_or(ptr::null(), |pt| pt.as_ptr());
        let err = unsafe { f_getfree(pathvar, &mut nclst as *mut _, &mut raw as *mut _)};
        let checked_raw = wrap_errors(raw, err)?;
        let fs : &FATFS = unsafe {
            checked_raw.as_ref().ok_or(std::io::Error::from(ErrorKind::NotFound))?
        };
        let ssize = 512;
        let cluster_size = (fs.csize as u64) * (ssize);
        let total_clusters = (fs.n_fatent - 2) as u64;
        let free_clusters = nclst as u64;
        let retval = FsStats {
            cluster_size,
            total_clusters, 
            free_clusters,
        };
        Ok(retval)
    }
}

pub struct FatfsSysFile {
    inner : FIL, 
}

impl Read for FatfsSysFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut retval : fatfs_sys::UINT = 0;
        let buff_ptr : *mut c_void = buf.as_mut_ptr() as *mut c_void;
        let buflen : fatfs_sys::UINT = buf.len() as fatfs_sys::UINT;
        let err = unsafe {
            f_read(&mut self.inner as *mut _, buff_ptr, buflen, &mut retval as *mut _)
        };
        wrap_errors(retval as usize, err)
    }
}

impl Seek for FatfsSysFile {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let pos_from_start = match pos {
            SeekFrom::Start(raw) => raw, 
            SeekFrom::Current(raw) => {
                let current_pos = self.inner.fptr as i64;
                (current_pos + raw) as u64
            },
            SeekFrom::End(raw) => {
                let end = self.inner.obj.objsize as i64;
                (raw + end) as u64
            }
        };
        let err = unsafe{f_lseek(&mut self.inner as *mut _, pos_from_start as u32)};
        wrap_errors(self.inner.fptr as u64, err)
    }
}

impl Write for FatfsSysFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let buff_ptr : *const c_void = buf.as_ptr() as *const c_void;
        let buflen : fatfs_sys::UINT = buf.len() as fatfs_sys::UINT;
        let mut retval : fatfs_sys::UINT = 0;
        let err = unsafe {
            f_write(&mut self.inner as *mut _, buff_ptr, buflen, &mut retval as *mut _)
        };
        wrap_errors(retval as usize, err)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let err = unsafe { f_sync(&mut self.inner as *mut _)};
        wrap_errors((), err)
    }
}

impl FileOps for FatfsSysFile {
    fn truncate(&mut self) -> Result<(), std::io::Error> {
        let err = unsafe { f_truncate(&mut self.inner as *mut _)};
        wrap_errors((), err)
    }
}


fn wrap_errors<T>(possible : T, err : FRESULT) -> std::io::Result<T> {
    match err {
        FRESULT::FR_OK => Ok(possible),
        FRESULT::FR_WRITE_PROTECTED => Err(Error::from(ErrorKind::PermissionDenied)),
        FRESULT::FR_TOO_MANY_OPEN_FILES => Err(Error::from(ErrorKind::AddrInUse)),
        _ => Err(Error::from(ErrorKind::Other)),
    }
}

impl Drop for FatfsSysFile {
    fn drop(&mut self) {
        let _e = unsafe { f_close(&mut self.inner as *mut _) };
    }
} 

pub struct FatfsSysDir {
    inner : DIR, 
    children : Vec<DirEntryData>,
    finished_reading_children : bool,
}
impl FatfsSysDir {

    pub fn from_inner(inner : DIR) -> FatfsSysDir {
        FatfsSysDir {
            inner, 
            children : Vec::new(),
            finished_reading_children : false,
        }
    }
    fn raw_readdir(&mut self) -> Result<Option<DirEntryData>, std::io::Error> {
        let mut fno = FILINFO::default();
        let err = unsafe { f_readdir(&mut self.inner as *mut _, &mut fno as *mut _)};
        let rawinfo = wrap_errors(fno, err)?;
        if rawinfo.fname[0] == 0 {
            return Ok(None);
        }
        let name_cstr = unsafe { CStr::from_ptr(&rawinfo.fname as *const _ as *const _)};
        let name_str = name_cstr.to_string_lossy();
        let retval = DirEntryData {
            name : name_str.to_string(), 
            len : rawinfo.fsize as usize, 
            flags : rawinfo.fattrib as u64,
        };
        Ok(Some(retval))
    }

    fn load_children(&mut self) -> Result<(), std::io::Error> {
        if self.finished_reading_children {
            return Ok(())
        }
        while let Some(ent) = self.raw_readdir()? {
            self.children.push(ent);
        }
        self.finished_reading_children = true;
        Ok(())
    }
}
impl Drop for FatfsSysDir {
    fn drop(&mut self) {
        let _e = unsafe { f_closedir(&mut self.inner as *mut _) };
    }
} 

impl DirectoryOps for FatfsSysDir {
    fn open_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, std::io::Error>{ 
        let mut inner = DIR::default();
        let cpath : CString = CString::new(path.as_ref())?;
        let err_code = unsafe {f_opendir(&mut inner as *mut _, cpath.as_ptr())};
        let retval = Directory::FatfsSys(FatfsSysDir::from_inner(inner));
        wrap_errors(retval, err_code)
    }
    fn create_directory<PathType : AsRef<str>>(&mut self, path : PathType) -> Result<Directory, std::io::Error>{
        let cpath : CString = CString::new(path.as_ref())?;
        let err_code = unsafe {f_mkdir(cpath.as_ptr())};
        wrap_errors((), err_code)?;
        self.open_directory(path)
    }
    fn open_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error>{ 
        let mode = FA_OPEN_EXISTING as u8; 
        let mut inner = FIL::default();
        let cpath : CString = CString::new(path.as_ref())?;
        let err_code = unsafe {f_open(&mut inner as *mut _, cpath.as_ptr(), mode)};
        let retval = File::FatfsSys(FatfsSysFile{ inner });
        wrap_errors(retval, err_code)
    }
    fn create_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error>{
        let mode = FA_CREATE_NEW as u8; 
        let mut inner = FIL::default();
        let cpath : CString = CString::new(path.as_ref())?;
        let err_code = unsafe {f_open(&mut inner as *mut _, cpath.as_ptr(), mode)};
        let retval = File::FatfsSys(FatfsSysFile{ inner });
        wrap_errors(retval, err_code)
    }
    fn remove_path<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<(), std::io::Error>{ 
        let cpath : CString = CString::new(path.as_ref())?;
        let err = unsafe { f_unlink(cpath.as_ptr())};
        wrap_errors((), err)
    }
    fn iter<'a>(&'a mut self) -> DirIter<'a>{ 
        self.load_children();
        DirIter::FatfsSys(FatfsSysDirIter::new(&self.children))
    }
}

pub struct FatfsSysDirIter<'a> {
    items : &'a Vec<DirEntryData>,
    idx : usize,
}

impl <'a> FatfsSysDirIter<'a> {
    pub fn new(items : &'a Vec<DirEntryData>) -> Self {
        FatfsSysDirIter {
            items, 
            idx : 0,
        }
    }
}

impl <'a> Iterator for FatfsSysDirIter<'a> {
    type Item = DirEntryData;
    fn next(&mut self) -> Option<DirEntryData> {
        let retval = self.items.get(self.idx).cloned();
        self.idx += 1;
        retval
    }
}

impl <'a> DirIterOps for FatfsSysDirIter<'a> {}