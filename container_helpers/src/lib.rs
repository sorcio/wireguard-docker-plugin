//! Lightweight containerization of own process.
//!
//! This module provides a convenient interface to unshare namespaces and enter
//! some sort of containerized environment. Its purpose is to aid in integration
//! testing, especially tests that need CAP_SYS_ADMIN (we can provide it within
//! a user namespace, so the tests can run without root privileges) or tests that
//! need to exercise container-related functionality.
//!
//! This implementation is not intended for security and sandboxing purposes. In
//! fact, the kind of isolation provided by this module could be reverted and can
//! be escaped from. Do not use this module for security purposes.
//!
//! # Example
//!
//! This example demonstrates how to create a new container with a user namespace,
//! a PID namespace, a network namespace, and map the current user to root within
//! the container.
//!
//! ```standalone_crate
//! # use std::io;
//! use container_helpers::ContainerOptions;
//! let container_options = ContainerOptions::new()
//!     .user_ns()
//!     .pid_ns()
//!     .net_ns()
//!     .map_user_to_root();
//! unsafe { container_options.enter() }?;
//! # Ok::<(), container_helpers::Error>(())
//! ```
//!
//! Alternatively you can fork right after entering the namespaces. This is
//! useful for PID namespaces, where the PID of the child process will be 1.
//!
//! ```standalone_crate
//! # use std::io;
//! # use container_helpers::ContainerOptions;
//! # let container_options = ContainerOptions::new()
//! #     .user_ns()
//! #     .pid_ns()
//! #     .net_ns()
//! #     .map_user_to_root();
//! unsafe { container_options.enter_and_fork() }?;
//! # Ok::<(), container_helpers::Error>(())
//! ```
//!
//! When using ['ContainerOptions::chroot`] it can be useful to run some setup code
//! before chrooting, but within the newly created namespaces, e.g. to perform mount
//! operations needed for the chroot, or to use system tools that are not available
//! in the chroot filesystem.
//!
//! ```standalone_crate
//! # use std::io;
//! # use container_helpers::ContainerOptions;
//! # let root_path = std::env::temp_dir();
//! let container_options = ContainerOptions::new()
//!     .user_ns()
//!     .pid_ns()
//!     .mount_ns()
//!     .net_ns()
//!     .chroot(root_path)
//!     .setup_fn(|| {
//!         // Perform setup operations here
//!         Ok(())
//!     });
//! unsafe { container_options.enter_and_fork() }?;
//! # Ok::<(), container_helpers::Error>(())
//! ```
//!
//! # Safety
//!
//! Consider whether entering a new namespace and forking can violate some
//! invariants. That said, in most cases it should be safe to call
//! [`ContainerOptions::enter`] (or [`ContainerOptions::enter_and_fork`]) as the
//! very first thing in your process.

use std::path::PathBuf;

use libc_extra::fork;
use rustix::{
    process::{Pid, WaitOptions, waitpid},
    thread::UnshareFlags,
};
use thiserror::Error;

