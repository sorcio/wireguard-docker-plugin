#![cfg(target_os = "linux")]

mod common;
use std::{os::unix::fs::MetadataExt, path::Path};

use common::{config_content, expected_content, TestFuseMount};
use rustix::path::Arg;
use small_ctor::ctor;

use crate::common::{ensure_can_mount, TestServiceMock};

#[ctor]
unsafe fn init_before_any_test_is_run() {
    ensure_can_mount();
}

#[tokio::test]
async fn test_fuse_mount_succeeds() {
    let test_service = TestServiceMock::new().unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let mount_path = mount.path();
    assert!(mount_path.is_dir());
    // A common way (e.g. python stdlib is_mount() works this way) to test that
    // a directory is a mount point is to check that its device ID is different
    // from its parent's device ID.
    let mount_path_dev = mount_path.metadata().unwrap().dev();
    let parent_dev = mount_path.parent().unwrap().metadata().unwrap().dev();
    assert_ne!(
        mount_path_dev, parent_dev,
        "Mount path is not a mount point (st_dev is the same as parent)"
    );
}

#[tokio::test]
async fn test_fuse_readme_exists() {
    let test_service = TestServiceMock::new().unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();

    let readme_path = mount.readme_path();
    assert!(readme_path.exists(), "README file should exist");

    let content = std::fs::read_to_string(&readme_path).expect("Should be able to read README");

    assert!(content.contains("wireguard-docker-plugin"));
    assert!(content.contains("dnsfs"));
}

#[tokio::test]
async fn test_read_resolv_conf_single_dns() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("simple-dns", &config_content::simple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("simple-dns");
    assert!(resolv_path.exists(), "resolv.conf file should exist");
    let content =
        std::fs::read_to_string(&resolv_path).expect("Should be able to read resolv.conf");
    assert_eq!(content, expected_content::SIMPLE_DNS);
}

#[tokio::test]
async fn test_read_resolv_conf_multiple_dns() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("multiple-dns", &config_content::multiple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("multiple-dns");
    let content =
        std::fs::read_to_string(&resolv_path).expect("Should be able to read resolv.conf");
    assert_eq!(content, expected_content::MULTIPLE_DNS);
}

// Test: Read resolv.conf with no DNS servers (empty file)
#[tokio::test]
async fn test_read_resolv_conf_no_dns() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("no-dns", &config_content::no_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("no-dns");
    let content =
        std::fs::read_to_string(&resolv_path).expect("Should be able to read resolv.conf");
    assert_eq!(content, expected_content::NO_DNS);
    assert!(content.is_empty(), "Should be empty when no DNS configured");
}

#[tokio::test]
async fn test_read_resolv_conf_ipv6_dns() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("ipv6-dns", &config_content::ipv6_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("ipv6-dns");
    let content =
        std::fs::read_to_string(&resolv_path).expect("Should be able to read resolv.conf");
    assert_eq!(content, expected_content::IPV6_DNS);
    assert!(content.contains("2606:4700:4700::1111"));
}

// Test: Read resolv.conf with mixed IPv4 and IPv6 DNS
#[tokio::test]
async fn test_read_resolv_conf_mixed_dns() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("mixed-dns", &config_content::mixed_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("mixed-dns");
    let content =
        std::fs::read_to_string(&resolv_path).expect("Should be able to read resolv.conf");
    assert_eq!(content, expected_content::MIXED_DNS);
}

#[tokio::test]
async fn test_read_resolv_conf_nonexistent_config() {
    let test_service = TestServiceMock::new().unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("nonexistent");
    assert!(!resolv_path.exists(), "Nonexistent config should not exist");
    let err = std::fs::read(&resolv_path).expect_err("Should fail to read nonexistent config");
    assert!(err.kind() == std::io::ErrorKind::NotFound);
}

#[tokio::test]
async fn test_fuse_file_attributes() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("test-attrs", &config_content::simple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("test-attrs");
    let metadata = std::fs::metadata(&resolv_path).expect("Should be able to get file metadata");
    assert!(metadata.is_file());
    assert_eq!(metadata.len(), expected_content::SIMPLE_DNS.len() as u64);
    assert_eq!(metadata.mode() & 0o777, 0o644);
}

