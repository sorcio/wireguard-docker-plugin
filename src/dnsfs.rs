use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use rustix::io::Errno;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::service::NetworkPluginService;
use crate::types::NetworkId;
use crate::wg::Wg;
use cache::{FsCache, NewFileEntry};

const ENOENT: i32 = Errno::NOENT.raw_os_error();

const TTL: Duration = Duration::from_secs(1);

const HELLO_DIR_ATTR: FileAttr = FileAttr {
    ino: INODE_ROOT,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 0,
    gid: 0,
    rdev: 0,
    flags: 0,
    blksize: BLOCK_SIZE as _,
};

const BLOCK_SIZE: usize = 512;

const DEFAULT_FILE_ATTR: FileAttr = FileAttr {
    ino: 0,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::RegularFile,
    perm: 0o644,
    nlink: 1,
    uid: 0,
    gid: 0,
    rdev: 0,
    flags: 0,
    blksize: BLOCK_SIZE as _,
};

const README_TXT_CONTENT: &str = "\
wireguard-docker-plugin dnsfs
=============================

This directory is populated by wireguard-docker-plugin in order to
provide resolv.conf files to containers.

None of the files actually exist on disk. This is a FUSE filesystem.

For each Wireguard configuration file, a virtual file with the same name
will be provided. This file can be bind-mounted into containers as their
resolv.conf file.

Listing the files in this directory might not match the files you expect.
This is because the files are virtual, are created on-demand when needed,
and might be cached even after they are not needed anymore.

Go ahead and acccess ./<config_name>.resolv.conf even if it's not listed
and you will see the nameservers as provided in the DNS line(s) of the
corresponding Wireguard configuration file, if it exists and if there are
any nameservers configured.

You will also notice the ./magic file. If you are accessing it from a
container with a wg0 interface configured by wireguard-docker-plugin, it
will contain the same content as the <config_name>resolv.conf file.
Otherwise it will always appear empty.

Refer to wireguard-docker-plugin documentation for information on how to
use this filesystem. Most of the times you will never interact with it
directly, but instead use the volume mount points provided by the plugin.
";

const README_TXT_ATTR: FileAttr = FileAttr {
    ino: INODE_README,
    size: README_TXT_CONTENT.len() as u64,
    blocks: README_TXT_CONTENT.len().div_ceil(BLOCK_SIZE) as _,
    perm: 0o444,
    ..DEFAULT_FILE_ATTR
};

const MAGIC_FILE_ATTR: FileAttr = FileAttr {
    ino: INODE_MAGIC,
    size: 0,
    blocks: 0,
    perm: 0o444,
    ..DEFAULT_FILE_ATTR
};

mod cache {
    use fuser::{FileAttr, FileType};

    use std::{collections::HashMap, time::SystemTime};

    use super::{BLOCK_SIZE, DEFAULT_FILE_ATTR};

    struct Counter(u64);

    impl Counter {
        fn new(start: u64) -> Self {
            Counter(start)
        }
        fn next(&mut self) -> u64 {
            let next = self.0;
            self.0 = next.checked_add(1).unwrap();
            next
        }
    }

    pub(super) struct FsCache {
        counter: Counter,
        cache: HashMap<u64, FsEntry>,
        lookup_cache: HashMap<String, u64>,
    }

    pub(super) struct FsEntry {
        ref_count: u64,
        attr: FileAttr,
        data: Vec<u8>,
    }

    impl FsCache {
        pub(super) fn new() -> Self {
            FsCache {
                counter: Counter::new(1024),
                cache: HashMap::new(),
                lookup_cache: HashMap::new(),
            }
        }

        pub(super) fn lookup(
            &mut self,
            name: &str,
            default: impl FnOnce() -> Option<NewFileEntry>,
        ) -> Option<&mut FsEntry> {
            if let Some(id) = self.lookup_cache.get(name) {
                self.cache.get_mut(id).map(FsEntry::acquire)
            } else {
                default().map(|new| {
                    let inode = self.counter.next();
                    let attrs = FileAttr {
                        ino: inode,
                        size: new.data.len() as u64,
                        blocks: new.data.len().div_ceil(BLOCK_SIZE) as _,
                        atime: new.date,
                        mtime: new.date,
                        ctime: new.date,
                        ..DEFAULT_FILE_ATTR
                    };
                    self.cache.insert(inode, FsEntry::new(attrs, new.data));
                    self.lookup_cache.insert(name.to_string(), inode);
                    self.cache.get_mut(&inode).unwrap().acquire()
                })
            }
        }

