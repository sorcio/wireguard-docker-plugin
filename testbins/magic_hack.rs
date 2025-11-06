//! This is invoked by tests/dnsfs.rs to test the dockerd hack.

#[cfg(target_os = "linux")]
fn main() {
    use container_helpers::ContainerOptions;

    let mount_source = std::env::args_os()
        .nth(1)
        .expect("expected mount source argument");

    let temp_dir = tempfile::TempDir::new().expect("failed to create temp dir");
    let root_path = temp_dir.path().to_owned();
    let mount_point = root_path.join("mnt");
    std::fs::create_dir(&mount_point).expect("failed to create mount point");
    unsafe {
        ContainerOptions::new()
            .user_ns()
            .mount_ns()
            .pid_ns()
            .net_ns()
            .setup_fn(|| {
                std::fs::write("/proc/self/setgroups", b"deny").expect("failed to write setgroups");
                std::fs::write("/proc/self/uid_map", b"").expect("failed to write uid_map");
                std::fs::write("/proc/self/gid_map", b"").expect("failed to write gid_map");
                rustix::mount::mount_bind(mount_source, &mount_point)?;
                Ok(())
            })
            // .map_user_to_root()
            .chroot(root_path)
            .enter_and_fork()
    }
    .unwrap();

    rustix::thread::set_name(c"dockerd").expect("failed to set thread name");
    let dir = match std::fs::read_dir("/mnt/magic") {
        Ok(dir) => dir,
        Err(err) => {
            panic!("failed to read directory /mnt/magic: {err:?}");
        }
    };
    for entry in dir.flatten() {
        println!("{}", entry.file_name().to_string_lossy());
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("Unsupported platform");
    std::process::ExitCode::FAILURE
}
