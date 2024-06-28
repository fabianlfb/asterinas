// SPDX-License-Identifier: MPL-2.0

#![allow(unused_variables)]

use core::time::Duration;

use aster_rights::Full;
use core2::io::{Error as IoError, ErrorKind as IoErrorKind, Result as IoResult, Write};

use super::{DirentVisitor, FileSystem, IoctlCmd};
use crate::{
    events::IoEvents,
    fs::device::{Device, DeviceType},
    prelude::*,
    process::{signal::Poller, Gid, Uid},
    time::clocks::RealTimeCoarseClock,
    vm::vmo::Vmo,
};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromInt)]
pub enum InodeType {
    NamedPipe = 0o010000,
    CharDevice = 0o020000,
    Dir = 0o040000,
    BlockDevice = 0o060000,
    File = 0o100000,
    SymLink = 0o120000,
    Socket = 0o140000,
}

impl InodeType {
    pub fn support_read(&self) -> bool {
        matches!(
            self,
            InodeType::File | InodeType::Socket | InodeType::CharDevice | InodeType::BlockDevice
        )
    }

    pub fn support_write(&self) -> bool {
        matches!(
            self,
            InodeType::File | InodeType::Socket | InodeType::CharDevice | InodeType::BlockDevice
        )
    }

    pub fn is_regular_file(&self) -> bool {
        *self == InodeType::File
    }

    pub fn is_directory(&self) -> bool {
        *self == InodeType::Dir
    }
}

impl From<DeviceType> for InodeType {
    fn from(type_: DeviceType) -> InodeType {
        match type_ {
            DeviceType::CharDevice => InodeType::CharDevice,
            DeviceType::BlockDevice => InodeType::BlockDevice,
            DeviceType::MiscDevice => InodeType::CharDevice,
        }
    }
}

bitflags! {
    pub struct InodeMode: u16 {
        /// set-user-ID
        const S_ISUID = 0o4000;
        /// set-group-ID
        const S_ISGID = 0o2000;
        /// sticky bit
        const S_ISVTX = 0o1000;
        /// read by owner
        const S_IRUSR = 0o0400;
        /// write by owner
        const S_IWUSR = 0o0200;
        /// execute/search by owner
        const S_IXUSR = 0o0100;
        /// read by group
        const S_IRGRP = 0o0040;
        /// write by group
        const S_IWGRP = 0o0020;
        /// execute/search by group
        const S_IXGRP = 0o0010;
        /// read by others
        const S_IROTH = 0o0004;
        /// write by others
        const S_IWOTH = 0o0002;
        /// execute/search by others
        const S_IXOTH = 0o0001;
    }
}

impl InodeMode {
    pub fn is_readable(&self) -> bool {
        self.contains(Self::S_IRUSR)
    }

    pub fn is_writable(&self) -> bool {
        self.contains(Self::S_IWUSR)
    }

    pub fn is_executable(&self) -> bool {
        self.contains(Self::S_IXUSR)
    }

    pub fn has_sticky_bit(&self) -> bool {
        self.contains(Self::S_ISVTX)
    }

    pub fn has_set_uid(&self) -> bool {
        self.contains(Self::S_ISUID)
    }