        pub(super) fn get(&mut self, inode: u64) -> Option<&mut FsEntry> {
            self.cache.get_mut(&inode).map(FsEntry::acquire)
        }

        pub(super) fn iter_entries(&self) -> impl Iterator<Item = (u64, FileType, &str)> {
            self.lookup_cache
                .iter()
                .map(|(name, ino)| (*ino, FileType::RegularFile, name.as_str()))
        }

        pub(super) fn forget(&mut self, inode: u64, nlookup: u64) {
            if let Some(entry) = self.cache.get_mut(&inode) {
                entry.release(nlookup);
                if entry.ref_count == 0 {
                    self.cache.remove(&inode);
                    self.lookup_cache.retain(|_, i| *i != inode);
                }
            }
        }
    }

    impl FsEntry {
        fn new(attr: FileAttr, data: Vec<u8>) -> Self {
            FsEntry {
                ref_count: 1,
                attr,
                data,
            }
        }

        fn acquire(&mut self) -> &mut Self {
            self.ref_count += 1;
            self
        }

        fn release(&mut self, nlookup: u64) {
            if let Some(count) = self.ref_count.checked_sub(nlookup) {
                self.ref_count = count;
            } else {
                log::error!(inode = self.attr.ino; "Ref count underflow");
            }
        }

        pub(super) fn attr(&self) -> &FileAttr {
            &self.attr
        }

        pub(super) fn content(&self) -> &[u8] {
            &self.data
        }
    }

    pub(super) struct NewFileEntry {
        pub(super) date: SystemTime,
        pub(super) data: Vec<u8>,
    }
}

const INODE_ROOT: u64 = 1;
const INODE_README: u64 = 2;
const INODE_MAGIC: u64 = 3;

// Explanation of ResolvConfFS basic functionality, and its magic.
//
// The basic function of this FUSE filesystem is to provide a (read-only)
// resolv.conf file for containers, which contains the DNS servers provided by
// the Wireguard config.
//
// The filesystem is intended to be the source of mount points for the volume
// plugin. Basically, the plugin will map a volume name like
// `wireguard-dns-<config_name>` to a file in the ResolvConfFS filesystem called
// `/<config_name>.conf`.
//
// The filesystem also provides a `/magic` file, which is meant to be mapped to
// a generic volume name like `wireguard-dns`, and will attempt to automatically
// look up the Wireguard config that is loaded for a given container.
// Specifically, it will look up the `wg0` interface within the container's
// network namespace. We currently use the `ifalias` attribute to add an
// identifier that we can map back to a config name. This magic is a convenience
// feature to avoid having to specify the config name explicitly.
//
// There is also a more hackish convenience feature that we call the "dockerd
// hack". It has a single purpose: prevent a common error when the volume is
// mounted without the `nocopy` option. It's literally just because we can.
// Normally, without the `nocopy` option, Docker will attempt to copy the
// contents of the container filesystem under the mount point to the volume.
// Which not only makes little sense, but will also fail with a confusing error
// message from Docker. To prevent this, we make the Docker daemon believe that
// the volume corresponds not to a single file, but to a directory with at least
// one file. This is implemented only for the `/magic` file; other files will
// require the more explicit configuration with the `nocopy` option.

struct ResolvConfFs<Wg> {
    service: Arc<NetworkPluginService<Wg>>,
    cache: FsCache,
}

impl<Wg> ResolvConfFs<Wg> {
    pub fn new(service: Arc<NetworkPluginService<Wg>>) -> Self {
        ResolvConfFs {
            service,
            cache: FsCache::new(),
        }
    }
}

impl<Wg> core::fmt::Debug for ResolvConfFs<Wg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResolvConfFs").finish()
    }
}

