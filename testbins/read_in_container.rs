//! This is invoked by tests/dnsfs.rs to test the magic resolv.conf file.

use std::{io::Write, path::Path};

use container_helpers::ContainerOptions;
use rtnetlink::{
    new_connection,
    packet_route::link::{LinkAttribute, LinkMessage},
    LinkDummy,
};

fn main() {
    // validate arguments first so we can fail early
    let Ok(AppArgs {
        mount_source,
        ifalias,
        path,
    }) = AppArgs::from_env()
    else {
        usage();
    };

    let process_exe = std::env::current_exe().expect("failed to get process executable path");
    let root_path = process_exe
        .parent()
        .expect("failed to get root path")
        .to_owned();

    let container_options = ContainerOptions::new()
        .user_ns()
        .mount_ns()
        .net_ns()
        .pid_ns()
        .chroot(root_path.clone())
        .map_user_to_root()
        .setup_fn(|| container_setup(&root_path, &mount_source));
    if let Err(err) = unsafe { container_options.enter_and_fork() } {
        panic!("failed to enter container: {err}");
    };

    setup_network_interfaces_with_rtnetlink(ifalias);

    let content = std::fs::read(path).expect("failed to read file");
    std::io::stdout()
        .write_all(&content)
        .expect("failed to write to stdout");
}

struct AppArgsError;

struct AppArgs {
    mount_source: String,
    ifalias: String,
    path: String,
}

impl AppArgs {
    fn from_env() -> Result<Self, AppArgsError> {
        let mut args = std::env::args().skip(1);
        let mount_source = args.next().ok_or(AppArgsError)?;
        let ifalias = args.next().ok_or(AppArgsError)?;
        let path = args.next().ok_or(AppArgsError)?;
        Ok(Self {
            mount_source,
            ifalias,
            path,
        })
    }
}

fn usage() -> ! {
    let program_name = std::env::args().next().unwrap();
    eprintln!("Usage: {program_name} <MOUNT_SOURCE> <IFALIAS> <PATH>");
    std::process::exit(1)
}

async fn do_setup_network_interfaces_with_rtnetlink(
    ifalias: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (rt_connection, rt, _) = new_connection()?;
    let rt_task = tokio::spawn(rt_connection);
    eprintln!("Creating dummy interface wg0...");
    rt.link()
        .add(LinkDummy::new("wg0").build())
        .execute()
        .await?;
    eprintln!("Setting alias for wg0...");
    let mut message = LinkMessage::default();
    message
        .attributes
        .push(LinkAttribute::IfName("wg0".to_string()));
    message
        .attributes
        .push(LinkAttribute::IfAlias(ifalias.to_string()));
    let request = rt.link().set(message);
    request.execute().await.map_err(Box::new)?;
    eprintln!("Done setting up network interfaces.");
    rt_task.abort();
    Ok(())
}

fn setup_network_interfaces_with_rtnetlink(ifalias: String) {
    // We can't rely on `ip link` existing, so let's use netlink directly.
    // Equivalent to:
    //
    // ```
    // ip link add wg0 type dummy
    // ip link set wg0 alias wg0
    // ```
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .build()
        .expect("Failed to create tokio runtime");
    rt.block_on(
        async move { tokio::spawn(do_setup_network_interfaces_with_rtnetlink(ifalias)).await },
    )
    .unwrap()
    .unwrap();
}

fn container_setup(rootfs: &Path, mount_source: &str) -> std::io::Result<()> {
    rustix::mount::mount_bind(mount_source, rootfs.join("mnt"))?;
    Ok(())
}