    pub fn has_set_gid(&self) -> bool {
        self.contains(Self::S_ISGID)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Metadata {
    pub dev: u64,
    pub ino: u64,
    pub size: usize,
    pub blk_size: usize,
    pub blocks: usize,
    pub atime: Duration,
    pub mtime: Duration,
    pub ctime: Duration,
    pub type_: InodeType,
    pub mode: InodeMode,
    pub nlinks: usize,
    pub uid: Uid,
    pub gid: Gid,
    pub rdev: u64,
}

impl Metadata {
    pub fn new_dir(ino: u64, mode: InodeMode, blk_size: usize) -> Self {
        let now = RealTimeCoarseClock::get().read_time();
        Self {
            dev: 0,
            ino,
            size: 2,
            blk_size,
            blocks: 1,
            atime: now,
            mtime: now,
            ctime: now,
            type_: InodeType::Dir,
            mode,
            nlinks: 2,
            uid: Uid::new_root(),
            gid: Gid::new_root(),
            rdev: 0,
        }
    }

    pub fn new_file(ino: u64, mode: InodeMode, blk_size: usize) -> Self {
        let now = RealTimeCoarseClock::get().read_time();
        Self {
            dev: 0,
            ino,
            size: 0,
            blk_size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            type_: InodeType::File,
            mode,
            nlinks: 1,
            uid: Uid::new_root(),
            gid: Gid::new_root(),
            rdev: 0,
        }
    }

    pub fn new_symlink(ino: u64, mode: InodeMode, blk_size: usize) -> Self {
        let now = RealTimeCoarseClock::get().read_time();
        Self {
            dev: 0,
            ino,
            size: 0,
            blk_size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            type_: InodeType::SymLink,
            mode,
            nlinks: 1,
            uid: Uid::new_root(),
            gid: Gid::new_root(),
            rdev: 0,
        }
    }
    pub fn new_device(ino: u64, mode: InodeMode, blk_size: usize, device: &dyn Device) -> Self {
        let now = RealTimeCoarseClock::get().read_time();
        Self {
            dev: 0,
            ino,
            size: 0,
            blk_size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            type_: InodeType::from(device.type_()),
            mode,
            nlinks: 1,
            uid: Uid::new_root(),
            gid: Gid::new_root(),
            rdev: device.id().into(),
        }
    }

    pub fn new_socket(ino: u64, mode: InodeMode, blk_size: usize) -> Metadata {
        let now = RealTimeCoarseClock::get().read_time();
        Self {
            dev: 0,
            ino,
            size: 0,
            blk_size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            type_: InodeType::Socket,
            mode,
            nlinks: 1,
            uid: Uid::new_root(),
            gid: Gid::new_root(),
            rdev: 0,
        }
    }
}

pub trait Inode: Any + Sync + Send {
    fn size(&self) -> usize;

    fn resize(&self, new_size: usize) -> Result<()>;

    fn metadata(&self) -> Metadata;

    fn ino(&self) -> u64;

    fn type_(&self) -> InodeType;

    fn mode(&self) -> Result<InodeMode>;

    fn set_mode(&self, mode: InodeMode) -> Result<()>;

    fn owner(&self) -> Result<Uid>;

    fn set_owner(&self, uid: Uid) -> Result<()>;

    fn group(&self) -> Result<Gid>;

    fn set_group(&self, gid: Gid) -> Result<()>;

    fn atime(&self) -> Duration;

    fn set_atime(&self, time: Duration);

    fn mtime(&self) -> Duration;

    fn set_mtime(&self, time: Duration);

    fn ctime(&self) -> Duration;

    fn set_ctime(&self, time: Duration);

    fn page_cache(&self) -> Option<Vmo<Full>> {
        None
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        Err(Error::new(Errno::EISDIR))
    }

    fn read_direct_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        Err(Error::new(Errno::EISDIR))
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize> {
        Err(Error::new(Errno::EISDIR))
    }

    fn write_direct_at(&self, offset: usize, buf: &[u8]) -> Result<usize> {
        Err(Error::new(Errno::EISDIR))
    }

    fn create(&self, name: &str, type_: InodeType, mode: InodeMode) -> Result<Arc<dyn Inode>> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn mknod(&self, name: &str, mode: InodeMode, dev: Arc<dyn Device>) -> Result<Arc<dyn Inode>> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn as_device(&self) -> Option<Arc<dyn Device>> {
        None
    }

    fn readdir_at(&self, offset: usize, visitor: &mut dyn DirentVisitor) -> Result<usize> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn link(&self, old: &Arc<dyn Inode>, name: &str) -> Result<()> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn unlink(&self, name: &str) -> Result<()> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn rmdir(&self, name: &str) -> Result<()> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn lookup(&self, name: &str) -> Result<Arc<dyn Inode>> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn rename(&self, old_name: &str, target: &Arc<dyn Inode>, new_name: &str) -> Result<()> {
        Err(Error::new(Errno::ENOTDIR))
    }

    fn read_link(&self) -> Result<String> {
        Err(Error::new(Errno::EISDIR))
    }

    fn write_link(&self, target: &str) -> Result<()> {
        Err(Error::new(Errno::EISDIR))
    }

    fn ioctl(&self, cmd: IoctlCmd, arg: usize) -> Result<i32> {
        Err(Error::new(Errno::EISDIR))
    }

    fn sync_all(&self) -> Result<()> {
        Ok(())
    }

    fn sync_data(&self) -> Result<()> {
        Ok(())
    }

    fn poll(&self, mask: IoEvents, _poller: Option<&Poller>) -> IoEvents {
        let events = IoEvents::IN | IoEvents::OUT;
        events & mask
    }

    fn fs(&self) -> Arc<dyn FileSystem>;

    /// Returns whether a VFS dentry for this inode should be put into the dentry cache.
    ///
    /// The dentry cache in the VFS layer can accelerate the lookup of inodes. So usually,
    /// it is preferable to use the dentry cache. And thus, the default return value of this method
    /// is `true`.
    ///
    /// But this caching can raise consistency issues in certain use cases. Specifically, the dentry
    /// cache works on the assumption that all FS operations go through the dentry layer first.
    /// This is why the dentry cache can reflect the up-to-date FS state. Yet, this assumption
    /// may be broken. If the inodes of a file system may "disappear" without unlinking through the
    /// VFS layer, then their dentries should not be cached. For example, an inode in procfs
    /// (say, `/proc/1/fd/2`) can "disappear" without notice from the perspective of the dentry cache.
    /// So for such inodes, they are incompatible with the dentry cache. And this method returns `false`.
    ///
    /// Note that if any ancestor directory of an inode has this method returns `false`, then
    /// this inode would not be cached by the dentry cache, even when the method of this
    /// inode returns `true`.
    fn is_dentry_cacheable(&self) -> bool {
        true
    }
}

impl dyn Inode {
    pub fn downcast_ref<T: Inode>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }

    pub fn read_all(&self, buf: &mut Vec<u8>) -> Result<usize> {
        if !self.type_().support_read() {
            return_errno!(Errno::EISDIR);
        }

        let file_size = self.size();
        if buf.len() < file_size {
            buf.resize(file_size, 0);
        }
        self.read_at(0, &mut buf[..file_size])
    }

    pub fn read_direct_all(&self, buf: &mut Vec<u8>) -> Result<usize> {
        if !self.type_().support_read() {
            return_errno!(Errno::EISDIR);
        }

        let file_size = self.size();
        if buf.len() < file_size {
            buf.resize(file_size, 0);
        }
        self.read_direct_at(0, &mut buf[..file_size])
    }

    pub fn writer(&self, from_offset: usize) -> InodeWriter {
        InodeWriter {
            inner: self,
            offset: from_offset,
        }
    }
}

pub struct InodeWriter<'a> {
    inner: &'a dyn Inode,
    offset: usize,
}

impl<'a> Write for InodeWriter<'a> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let write_len = self
            .inner
            .write_at(self.offset, buf)
            .map_err(|_| IoError::new(IoErrorKind::WriteZero, "failed to write buffer"))?;
        self.offset += write_len;
        Ok(write_len)
    }

    #[inline]
    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

impl Debug for dyn Inode {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("Inode")
            .field("metadata", &self.metadata())
            .field("fs", &self.fs())
            .finish()
    }
}
