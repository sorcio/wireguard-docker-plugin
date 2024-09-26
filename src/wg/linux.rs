use std::sync::{Arc, Mutex};

use futures_util::stream::StreamExt;
use rtnetlink::{
    new_connection,
    packet_core::NetlinkMessage,
    packet_route::{
        link::{LinkAttribute, LinkMessage},
        RouteNetlinkMessage,
    },
    LinkWireguard,
};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use wireguard_uapi::WgSocket;

use crate::types::EndpointId;

use super::{Config, WgError};

#[derive(Debug, Error)]
pub(super) enum WgErrorInner {
    #[error("rtnetlink error: {0}")]
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
    watcher: LinkWatcher,
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
            watcher: LinkWatcher::new()?,
        })
    }

    pub(crate) async fn create_interface(
        &self,
        endpoint_id: &EndpointId,
        config: Config,
    ) -> Result<String, WgError> {
        let if_name = Self::interface_name(endpoint_id);
        self.rt
            .link()
            .add(LinkWireguard::new(&if_name).build())
            .execute()
            .await
            .map_err(WgErrorInner::from)?;

        {
            let wg_socket = self.wg_socket.clone();
            let if_name = if_name.clone();
            tokio::task::spawn_blocking(move || {
                let mut wg_socket = wg_socket.lock().unwrap();
                let uapi_device = config_to_uapi_device(&if_name, &config);
                wg_socket.set_device(uapi_device)
            })
            .await
            .map_err(WgErrorInner::from)?
            .map_err(WgErrorInner::from)?;
        }
        Ok(if_name)
    }

    pub(crate) async fn delete_interface(&self, endpoint_id: &EndpointId) {
        let name = Self::interface_name(endpoint_id);
        if !delete_link_if_found(self.rt.clone(), name.clone())
            .await
            .unwrap_or(false)
        {
            self.watcher.mark_for_deletion(name).await;
        }
    }

    fn interface_name(endpoint_id: &EndpointId) -> String {
        let suffix = &endpoint_id.as_str()[0..8];
        format!("wgdkr{suffix}")
    }
}

async fn delete_link_if_found(
    handle: rtnetlink::Handle,
    name: String,
) -> Result<bool, rtnetlink::Error> {
    // rtnetlink crate does not have a delete link by name method
    // and the LinkDelMessage does not have a way to add a name
    // attribute (IFLA_IFNAME) so we need to construct the message
    // ourselves.

    let mut request = handle.link().del(0);
    request
        .message_mut()
        .attributes
        .push(LinkAttribute::IfName(name));

    const ENODEV: i32 = rustix::io::Errno::NODEV.raw_os_error();
    if let Err(err) = request.execute().await {
        if let rtnetlink::Error::NetlinkError(err) = &err {
            if err.raw_code() == ENODEV {
                return Ok(false);
            }
        }
        Err(err)
    } else {
        Ok(true)
    }

    // // old implementation
    // let mut links = handle.link().get().match_name(name).execute();
    // if let Some(link) = links.try_next().await? {
    //     handle.link().del(link.header.index).execute().await?;
    //     Ok(true)
    // } else {
    //     Ok(false)
    // }
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

const fn nl_mgrp(group: u32) -> u32 {
    if group > 31 {
        panic!("use netlink_sys::Socket::add_membership() for this group");
    }
    if group == 0 {
        0
    } else {
        1 << (group - 1)
    }
}

struct LinkWatcher {
    rt_task: JoinHandle<()>,
    #[expect(unused)]
    rt: rtnetlink::Handle,
    watcher_task: JoinHandle<()>,
    marked_for_deletion: Arc<AsyncMutex<Vec<String>>>,
}

impl LinkWatcher {
    pub(crate) fn new() -> Result<Self, WgError> {
        let (mut rt_connection, rt, mut messages) = new_connection().map_err(WgErrorInner::from)?;

        // use netlink_proto::sys::{AsyncSocket, SocketAddr};
        use rtnetlink::proto::sys::{AsyncSocket, SocketAddr};
        let groups = nl_mgrp(rtnetlink::constants::RTMGRP_LINK);
        let addr = SocketAddr::new(0, groups);
        rt_connection
            .socket_mut()
            .socket_mut()
            .bind(&addr)
            .map_err(WgErrorInner::from)?;
        let marked_for_deletion: Arc<AsyncMutex<Vec<String>>> = Default::default();

        let messages_task = tokio::spawn({
            let marked_for_deletion = marked_for_deletion.clone();
            let rt = rt.clone();
            async move {
                while let Some((message, _)) = messages.next().await {
                    Self::process_message(rt.clone(), marked_for_deletion.clone(), message).await;
                }
            }
        });

        let rt_task = tokio::spawn(rt_connection);

        Ok(Self {
            rt_task,
            rt,
            watcher_task: messages_task,
            marked_for_deletion,
        })
    }

    async fn mark_for_deletion(&self, name: String) {
        let mut list = self.marked_for_deletion.lock().await;
        if !list.contains(&name) {
            list.push(name);
        }
    }

    async fn process_message(
        rt: rtnetlink::Handle,
        marked_for_deletion: Arc<AsyncMutex<Vec<String>>>,
        message: NetlinkMessage<RouteNetlinkMessage>,
    ) {
        let rtnetlink::packet_core::NetlinkPayload::InnerMessage(payload) = message.payload else {
            return;
        };
        match payload {
            RouteNetlinkMessage::NewLink(link) => {
                let Some(name) = get_name_from_link(&link) else {
                    return;
                };
                let mut list = marked_for_deletion.lock().await;
                if let Some(pos) = list.iter().position(|n| n == name) {
                    if delete_link_if_found(rt.clone(), name.clone())
                        .await
                        .unwrap_or_else(|e| {
                            log::error!("Failed to delete link {}: {}", name, e);
                            false
                        })
                    {
                        list.remove(pos);
                    }
                }
            }
            RouteNetlinkMessage::DelLink(link) => {
                let Some(name) = get_name_from_link(&link) else {
                    return;
                };
                let mut list = marked_for_deletion.lock().await;
                if let Some(pos) = list.iter().position(|n| n == name) {
                    list.remove(pos);
                }
            }
            _ => {}
        }
    }
}

impl Drop for LinkWatcher {
    fn drop(&mut self) {
        // TODO: is this the right thing to do?
        self.rt_task.abort();
        self.watcher_task.abort();
    }
}

fn get_name_from_link(link: &LinkMessage) -> Option<&String> {
    link.attributes.iter().find_map(|attr| {
        if let LinkAttribute::IfName(name) = attr {
            Some(name)
        } else {
            None
        }
    })
}