#[cfg(not(feature = "disable-dockerd-hack"))]
fn is_pid_docker_daemon(pid: u32) -> bool {
    // For the sake of the "dockerd hack", we try to guess whether the process
    // is dockerd. Ideally this check never happens in the most frequent pass
    // (container requests its own resolv.conf), and makes as few syscalls as
    // possible. False negatives are okay, but false positives are bad. Aside:
    // the pid is very likely actually a thread id, and that's fine, everything
    // works the same way.
    log::debug!(pid; "is_pid_docker_daemon?");
    // First hint: we cannot readlink `/proc/{pid}/ns/net`. We need to be able
    // to read it for our regular container processes, to do the magic file
    // thing. But on most setups, we can't read it for dockerd.
    let ns_net_path = format!("/proc/{pid}/ns/net");
    if std::fs::read_link(&ns_net_path).is_ok() {
        log::debug!(pid; "is_pid_docker_daemon? no, /proc/{pid}/ns/net is readable");
        return false;
    }
    // Second hint: the process self-identifies as "dockerd". While any process
    // could change their name to dockerd, I mean, it probably means they are
    // doing something weird. Until a real failure is actually shown, we can
    // probably work with this.
    let path = format!("/proc/{pid}/comm");
    let comm = match std::fs::read(&path) {
        Ok(exe_path) => exe_path,
        Err(err) => {
            log::debug!(pid, path = path.as_str(), err:?; "is_pid_docker_daemon: cannot read comm");
            return false;
        }
    };
    log::debug!(pid; "comm: {:?}", str::from_utf8(&comm));
    comm == b"dockerd\n"
}

#[cfg(feature = "disable-dockerd-hack")]
const fn is_pid_docker_daemon(_pid: u32) -> bool {
    false
}

impl<WgImpl: Wg> ResolvConfFs<WgImpl> {
    fn get_magic_file_attr(&self, pid: u32) -> FileAttr {
        let content = self.get_magic_file_content(pid);
        FileAttr {
            size: content.len() as u64,
            blocks: content.len().div_ceil(BLOCK_SIZE) as _,
            ..MAGIC_FILE_ATTR
        }
    }

    fn get_magic_file_content(&self, pid: u32) -> Vec<u8> {
        log::debug!(pid; "get_magic_file_content");
        let network_id_raw =
            std::fs::read_to_string(format!("/proc/{}/root/sys/class/net/wg0/ifalias", pid))
                .unwrap_or_default();
        let network_id = match <&NetworkId>::try_from(network_id_raw.as_str().trim_ascii_end()) {
            Ok(network_id) => network_id,
            Err(err) => {
                log::debug!(pid, network_id = network_id_raw.as_str(), err:?; "get_magic_file_content: invalid network id");
                return Vec::new();
            }
        };
        let Some(config) = self.service.lookup_config_by_network_id(network_id) else {
            log::debug!(pid, network_id = network_id.as_str(); "get_magic_file_content: network not found");
            return Vec::new();
        };
        log::debug!(pid, network_id = network_id.as_str(); "get_magic_file_content: network found");
        config.format_resolv_conf().into_bytes()
    }
}

impl<WgImpl: Wg> Filesystem for ResolvConfFs<WgImpl> {
    fn lookup(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        // We only deal with UTF-8 filenames
        let Some(name) = name.to_str() else {
            reply.error(ENOENT);
            return;
        };
        log::debug!(name; "lookup");
        // The only valid parent is the root directory (in all cases) and the
        // "magic" directory (only for the dockerd hack, so it is only expected
        // to occur in that case, and be a regular file in all other cases).
        match (parent, name) {
            (INODE_ROOT | INODE_MAGIC, "README") => {
                reply.entry(&TTL, &README_TXT_ATTR, 0);
            }
            (INODE_ROOT, "magic") => {
                if is_pid_docker_daemon(req.pid()) {
                    // dockerd hack: "magic" becomes a directory
                    let attr = FileAttr {
                        ino: INODE_MAGIC,
                        kind: FileType::Directory,
                        ..DEFAULT_FILE_ATTR
                    };
                    reply.entry(&Duration::from_nanos(1), &attr, 0);
                } else {
                    reply.entry(&TTL, &self.get_magic_file_attr(req.pid()), 0);
                }
            }
            (INODE_ROOT, _) => {
                if let Some(entry) = self.cache.lookup(name, || {
                    let name = name.strip_suffix(".resolv.conf")?;
                    let config = self.service.lookup_config(name)?;
                    let data = config.format_resolv_conf().into_bytes();
                    Some(NewFileEntry {
                        date: SystemTime::now(),
                        data,
                    })
                }) {
                    reply.entry(&TTL, entry.attr(), 0);
                } else {
                    reply.error(ENOENT);
                }
            }
            _ => {
                reply.error(ENOENT);
            }
        }
    }

    fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
        self.cache.forget(ino, nlookup);
    }

    fn getattr(&mut self, req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        match ino {
            INODE_ROOT => reply.attr(&TTL, &HELLO_DIR_ATTR),
            INODE_README => reply.attr(&TTL, &README_TXT_ATTR),
            INODE_MAGIC => reply.attr(&TTL, &self.get_magic_file_attr(req.pid())),
            _ => {
                if let Some(entry) = self.cache.get(ino) {
                    reply.attr(&TTL, entry.attr())
                } else {
                    reply.error(ENOENT)
                }
            }
        }
    }

    fn read(
        &mut self,
        req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        if ino == INODE_README {
            let data = README_TXT_CONTENT.as_bytes();
            let slice = subslice(data, offset as usize, size as usize);
            reply.data(slice);
        } else if ino == INODE_MAGIC {
            let pid = req.pid();
            let magic_content = self.get_magic_file_content(pid);
            let slice = subslice(&magic_content, offset as usize, size as usize);
            reply.data(slice);
        } else if let Some(entry) = self.cache.get(ino) {
            let data = entry.content();
            let slice = subslice(data, offset as usize, size as usize);
            reply.data(slice);
        } else {
            reply.error(ENOENT);
        };
    }

    fn readdir(
        &mut self,
        req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        log::debug!(req:?, ino, fh, offset; "readdir");
        if ino != INODE_ROOT && ino != INODE_MAGIC {
            reply.error(ENOENT);
            return;
        }

        let magic_type = if is_pid_docker_daemon(req.pid()) {
            FileType::Directory
        } else {
            FileType::RegularFile
        };

        let entries = [
            (ino, FileType::Directory, "."),
            (INODE_ROOT, FileType::Directory, ".."),
            (INODE_README, FileType::RegularFile, "README"),
            (INODE_MAGIC, magic_type, "magic"),
        ];

        let all_entries = entries.into_iter().chain(self.cache.iter_entries());

        let limit = if ino == INODE_ROOT {
            usize::MAX
        } else {
            // Only for the dockerd hack, `/magic` is a directory, which
            // contains one file. So we only list the first three entries
            // corresponding to ., .., and README.
            3
        };

        for (i, entry) in all_entries.take(limit).enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }
}

fn subslice<T>(s: &[T], offset: usize, size: usize) -> &[T] {
    let start = offset.min(s.len());
    let end = offset.saturating_add(size).min(s.len());
    &s[start..end]
}

#[derive(Debug)]
pub struct ResolvConfFsSession<WgImpl>
where
    WgImpl: Wg,
{
    session: fuser::Session<ResolvConfFs<WgImpl>>,
}

impl<WgImpl> ResolvConfFsSession<WgImpl>
where
    WgImpl: Wg + Send + Sync + 'static,
{
    pub fn new(
        service: Arc<NetworkPluginService<WgImpl>>,
        mountpoint: &Path,
    ) -> std::io::Result<Self> {
        let filesystem = ResolvConfFs::new(service);
        let session = fuser::Session::new(
            filesystem,
            mountpoint,
            &[
                MountOption::RO,
                MountOption::FSName("wireguarddns".to_string()),
                MountOption::AllowRoot,
            ],
        )?;
        Ok(Self { session })
    }

    pub fn new_panicking(service: Arc<NetworkPluginService<WgImpl>>, mountpoint: &Path) -> Self {
        let filesystem = ResolvConfFs::new(service);
        let session = fuser::Session::new(
            filesystem,
            mountpoint,
            &[
                MountOption::RO,
                MountOption::FSName("wireguarddns".to_string()),
                MountOption::AllowRoot,
            ],
        )
        .unwrap();
        Self { session }
    }

    pub fn run(&mut self) -> std::io::Result<()> {
        self.session.run()
    }

    pub fn spawn(self) -> std::io::Result<fuser::BackgroundSession> {
        self.session.spawn()
    }
}

pub fn spawn<WgImpl>(
    service: Arc<NetworkPluginService<WgImpl>>,
    mountpoint: &Path,
) -> std::io::Result<fuser::BackgroundSession>
where
    WgImpl: Wg + Send + Sync + 'static,
{
    std::fs::create_dir_all(mountpoint)?;
    let session = ResolvConfFsSession::new(service, mountpoint)?;
    session.spawn()
}
