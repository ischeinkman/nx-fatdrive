use std::io::{Read, Write, Seek};
use crate::buf_scsi::OffsetScsiDevice;
use mbr_nostd::PartitionTableEntry;
pub mod fatfs_rs;
pub mod fatfs_raw;


pub trait FileSystemOps : Sized {
    fn root(&mut self) -> Result<Directory, std::io::Error>;
    fn stats(&self) -> Result<FsStats, std::io::Error>;
    fn from_device(dev : OffsetScsiDevice, part : PartitionTableEntry) -> Result<Self, std::io::Error>;
}

pub enum FileSystem {
    Fatfs(fatfs::FileSystem<OffsetScsiDevice>),
    FatfsSys(fatfs_raw::FatfsSysFileSystem),
}

impl FileSystemOps for FileSystem {
    fn root(&mut self) -> Result<Directory, std::io::Error> {
        match self {
            FileSystem::Fatfs(f) => FileSystemOps::root(f),
            FileSystem::FatfsSys(f) => FileSystemOps::root(f),
        }
    }
    fn stats(&self) -> Result<FsStats, std::io::Error> {
        match self {
            FileSystem::Fatfs(f) => FileSystemOps::stats(f),
            FileSystem::FatfsSys(f) => FileSystemOps::stats(f),
        }
    }
    fn from_device(dev : OffsetScsiDevice, part : PartitionTableEntry) -> Result<Self, std::io::Error> {
        //TODO: how do we deal with ownership?
        fatfs_raw::FatfsSysFileSystem::from_device(dev, part).map(|f| FileSystem::FatfsSys(f))
    }

}

pub trait FileOps : Read + Write + Seek {
    fn truncate(&mut self) -> Result<(), std::io::Error>;
}

pub enum File<'a> {
    Fatfs(fatfs::File<'a, OffsetScsiDevice>),
    FatfsSys(fatfs_raw::FatfsSysFile),
}

impl <'a> Read for File<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        match self {
            File::Fatfs(f) => Read::read(f, buf),
            File::FatfsSys(f) => Read::read(f, buf),
        }
    }
}
impl <'a> Write for File<'a> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        match self {
            File::Fatfs(f) => Write::write(f, buf),
            File::FatfsSys(f) => Write::write(f, buf),
        }
    }
    fn flush(&mut self) -> Result<(), std::io::Error> {
        match self {
            File::Fatfs(f) => Write::flush(f),
            File::FatfsSys(f) => Write::flush(f),
        }
    }
}

impl <'a> Seek for File<'a> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> Result<u64, std::io::Error> {
        match self {
            File::Fatfs(f) => Seek::seek(f, pos),
            File::FatfsSys(f) => Seek::seek(f, pos),
        }
    }

}

impl <'a> FileOps for File<'a> {
    fn truncate(&mut self) -> Result<(), std::io::Error> {
        match self {
            File::Fatfs(f) => FileOps::truncate(f),
            File::FatfsSys(f) => FileOps::truncate(f),
        }
    }

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
    FatfsSys(fatfs_raw::FatfsSysDir),
}

impl <'a> DirectoryOps for Directory<'a> {
    fn open_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, std::io::Error> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::open_directory(f, path),
            Directory::FatfsSys(f) => DirectoryOps::open_directory(f, path),
        }
    }
    fn create_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, std::io::Error> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::create_directory(f, path),
            Directory::FatfsSys(f) => DirectoryOps::create_directory(f, path),
        }
    }
    fn open_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::open_file(f, path),
            Directory::FatfsSys(f) => DirectoryOps::open_file(f, path),
        }
    }
    fn create_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, std::io::Error> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::create_file(f, path),
            Directory::FatfsSys(f) => DirectoryOps::create_file(f, path),
        }
    }
    fn remove_path<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<(), std::io::Error> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::remove_path(f, path),
            Directory::FatfsSys(f) => DirectoryOps::remove_path(f, path),
        }
    }
    fn iter<'b>(&'b mut self) -> DirIter<'b> {
        match self {
            Directory::Fatfs(f) => DirectoryOps::iter(f),
            Directory::FatfsSys(f) => DirectoryOps::iter(f),
        }
    }
    
}

pub trait DirIterOps : Iterator<Item=DirEntryData> {

}

pub enum DirIter<'a> {
    Fatfs(fatfs_rs::FatfsDirIter<'a> ),
    FatfsSys(fatfs_raw::FatfsSysDirIter<'a> ),
}

impl <'a> Iterator for DirIter<'a> {
    type Item = DirEntryData;
    fn next(&mut self) -> Option<DirEntryData> {
        match self {
            DirIter::Fatfs(f) => Iterator::next(f),
            DirIter::FatfsSys(f) => Iterator::next(f),
        }
    }
}

impl <'a> DirIterOps for DirIter<'a> {}

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
