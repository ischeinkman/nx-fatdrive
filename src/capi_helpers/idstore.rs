use crate::OffsetScsiDevice;
use super::*;
use super::err;
use super::err::LibnxErrMapper;
use fatfs::{Dir, DirEntry, File, FileSystem, ReadWriteSeek};
use crate::get_filesystem;
use std::collections::HashMap;
use std::io::{Read, Write};


pub struct DirEntryData {
    pub name : String, 
    pub type_val : u64, 
    pub size : u64,
}
pub struct IdStore {
    next_id : u64,
    dir_handle_map : HashMap<u64, Dir<'static, OffsetScsiDevice>>,
    dir_name_map : HashMap<u64, String>, 
    dir_iter_map : HashMap<u64, u64>,
    file_handle_map : HashMap<u64, File<'static, OffsetScsiDevice>>,
    file_name_map : HashMap<u64, String>,
}

impl IdStore {

    const _DT_DIR : u64 = 0x4;
    const _DT_REG : u64 = 0x1;
    
    pub fn new() -> IdStore {
        IdStore {
            next_id : 0, 
            dir_handle_map : HashMap::new(),
            dir_name_map : HashMap::new(),
            dir_iter_map : HashMap::new(),
            file_handle_map : HashMap::new(),
            file_name_map : HashMap::new(),
        }
    }

    pub fn has_file(&self, path : &String) -> Option<u64> {
        self.file_name_map.iter().find_map(|kv| {
            let (k, v) = kv;
            if v == path {
                Some(*k)
            }
            else {
                None
            }
        })
    }

    pub fn has_dir(&self, path : &String) -> Option<u64> {
        self.dir_name_map.iter().find_map(|kv| {
            let (k, v) = kv;
            if v == path {
                Some(*k)
            }
            else {
                None
            }
        })
    }

    pub fn insert_file(&mut self, path : String, fl : File<'static, OffsetScsiDevice>) -> u64 {
        let id = self.next_id;
        self.next_id = if id == u64::max_value() { 0 } else { id + 1 };
        self.file_handle_map.insert(id, fl);
        self.file_name_map.insert(id, path);
        id
    }

    pub fn insert_dir(&mut self, path : String, dir : Dir<'static, OffsetScsiDevice>) -> u64 {
        let id = self.next_id;
        self.next_id = if id == u64::max_value() { 0 } else { id + 1 };
        self.dir_handle_map.insert(id, dir);
        self.dir_name_map.insert(id, path);
        self.dir_iter_map.insert(id, 0);
        id
    }

    pub unsafe fn open_file(&mut self, path : &str) -> Result<u64, u32> {
        let path = path.to_owned();
        if let Some(existing) = self.has_file(&path) {
            return Ok(existing);
        }
        let (fs, _guard) = get_filesystem()?;
        let new_fl = fs.root_dir().open_file(&path).map_err(LibnxErrMapper::map)?;
        Ok(self.insert_file(path, new_fl))
    }

    pub unsafe fn open_dir(&mut self, path : &str) -> Result<u64, u32> {
        let path = path.to_owned();
        if let Some(existing) = self.has_dir(&path) {
            return Ok(existing);
        }
        let (fs, _guard) = get_filesystem()?;
        let new_fl = if path == "/" || path == "" {
            fs.root_dir()
        }
        else {
            fs.root_dir().open_dir(&path).map_err(LibnxErrMapper::map)? 
        };
        Ok(self.insert_dir(path, new_fl))
    }

    pub fn close_file(&mut self, id : u64) -> Result<(), u32> {
        let mut f = match self.file_handle_map.remove(&id) {
            Some(f) => f,
            None => {
                return Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND);
            }
        };
        f.flush().map_err(LibnxErrMapper::map)?;
        self.file_name_map.remove(&id);
        Ok(())
    }

    pub fn close_dir(&mut self, id : u64) -> Result<(), u32> {
        let mut f = match self.dir_handle_map.remove(&id) {
            Some(f) => f,
            None => {
                return Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND);
            }
        };
        self.dir_name_map.remove(&id);
        self.dir_iter_map.remove(&id);
        Ok(())
    }

    pub fn get_file_handle<'a>(&'a mut self, id : u64) -> Result<&'a mut File<'static, OffsetScsiDevice>, u32> {
        let existing = match self.file_handle_map.get_mut(&id) {
            Some(f) => f,
            None => {
                return Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND);
            }
        };
        Ok(existing)
    }

    pub fn read_next_dirent(&mut self, id : u64) -> Result<Option<DirEntryData>, u32> {
        let dir = match self.dir_handle_map.get_mut(&id) {
            Some(d) => d, 
            None => {
                return Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND);
            }
        };

        let idx_ref = match self.dir_iter_map.get_mut(&id) {
            Some(idx) => idx, 
            None => {
                return Err(NX_FATDRIVE_ERR_UNKNOWN);
            }
        };
        let idx = *idx_ref;
        *idx_ref += 1;

        let mut dir_iter = dir.iter().skip(idx as usize);

        let retval_source = match dir_iter.next() {
            Some(Ok(r)) => r,
            Some(Err(e)) => {
                return Err(LibnxErrMapper::map(e));
            },
            None => {
                return Ok(None);
            }
        };
        

        Ok(Some(DirEntryData {
            name : retval_source.file_name(),
            type_val : if retval_source.is_file() { Self::_DT_REG } else { Self::_DT_DIR },
            size : retval_source.len(),
        }))
    }

    pub fn get_path_for_id(&self, id : u64) -> Result<&str, u32> {
        if let Some(p) = self.file_name_map.get(&id) {
            return Ok(&p);
        }
        else if let Some(p) = self.dir_name_map.get(&id) {
            return Ok(&p);
        }
        else {
            return Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND);
        }
    }

    pub unsafe fn stat_path(&self, path : &str) -> Result<(u64, u64), u32> {
        let stripped_path = path.replace("//", "/").trim_end_matches('/').to_owned();
        let mut path_itr = stripped_path.rsplitn(2, '/');
        let ent_name = match path_itr.next() {
            Some(e) => e,
            None => {
                return Err(NX_FATDRIVE_ERR_UNKNOWN);
            }
        };
        let parent_name = path_itr.next();
        let mut parent_handle_itr = if let Some(id) = parent_name.and_then(|nm| self.has_dir(&nm.to_owned())) {
            match self.dir_handle_map.get(&id) {
                Some(h) => h.iter(),
                None => {
                    return Err(NX_FATDRIVE_ERR_UNKNOWN);
                }
            }
        }
        else if !(parent_name.map(|nm| nm.is_empty()).unwrap_or(true)) {
            let (mut fs, _fs_guard) = get_filesystem().map_err(LibnxErrMapper::map)?;
            fs.root_dir().open_dir(parent_name.unwrap()).map_err(LibnxErrMapper::map)?.iter()
        }
        else {
            let (mut fs, _fs_guard) = get_filesystem().map_err(LibnxErrMapper::map)?;
            fs.root_dir().iter()
        };

        parent_handle_itr.find_map(|entres| {
            let ent = match entres.map_err(LibnxErrMapper::map) {
                Ok(e) => e,
                Err(err) => {
                    return Some(Err(err));
                }
            };
            if ent.file_name().trim_end_matches('/') != ent_name {
                return None;
            }
            return Some( Ok( (ent.len(), ent.attributes().bits() as u64)));
        }).unwrap_or(Err(NX_FATDRIVE_ERR_FILE_NOT_FOUND))
    }
}