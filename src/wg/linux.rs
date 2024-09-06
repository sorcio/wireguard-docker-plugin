use std::sync::{Arc, Mutex};

use rtnetlink::new_connection;
use thiserror::Error;
use tokio::task::JoinHandle;
use wireguard_uapi::WgSocket;

use super::{Config, WgError};

#[derive(Debug, Error)]
pub(super) enum WgErrorInner {
    #[error("A netlink request failed")]
    RequestFailed(#[from] rtnetlink::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("WireGuard connection error")]
    WgSocket(#[from] wireguard_uapi::err::ConnectError),
    #[error("error reading config: {0}")]
    ConfigParse(String),
    #[error("WireGuard device configuration error: {0}")]
    SetDevice(#[from] wireguard_uapi::err::SetDeviceError),
    #[error("aborted")]
    Aborted(#[from] tokio::task::JoinError),
}

pub(crate) struct Wg {
    #[expect(unused)]
    rt_task: JoinHandle<()>,
    rt: rtnetlink::Handle,
    wg_socket: Arc<Mutex<WgSocket>>,
}

impl Wg {
    pub(crate) fn new() -> Result<Self, WgError> {
        let (rt_connection, rt, _) = new_connection().map_err(WgErrorInner::from)?;
        let wg_socket = Arc::new(Mutex::new(WgSocket::connect().map_err(WgErrorInner::from)?));
        let rt_task = tokio::spawn(rt_connection);
        Ok(Self {
            rt_task,
            rt,
            wg_socket,
        })
    }

    pub(crate) async fn create_interface(&self, name: &str, config: Config) -> Result<(), WgError> {
        let if_name = format!("wg-docker-{name}");
        self.rt
            .link()
            .add()
            .wireguard(if_name.clone())
            .execute()
            .await
            .map_err(WgErrorInner::from)?;

        {
            let wg_socket = self.wg_socket.clone();
            tokio::task::spawn_blocking(move || {
                let mut wg_socket = wg_socket.lock().unwrap();
                let uapi_device = config_to_uapi_device(&if_name, &config);
                wg_socket.set_device(uapi_device)
            })
            .await
            .map_err(WgErrorInner::from)?
            .map_err(WgErrorInner::from)?;
        }
        Ok(())
    }
}

fn config_to_uapi_device<'a>(
    if_name: &'a str,
    config: &'a Config,
) -> wireguard_uapi::set::Device<'a> {
    let mut device =
        wireguard_uapi::set::Device::from_ifname(if_name).private_key(config.private_key.bytes());

    if let Some(port) = config.listen_port {
        device = device.listen_port(port);
    }

    if let Some(fw_mark) = config.fw_mark {
        device = device.fwmark(fw_mark);
    }

    device.peers.extend(config.peers.iter().map(|peer_config| {
        let mut peer = wireguard_uapi::set::Peer::from_public_key(peer_config.public_key.bytes());
        if let Some(psk) = &peer_config.preshared_key {
            peer = peer.preshared_key(psk.bytes());
        }
        if let Some(endpoint) = &peer_config.endpoint {
            peer = peer.endpoint(endpoint);
        }
        peer.allowed_ips
            .extend(
                peer_config
                    .allowed_ips
                    .iter()
                    .map(|ip| wireguard_uapi::set::AllowedIp {
                        ipaddr: ip.ip(),
                        cidr_mask: Some(ip.cidr()),
                    }),
            );
        if let Some(pk) = peer_config.persistent_keepalive {
            peer = peer.persistent_keepalive_interval(pk.get());
        }
        peer
    }));

    device
}
