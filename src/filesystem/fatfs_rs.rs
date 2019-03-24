use fatfs::{Dir,  FileAttributes};
use crate::buf_scsi::OffsetScsiDevice;
use super::{FileOps, File, DirectoryOps, FileSystemOps, DirEntryData, DirEntryType, DirIter, DirIterOps, Directory, FsStats};
use crate::capi_helpers::{LibnxErrMapper};
use mbr_nostd::{PartitionTableEntry, PartitionType};
use std::io::Write;
use std::io;

impl <'a> FileOps for fatfs::File<'a, OffsetScsiDevice> {
    fn truncate(&mut self) -> Result<(), io::Error> {
        fatfs::File::truncate(self)
    }
}

pub struct FatfsDirectory<'a> {
    inner : Dir<'a, OffsetScsiDevice>,
}
pub struct FatfsDirIter<'a> {
    inner : fatfs::DirIter<'a, OffsetScsiDevice>,
}

impl <'a> Iterator for FatfsDirIter<'a> {
    type Item=DirEntryData;
    fn next(&mut self) -> Option<DirEntryData> {
        self.inner.next().and_then(|ent| {
            let ent = match ent {
                Ok(e) => e, 
                Err(_u) => {return None;}
            };
            let type_bits = if ent.attributes().contains(FileAttributes::DIRECTORY) {
                (DirEntryType::Directory as u64) << 12
            } else {
                (DirEntryType::RegularFile as u64) << 12
            };
            let permissions_bits = if ent.attributes().contains(FileAttributes::READ_ONLY) {
                0o444
            } else {
                0o666
            };
            Some(DirEntryData {
                name : ent.file_name(),
                len : ent.len() as usize,
                flags : type_bits | permissions_bits
            })
        })
    }
}

impl <'a> DirIterOps for FatfsDirIter<'a> { }

impl <'a> DirectoryOps for FatfsDirectory<'a> {
    fn open_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, io::Error> {
        let inner = self.inner.open_dir(path.as_ref())?;
        Ok(Directory::Fatfs(FatfsDirectory{inner}))
    }
    fn create_directory<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<Directory, io::Error> {
        let inner = self.inner.create_dir(path.as_ref())?;
        Ok(Directory::Fatfs(FatfsDirectory{inner}))
    }
    fn open_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, io::Error> {
        Ok(File::Fatfs(self.inner.open_file(path.as_ref())?))
    }
    fn create_file<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<File, io::Error> {
        Ok(File::Fatfs(self.inner.create_file(path.as_ref())?))
    }
    fn remove_path<PathType : AsRef<str>>(&mut self, path :PathType) -> Result<(), io::Error> {
        self.inner.remove(path.as_ref())
    }
    fn iter<'b>(&'b mut self) -> DirIter<'b> {
        let raw = self.inner.iter();
        DirIter::Fatfs(FatfsDirIter{ inner : raw })
    }
}

impl FileSystemOps for fatfs::FileSystem<OffsetScsiDevice> {
    fn root(&mut self) -> Result<Directory, io::Error>  {
        Ok(Directory::Fatfs(FatfsDirectory{ inner : self.root_dir()}))
    }
    fn stats(&self) -> Result<FsStats, io::Error> {
        let inner = fatfs::FileSystem::stats(self)?;
        let retval = FsStats {
            cluster_size : inner.cluster_size() as u64,
            total_clusters : inner.total_clusters() as u64, 
            free_clusters : inner.free_clusters() as u64, 
        };
        Ok(retval)
    }
    fn from_device(dev: OffsetScsiDevice, part : PartitionTableEntry) -> Result<Self, io::Error> {
        match part.partition_type {
            PartitionType::Fat32(_) | PartitionType::Fat16(_) | PartitionType::Fat12(_) => Self::new(dev, fatfs::FsOptions::new()),
            _ => Err(io::Error::from(io::ErrorKind::InvalidData))
        }
    }
}