#[tokio::test]
async fn test_fuse_readdir() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("test1", &config_content::simple_dns())
        .unwrap();
    test_service
        .write_config("test2", &config_content::multiple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    // touch expected entries and keep them open to ensure they are cached
    let _ = std::fs::File::open(mount.resolv_conf_path("test1")).unwrap();
    let _ = std::fs::File::open(mount.resolv_conf_path("test2")).unwrap();
    // list directory contents
    let entries = std::fs::read_dir(mount.path()).expect("Should be able to read directory");
    let entry_names: Vec<String> = {
        let mut names: Vec<_> = entries
            .map(|e| {
                e.expect("Should be able to read directory entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        names.sort();
        names
    };
    assert_eq!(entry_names.len(), 4);
    assert_eq!(
        entry_names,
        vec!["README", "magic", "test1.resolv.conf", "test2.resolv.conf"]
    );
}

#[tokio::test]
async fn test_multiple_configs() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("config1", &config_content::simple_dns())
        .unwrap();
    test_service
        .write_config("config2", &config_content::multiple_dns())
        .unwrap();
    test_service
        .write_config("config3", &config_content::no_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let content1 = std::fs::read_to_string(mount.resolv_conf_path("config1")).unwrap();
    let content2 = std::fs::read_to_string(mount.resolv_conf_path("config2")).unwrap();
    let content3 = std::fs::read_to_string(mount.resolv_conf_path("config3")).unwrap();
    assert_eq!(content1, expected_content::SIMPLE_DNS);
    assert_eq!(content2, expected_content::MULTIPLE_DNS);
    assert_eq!(content3, expected_content::NO_DNS);
}

#[tokio::test]
async fn test_config_names_with_special_chars() {
    let test_service = TestServiceMock::new().unwrap();
    let valid_names = ["my-config", "my_config", "my.config", "config123"];
    for name in valid_names {
        test_service
            .write_config(name, &config_content::simple_dns())
            .unwrap();
    }
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    for name in valid_names {
        assert!(
            std::fs::read_to_string(mount.resolv_conf_path(name)).is_ok(),
            "Failed to read resolv.conf for config: {name}"
        );
    }
}

#[tokio::test]
async fn test_resolv_conf_format_correctness() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("format-test", &config_content::multiple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let content = std::fs::read_to_string(mount.resolv_conf_path("format-test")).unwrap();
    // Each line should be "nameserver <IP>\n"
    for line in content.lines().filter(|line| !line.is_empty()) {
        assert!(
            line.starts_with("nameserver "),
            "line should start with 'nameserver '"
        );
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "line should have format 'nameserver <IP>'");
        assert_eq!(parts[0], "nameserver");
        let _ip: std::net::IpAddr = parts[1]
            .parse()
            .unwrap_or_else(|_| panic!("Failed to parse IP address: {}", parts[1]));
    }
    assert!(content.ends_with('\n'), "Should end with newline");
}

#[tokio::test]
async fn test_resolv_conf_concurrent_reads() {
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config("test", &config_content::multiple_dns())
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let resolv_path = mount.resolv_conf_path("test");
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..10)
            .map(|_| {
                scope.spawn(|| {
                    std::fs::read_to_string(&resolv_path)
                        .expect("Should be able to read resolv.conf file")
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let first = &results[0];
    assert_eq!(first, expected_content::MULTIPLE_DNS);
    for result in &results[1..] {
        assert_eq!(result, first, "Concurrent reads should return same content");
    }
}

#[tokio::test]
async fn test_magic_file_exists() {
    let test_service = TestServiceMock::new().unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let magic_path = mount.magic_path();
    let contents =
        std::fs::read_to_string(&magic_path).expect("Magic file should exist and be readable");
    // We don't have a properly configured wg0 interface in our test environment
    // so the magic file should be empty.
    assert!(contents.is_empty(), "Magic file should be empty");
}

#[tokio::test]
#[cfg(not(feature = "disable-dockerd-hack"))]
async fn test_magic_file_type_from_test_process() {
    let output = do_test_magic_file_type_from_test_process().await;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "dockerd hack test command failed:\n--------------\n{stderr}\n--------------"
    );
    let output_str = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output_str, "README\n");
}

#[tokio::test]
#[cfg(feature = "disable-dockerd-hack")]
async fn test_magic_always_file_when_hack_disabled() {
    // Same conditions as test_magic_file_type_from_test_process but, when the
    // hack is disabled, the magic file should always be a regular file and not
    // a directory.
    let output = do_test_magic_file_type_from_test_process().await;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "subprocess should return an error when hack is disabled"
    );
    assert!(stderr.contains("Not a directory"), "{stderr}");
}

async fn do_test_magic_file_type_from_test_process() -> std::process::Output {
    use std::{
        io::{BufRead, BufReader},
        process::Command,
    };

    use crate::common::CommandExt;

    let fuse_temp_dir = tempfile::tempdir().expect("Failed to create tempdir");
    let fuse_path = fuse_temp_dir.path();

    // We want to run the fuse filesystem as a separate process, which will
    // execute in a separate user namespace, so to simulate the same kind of
    // permission mismatch real-world Docker would show.
    let fuse_process_path = env!("CARGO_BIN_EXE_isolated_dnsfs");
    let mut fuse_process = Command::new(fuse_process_path)
        .arg(fuse_path)
        .stdout(std::process::Stdio::piped())
        .spawn_with_guard()
        .expect("Failed to start isolated_dnsfs");

    // Wait for the fuse process to signal readiness
    let reader = BufReader::new(fuse_process.stdout.as_mut().unwrap());
    let line = reader.lines().next().unwrap().expect("Failed to read line");
    assert_eq!(line, "mount_ready");

    // The magic_hack binary will simply list the contents of the directory at
    // the given path (the `magic` entry in the fuse filesystem). It will
    // identify itself as "dockerd", so the entry must be a directory if the
    // hack is enabled, and a regular file if the hack is disabled.
    let subprocess_path = env!("CARGO_BIN_EXE_magic_hack");
    Command::new(subprocess_path)
        .arg(fuse_path)
        .output()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_magic_file_works() {
    use common::CommandExt;

    let fake_endpoint_id = "test_network_id";
    let fake_network_id = "hello_world_testing_ifalias";
    let fake_config_name = "test";
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config(fake_config_name, &config_content::multiple_dns())
        .unwrap();
    test_service
        .install_interface(fake_network_id, fake_endpoint_id, fake_config_name)
        .await
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let fuse_path = mount.path();
    // The subprocess will enter a new network namespace, configure a network
    // interface, and read the contents of the specified file (the magic file,
    // in this test).
    let subprocess_path = env!("CARGO_BIN_EXE_read_in_container");
    let exe_name = <_ as AsRef<Path>>::as_ref(subprocess_path)
        .file_name()
        .unwrap();
    let rootfs = tempfile::tempdir().expect("Failed to create temporary directory");
    let rootfs_path = rootfs.path();
    std::fs::copy(subprocess_path, rootfs_path.join(exe_name)).expect("Failed to copy executable");
    std::fs::create_dir(rootfs_path.join("mnt")).expect("Failed to create /mnt directory");

    let mut container = std::process::Command::new(rootfs_path.join(exe_name));
    container
        .arg(fuse_path)
        .arg(fake_network_id)
        .arg("/mnt/magic");

    let output = container.expect_output();
    assert_eq!(
        output.stdout.to_string_lossy(),
        expected_content::MULTIPLE_DNS
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_resolv_conf_from_container_works() {
    use common::CommandExt;

    let fake_endpoint_id = "test_network_id";
    let fake_network_id = "hello_world_testing_ifalias";
    let fake_config_name = "test";
    let test_service = TestServiceMock::new().unwrap();
    test_service
        .write_config(fake_config_name, &config_content::multiple_dns())
        .unwrap();
    test_service
        .install_interface(fake_network_id, fake_endpoint_id, fake_config_name)
        .await
        .unwrap();
    let mount = TestFuseMount::new(test_service.service()).unwrap();
    let fuse_path = mount.path();
    // The subprocess will enter a new network namespace, configure a network
    // interface, and read the contents of the specified file (the resolv.conf
    // file).
    let subprocess_path = env!("CARGO_BIN_EXE_read_in_container");
    let exe_name = <_ as AsRef<Path>>::as_ref(subprocess_path)
        .file_name()
        .unwrap();
    let rootfs = tempfile::tempdir().expect("Failed to create temporary directory");
    let rootfs_path = rootfs.path();
    std::fs::copy(subprocess_path, rootfs_path.join(exe_name)).expect("Failed to copy executable");
    std::fs::create_dir(rootfs_path.join("mnt")).expect("Failed to create /mnt directory");

    let mut container = std::process::Command::new(rootfs_path.join(exe_name));
    container
        .arg(fuse_path)
        .arg(fake_network_id)
        .arg("/mnt/test.resolv.conf");

    let output = container.expect_output();
    assert_eq!(
        output.stdout.to_string_lossy(),
        expected_content::MULTIPLE_DNS
    );
}