pub use libc_extra::{set_group, set_user};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed to create new user namespace: {0}")]
    UnshareUserNamespace(#[source] rustix::io::Errno),
    #[error("failed to update setgroups: {0}")]
    Setgroups(#[source] std::io::Error),
    #[error("failed to update uid_map: {0}")]
    UidMap(#[source] std::io::Error),
    #[error("failed to update gid_map: {0}")]
    GidMap(#[source] std::io::Error),
    #[error("failed to set user id: {0}")]
    SetUserId(#[source] std::io::Error),
    #[error("failed to set group id: {0}")]
    SetGroupId(#[source] std::io::Error),
    #[error("failed to unshare mount/net/pid namespaces: {0}")]
    UnshareMountNetPidNamespace(#[source] rustix::io::Errno),
    #[error("failed to run setup in container: {0}")]
    Setup(#[source] std::io::Error),
    #[error("failed to chroot: {0}")]
    Chroot(#[source] rustix::io::Errno),
    #[error("failed to create /proc: {0}")]
    CreateProcfs(#[source] std::io::Error),
    #[error("failed to create /sys: {0}")]
    CreateSysfs(#[source] std::io::Error),
    #[error("failed to mount procfs: {0}")]
    MountProcfs(#[source] rustix::io::Errno),
    #[error("failed to mount sysfs: {0}")]
    MountSysfs(#[source] rustix::io::Errno),
}

#[derive(Debug, Clone)]
#[must_use]
pub struct ContainerOptions<SetupFn> {
    user_ns: bool,
    mount_ns: bool,
    pid_ns: bool,
    net_ns: bool,
    fork: bool,
    map_user_to_root: bool,
    setup_fn: SetupFn,
    chroot: Option<PathBuf>,
}

impl ContainerOptions<()> {
    pub fn new() -> Self {
        Self {
            user_ns: false,
            mount_ns: false,
            pid_ns: false,
            net_ns: false,
            fork: false,
            map_user_to_root: false,
            setup_fn: (),
            chroot: None,
        }
    }
}

impl<T> ContainerOptions<T> {
    pub fn user_ns(mut self) -> Self {
        self.user_ns = true;
        self
    }

    pub fn map_user_to_root(mut self) -> Self {
        self.user_ns = true;
        self.map_user_to_root = true;
        self
    }

    pub fn mount_ns(mut self) -> Self {
        self.mount_ns = true;
        self
    }

    pub fn pid_ns(mut self) -> Self {
        self.pid_ns = true;
        self
    }

    pub fn net_ns(mut self) -> Self {
        self.net_ns = true;
        self
    }

    pub fn chroot(mut self, path: PathBuf) -> Self {
        self.chroot = Some(path);
        self
    }
}

impl ContainerOptions<()> {
    pub fn setup_fn<SetupFn>(self, setup_fn: SetupFn) -> ContainerOptions<SetupFn>
    where
        SetupFn: FnOnce() -> Result<(), std::io::Error>,
    {
        let ContainerOptions::<()> {
            user_ns,
            mount_ns,
            pid_ns,
            net_ns,
            fork,
            map_user_to_root,
            setup_fn: _,
            chroot,
        } = self;
        ContainerOptions {
            user_ns,
            mount_ns,
            pid_ns,
            net_ns,
            fork,
            map_user_to_root,
            setup_fn,
            chroot,
        }
    }
}

#[allow(private_bounds)]
impl<SetupFn> ContainerOptions<SetupFn>
where
    SetupFn: ContainerOptionsSetupFn,
{
    /// Enter a container with the given options.
    ///
    /// # Safety
    ///
    /// The process must not have multiple threads. Entering new namespaces
    /// might violate some invariants and invalidate some handles (file
    /// descriptors, etc) so just call this when you know it is safe.
    pub unsafe fn enter(self) -> Result<(), Error> {
        assert!(!self.fork);
        // SAFETY: we are not forking
        unsafe { enter_container(self) }
    }

    /// Enter a container with the given options, forking before the setup
    /// function is called, and continuing in the child process.
    ///
    /// The parent process will wait for the child process to exit and then
    /// exits. This means that execution will continue in the child process.
    /// From the point of view of the parent process, this function never
    /// returns.
    ///
    /// # Safety
    ///
    /// It must be safe to fork at this point. The process must not
    /// have multiple threads. Entering new namespaces might violate
    /// some invariants and invalidate some handles (file descriptors,
    /// etc) so just call this when you know it is safe.
    pub unsafe fn enter_and_fork(mut self) -> Result<(), Error> {
        self.fork = true;
        // SAFETY: the caller must ensure that it is safe to fork
        unsafe { enter_container(self) }
    }
}

trait ContainerOptionsSetupFn: Sized {
    fn do_setup(self) -> Result<(), std::io::Error>;
}

impl<SetupFn> ContainerOptionsSetupFn for SetupFn
where
    SetupFn: FnOnce() -> Result<(), std::io::Error>,
{
    fn do_setup(self) -> Result<(), std::io::Error> {
        (self)()
    }
}

impl ContainerOptionsSetupFn for () {
    fn do_setup(self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

impl Default for ContainerOptions<()> {
    fn default() -> Self {
        Self::new()
    }
}

/// Enter a container with the given options.
///
/// # Safety
///
/// In case options.fork is true, the caller must ensure that it is safe to fork
/// at this point.
unsafe fn enter_container<SetupFn>(options: ContainerOptions<SetupFn>) -> Result<(), Error>
where
    SetupFn: ContainerOptionsSetupFn,
{
    if options.user_ns {
        let parent_uid = rustix::process::getuid();
        let parent_gid = rustix::process::getgid();

        // SAFETY: we are not unsharing FILES
        unsafe { rustix::thread::unshare_unsafe(UnshareFlags::NEWUSER) }
            .map_err(Error::UnshareUserNamespace)?;

        if options.map_user_to_root {
            std::fs::write("/proc/self/setgroups", "deny").map_err(Error::Setgroups)?;
            std::fs::write("/proc/self/uid_map", format!("0 {parent_uid} 1"))
                .map_err(Error::UidMap)?;
            std::fs::write("/proc/self/gid_map", format!("0 {parent_gid} 1"))
                .map_err(Error::GidMap)?;
            unsafe {
                set_user(0).map_err(Error::SetUserId)?;
                set_group(0).map_err(Error::SetGroupId)?;
            }
        }
    }

    let mut flags: UnshareFlags = UnshareFlags::empty();
    if options.mount_ns {
        flags |= UnshareFlags::NEWNS;
    }
    if options.net_ns {
        flags |= UnshareFlags::NEWNET;
    }
    if options.pid_ns {
        flags |= UnshareFlags::NEWPID;
    }
    // SAFETY: we are not unsharing FILES
    unsafe { rustix::thread::unshare_unsafe(flags) }.map_err(Error::UnshareMountNetPidNamespace)?;

    if options.fork {
        // SAFETY: requirements must be upheld by caller
        unsafe {
            fork_and_continue_in_child_process();
        }
    }

    options.setup_fn.do_setup().map_err(Error::Setup)?;

    if let Some(root_path) = options.chroot {
        rustix::process::chroot(root_path).map_err(Error::Chroot)?;

        // Prepare chroot with needed mounts (only procfs and sysfs?)
        std::fs::create_dir_all("/proc").map_err(Error::CreateProcfs)?;
        std::fs::create_dir_all("/sys").map_err(Error::CreateSysfs)?;
        rustix::mount::mount(
            "",
            "/proc",
            "proc",
            rustix::mount::MountFlags::empty(),
            None,
        )
        .map_err(Error::MountProcfs)?;
        rustix::mount::mount(
            "",
            "/sys",
            "sysfs",
            rustix::mount::MountFlags::empty(),
            None,
        )
        .map_err(Error::MountSysfs)?;
    }

    Ok(())
}

/// Forks the current process and continues execution in the child process. The
/// parent process waits for the child process to finish, and then exits with the
/// same exit code as the child process.
///
/// # Safety
/// This caller must ensure that it is safe to fork at this point.
unsafe fn fork_and_continue_in_child_process() {
    // SAFETY: requirements upheld by caller
    let child_pid = unsafe { fork() };
    if let Some(child_pid) = Pid::from_raw(child_pid) {
        // The parent process waits for the child process to finish
        let exit = waitpid(Some(child_pid), WaitOptions::empty())
            .unwrap()
            .unwrap();
        std::process::exit(exit.1.exit_status().unwrap_or(1));
    }
}

mod libc_extra {
    use rustix::ffi::c_int;

    unsafe extern "C" {
        unsafe fn setresuid(ruid: c_int, euid: c_int, suid: c_int) -> c_int;
        unsafe fn setresgid(rgid: c_int, egid: c_int, sgid: c_int) -> c_int;
        pub(super) fn fork() -> c_int;
    }

    /// Set the real, effective, and saved user IDs of the current process.
    ///
    /// # Safety
    /// Caller must ensure that it is safe to set the user IDs.
    pub unsafe fn set_user(uid: c_int) -> std::io::Result<()> {
        let result = unsafe { setresuid(uid, uid, uid) };
        if result == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Set the real, effective, and saved group IDs of the current process.
    ///
    /// # Safety
    /// Caller must ensure that it is safe to set the group IDs.
    pub unsafe fn set_group(gid: c_int) -> std::io::Result<()> {
        let result = unsafe { setresgid(gid, gid, gid) };
        if result == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
