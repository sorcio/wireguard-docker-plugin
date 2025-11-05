use std::{path::PathBuf, process::ExitCode, sync::Arc};

use crate::{http, logging, service, wg};

#[cfg(target_os = "linux")]
use crate::{dnsfs, netns};

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    tokio::select! {
        _ = sigterm.recv() => {
            log::info!("Received SIGTERM");
        }
        _ = sigint.recv() => {
            log::info!("Received SIGINT");
        }
    };
}

pub fn run() -> ExitCode {
    if logging::configure_logging().is_err() {
        return ExitCode::FAILURE;
    }
    #[cfg(target_os = "linux")]
    {
        let netns_options = netns::NetworkNamespaceOptions::from_env();
        if netns::enter_net_namespace(&netns_options).is_err() {
            return ExitCode::FAILURE;
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .worker_threads(1)
        .thread_name("worker")
        .build()
        .unwrap();
    match rt.block_on(async_main()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("Error: {:?}", e);
            ExitCode::FAILURE
        }
    }
}

async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket_path = "/run/docker/plugins/wireguard.sock";
    let db_path = PathBuf::from("wireguard_db");
    let conf_path = PathBuf::from("wireguard_conf");
    let config_provider = wg::ConfigProvider::new_file(conf_path);
    // TODO: make this work outside of managed Docker plugins (v2 plugins)
    let dnsfs_path = PathBuf::from("/mounts/dnsfs");

    let service = Arc::new(service::NetworkPluginService::new(
        db_path,
        dnsfs_path,
        config_provider,
    )?);

    let shutdown = std::pin::pin!(shutdown_signal());

    #[cfg(target_os = "linux")]
    let background_fs = dnsfs::spawn(service.clone(), std::path::Path::new("/mounts/dnsfs"))?;

    http::server(socket_path, service, shutdown).await?;

    #[cfg(target_os = "linux")]
    tokio::task::spawn_blocking(|| {
        background_fs.join();
    })
    .await?;

    if std::fs::remove_file(socket_path).is_ok() {
        log::info!("Removed socket file");
    }

    Ok(())
}
