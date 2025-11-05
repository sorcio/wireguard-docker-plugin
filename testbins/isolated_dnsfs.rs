use std::sync::Arc;

use container_helpers::ContainerOptions;
use tempfile::TempDir;
use wireguard_docker_plugin::{
    dnsfs::ResolvConfFsSession,
    service::NetworkPluginService,
    wg::{mock::WgMock, ConfigProvider},
};

fn main() {
    let db_dir = TempDir::new().unwrap();
    let dnsfs_dir = TempDir::new().unwrap();
    let config_dir = TempDir::new().unwrap();

    let config_provider = ConfigProvider::new_file(config_dir.path().to_path_buf());

    let service = Arc::new(
        NetworkPluginService::<WgMock>::new(
            db_dir.path().to_path_buf(),
            dnsfs_dir.path().to_path_buf(),
            config_provider,
        )
        .unwrap(),
    );

    let mount_point = std::env::args_os()
        .nth(1)
        .expect("expected mount_point argument");
    let _ = std::fs::create_dir_all(&mount_point);
    let mut session = ResolvConfFsSession::new(service, mount_point.as_ref())
        .expect("failed to create fuse session");

    // We enter a new user namespace so that we can emulate running the service
    // as a separate container than the consumers of the FUSE file system.
    unsafe {
        ContainerOptions::new().user_ns().enter().unwrap();
    }

    // Signal the parent process that the mount is ready
    println!("mount_ready");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async move {
        // while this is a sync operation, the file system indirectly needs
        // tokio runtime to run
        tokio::task::spawn_blocking(move || {
            session.run().unwrap();
        })
        .await
        .unwrap();
    });
}
