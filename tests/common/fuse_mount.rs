use container_helpers::{ContainerOptions, Error as ContainerError};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use wireguard_docker_plugin::service::NetworkPluginService;

#[cfg(target_os = "linux")]
use wireguard_docker_plugin::dnsfs;
#[cfg(target_os = "linux")]
use wireguard_docker_plugin::wg::Wg;

/// Helper to mount FUSE filesystem for testing
///
/// This wrapper manages the lifecycle of a FUSE mount for tests:
/// - Creates a temporary mount point
/// - Spawns the FUSE filesystem
/// - Automatically unmounts on drop
#[cfg(target_os = "linux")]
pub struct TestFuseMount {
    mount_point: TempDir,
    _session: fuser::BackgroundSession,
}

#[cfg(target_os = "linux")]
impl TestFuseMount {
    /// Create a new FUSE mount with the given service
    pub fn new<WgImpl>(service: Arc<NetworkPluginService<WgImpl>>) -> std::io::Result<Self>
    where
        WgImpl: Wg + Send + Sync + 'static,
    {
        let mount_point = TempDir::new()?;
        let session = dnsfs::spawn(service, mount_point.path())?;
        Ok(Self {
            mount_point,
            _session: session,
        })
    }

    /// Get the path to the FUSE mount point
    pub fn path(&self) -> &Path {
        self.mount_point.path()
    }

    /// Get the path to a specific resolv.conf file
    pub fn resolv_conf_path(&self, config_name: &str) -> std::path::PathBuf {
        self.path().join(format!("{}.resolv.conf", config_name))
    }

    /// Get the path to the magic file
    pub fn magic_path(&self) -> std::path::PathBuf {
        self.path().join("magic")
    }

    /// Get the path to the README file
    pub fn readme_path(&self) -> std::path::PathBuf {
        self.path().join("README")
    }
}

static CAN_MOUNT: std::sync::OnceLock<Option<AllowedMount>> = std::sync::OnceLock::new();

pub fn ensure_can_mount() -> AllowedMount {
    if let Some(allowed_mount) =
        CAN_MOUNT.get_or_init(|| do_ensure_can_mount(PreferMount::Fusermount))
    {
        return *allowed_mount;
    }
    panic!(
        "This test needs to mount a FUSE filesystem. It requires \
        CAP_SYS_ADMIN, or fusermount, or a kernel supporting user \
        namespaces. None of these options are available."
    );
}

#[derive(Debug, PartialEq, Eq)]
enum PreferMount {
    Fusermount,
    NewUserNamespace,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum AllowedMount {
    SystemMount,
    Fusermount,
}

#[cfg(target_os = "linux")]
fn do_ensure_can_mount(prefer: PreferMount) -> Option<AllowedMount> {
    // If we don't have CAP_SYS_ADMIN, we have two ways to perform a FUSE mount anyway:
    //  - Use fusermount (the fuser crate will do so automatically if available)
    //  - Enter a new user namespace, a new mount namespace owned by the new user
    //    namespace, and mount the filesystem there.

    if is_cap_sys_admin_available() {
        eprintln!("ensure_can_mount: got CAP_SYS_ADMIN");
        return Some(AllowedMount::SystemMount);
    }
    if prefer == PreferMount::Fusermount && fusermount_bin_exists() {
        eprintln!("ensure_can_mount: using fusermount");
        return Some(AllowedMount::Fusermount);
    }
    if enter_new_namespaces()
        .inspect_err(|err| eprintln!("Cannot enter new namespaces: {err}"))
        .is_ok()
    {
        eprintln!("ensure_can_mount: entered new namespaces");
        return Some(AllowedMount::SystemMount);
    }
    if prefer == PreferMount::NewUserNamespace && fusermount_bin_exists() {
        eprintln!("ensure_can_mount: using fusermount");
        return Some(AllowedMount::Fusermount);
    }
    None
}

/// Enter a namespace where we can mount filesystems.
///
/// This will create a new user namespace and become root within it, and enter a
/// new mount namespace owned by the new user namespace.
///
/// The function is meant for integration tests, and will affect the entire
/// process. There is no safe way to exit the namespace once the test harness
/// has started.
fn enter_new_namespaces() -> Result<(), ContainerError> {
    unsafe {
        ContainerOptions::new()
            .user_ns()
            .mount_ns()
            .map_user_to_root()
            .enter()
    }
}

#[cfg(target_os = "linux")]
fn is_cap_sys_admin_available() -> bool {
    let caps = rustix::thread::capabilities(None).unwrap();
    caps.effective
        .contains(rustix::thread::CapabilitySet::SYS_ADMIN)
}

#[cfg(target_os = "linux")]
fn fusermount_bin_exists() -> bool {
    use std::os::unix::fs::PermissionsExt;
    const FUSERMOUNT_BIN: &str = "fusermount";
    const FUSERMOUNT3_BIN: &str = "fusermount3";
    [
        FUSERMOUNT3_BIN.to_string(),
        FUSERMOUNT_BIN.to_string(),
        format!("/bin/{FUSERMOUNT3_BIN}"),
        format!("/bin/{FUSERMOUNT_BIN}"),
    ]
    .iter()
    .any(|name| {
        // fuser crate approach is to actually call the binary like this:
        // `std::process::Command::new(name).arg("-h").output().is_ok()`
        // but we can be happy if the binary exists and looks executable:
        let path = std::path::Path::new(name);
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o6111 != 0)
            .unwrap_or(false)
    })
}

#[cfg(target_os = "linux")]
#[test]
fn test_can_mount() {
    // Test that we are able to ensure that we can mount, so that tests that
    // depend on that can run without errors.
    if ensure_can_mount() == AllowedMount::Fusermount {
        // If we have fusermount, we trust that it can mount for us
        return;
    }
    let temp_dir = TempDir::new().unwrap();
    // bind-mount the mount point to itself
    let mount_point = temp_dir.path();
    let result = rustix::mount::mount_bind(mount_point, mount_point);
    if result.is_ok() {
        rustix::mount::unmount(mount_point, rustix::mount::UnmountFlags::empty()).unwrap();
    }
    assert!(result.is_ok(), "Failed to mount: {}", result.unwrap_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn test_can_mount_fuse() {
    let mock_service = super::TestServiceMock::new().unwrap();
    TestFuseMount::new(mock_service.service()).unwrap();
}
