use std::io::{Read, Write, Seek};
use crate::buf_scsi::OffsetScsiDevice;
pub mod fatfs_rs;


pub trait FileSystemOps {
    fn root(&mut self) -> Directory;
    fn stats(&self) -> FsStats;
}

pub enum FileSystem {
    Fatfs(fatfs::FileSystem<OffsetScsiDevice>),
}

pub trait FileOps : Read + Write + Seek {
    fn flush(&mut self) -> Result<(), u32>;
    fn truncate(&mut self) -> Result<(), u32>;
}

pub enum File<'a> {
    Fatfs(fatfs::File<'a, OffsetScsiDevice>),
}

pub trait DirectoryOps : Sized {
    fn open_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, std::io::Error>;
    fn create_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, std::io::Error>;
    fn open_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error>;
    fn create_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error>;
    fn remove_path<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<(), std::io::Error>;
    fn iter<'a>(&'a mut self) -> DirIter<'a>;
}

pub enum Directory<'a> {
    Fatfs(fatfs_rs::FatfsDirectory<'a>),
}

pub trait DirIterOps : Iterator<Item=DirEntryData> {

}

pub enum DirIter<'a> {
    Fatfs(fatfs_rs::FatfsDirIter<'a> ),
}



pub struct FsStats {
    pub cluster_size : u64, 
    pub free_clusters : u64,
    pub total_clusters : u64, 
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntryData {
    pub name : String, 
    pub len : usize,
    pub flags : u64, 
}

impl DirEntryData {
    pub fn entry_type(&self) -> DirEntryType {
        let flag_byte = ((self.flags & 0xf000) >> 12) as u8;
        flag_byte.into()
    }
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum DirEntryType {
    Unknown, 
    Fifo, 
    CharacterSpecial,
    Directory, 
    BlockSpecial,
    RegularFile, 
    SymbolicLink,
    Socket, 
}
const _S_IFIFO : u8 = 1;/* named pipe (fifo) */
const _S_IFCHR : u8 = 2;/* character special */
const _S_IFDIR : u8 = 4;/* directory */
const _S_IFBLK : u8 = 6;/* block special */
const _S_IFREG : u8 = 8;/* regular */
const _S_IFLNK : u8 = 10;/* symbolic link */
const _S_IFSOCK : u8 = 12;/* socket */

impl From<u8> for DirEntryType {
    fn from(inner : u8) -> DirEntryType {
        match inner {
            _S_IFIFO => DirEntryType::Fifo,
            _S_IFCHR => DirEntryType::CharacterSpecial,
            _S_IFDIR => DirEntryType::Directory,
            _S_IFBLK => DirEntryType::BlockSpecial,
            _S_IFREG => DirEntryType::RegularFile,
            _S_IFLNK => DirEntryType::SymbolicLink,
            _S_IFSOCK => DirEntryType::Socket,
            _ => DirEntryType::Unknown,
        }
    }
}

impl From<DirEntryType> for u8 {
    fn from(wrapped : DirEntryType) -> u8 {
        match wrapped {
            DirEntryType::Fifo => _S_IFIFO,
            DirEntryType::CharacterSpecial => _S_IFCHR,
            DirEntryType::Directory => _S_IFDIR,
            DirEntryType::BlockSpecial => _S_IFBLK,
            DirEntryType::RegularFile => _S_IFREG,
            DirEntryType::SymbolicLink => _S_IFLNK,
            DirEntryType::Socket => _S_IFSOCK,
            DirEntryType::Unknown => 0,
        }
    }
}
