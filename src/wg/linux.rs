use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex},
};

use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;
use rtnetlink::{
    new_connection,
    packet_core::NetlinkMessage,
    packet_route::{
        link::{LinkAttribute, LinkMessage},
        route::{RouteAttribute, RouteMessage},
        RouteNetlinkMessage,
    },
    LinkWireguard,
};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use wireguard_uapi::WgSocket;

use crate::{types::EndpointId, wg::Wg};

use super::{Config, WgError};

/// Overhead for WireGuard packets, in bytes, over IPv4
const WG_MTU_OVERHEAD_IPV4: u32 = 60;
/// Overhead for WireGuard packets, in bytes, over IPv6 or mixed IPv4/IPv6
const WG_MTU_OVERHEAD_IPV6: u32 = 80;

#[derive(Debug, Error)]
pub(super) enum Error {
    #[error("rtnetlink error: {0}")]
    RequestFailed(#[from] rtnetlink::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("WireGuard connection error")]
    WgSocket(#[from] wireguard_uapi::err::ConnectError),
    #[error("WireGuard device configuration error: {0}")]
    SetDevice(#[from] wireguard_uapi::err::SetDeviceError),
    #[error("aborted")]
    Aborted(#[from] tokio::task::JoinError),
}

pub struct WgLinux {
    #[expect(unused)]
    rt_task: JoinHandle<()>,
    rt: rtnetlink::Handle,
    wg_socket: Arc<Mutex<WgSocket>>,
    watcher: LinkWatcher,
}

impl Wg for WgLinux {
    fn new() -> Result<Self, WgError> {
        let (rt_connection, rt, _) = new_connection().map_err(Error::from)?;
        let wg_socket = Arc::new(Mutex::new(WgSocket::connect().map_err(Error::from)?));
        let rt_task = tokio::spawn(rt_connection);
        Ok(Self {
            rt_task,
            rt,
            wg_socket,
            watcher: LinkWatcher::new()?,
        })
    }

    async fn create_interface(
        &self,
        endpoint_id: &EndpointId,
        config: Config,
        ifalias: &str,
    ) -> Result<String, WgError> {
        let if_name = Self::interface_name(endpoint_id);
        self.rt
            .link()
            .add(LinkWireguard::new(&if_name).build())
            .execute()
            .await
            .map_err(Error::from)?;

        {
            let config = config.clone();
            let wg_socket = self.wg_socket.clone();
            let if_name = if_name.clone();
            tokio::task::spawn_blocking(move || {
                let mut wg_socket = wg_socket.lock().unwrap();
                let uapi_device = config_to_uapi_device(&if_name, &config);
                wg_socket.set_device(uapi_device)
            })
            .await
            .map_err(Error::from)?
            .map_err(Error::from)?;
        }
        if let Some(mtu) = config.mtu {
            // If config specifies explicit MTU, we apply it directly
            set_mtu(self.rt.clone(), &if_name, mtu)
                .await
                .map_err(Error::from)?;
        } else {
            // If no explicit MTU is specified, we need to discover it
            discover_and_apply_mtu(self.rt.clone(), &if_name, &config).await;
        }
        set_ifalias(self.rt.clone(), &if_name, ifalias)
            .await
            .map_err(Error::from)?;
        Ok(if_name)
    }

    async fn delete_interface(&self, endpoint_id: &EndpointId) {
        let name = Self::interface_name(endpoint_id);
        if !delete_link_if_found(self.rt.clone(), name.clone())
            .await
            .unwrap_or(false)
        {
            self.watcher.mark_for_deletion(name).await;
        }
    }
}

impl WgLinux {
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
}

async fn set_ifalias(
    handle: rtnetlink::Handle,
    if_name: &str,
    alias: &str,
) -> Result<(), rtnetlink::Error> {
    let mut message = LinkMessage::default();
    message
        .attributes
        .push(LinkAttribute::IfName(if_name.to_string()));
    message
        .attributes
        .push(LinkAttribute::IfAlias(alias.to_string()));
    let request = handle.link().set(message);
    request.execute().await
}

async fn set_mtu(
    handle: rtnetlink::Handle,
    if_name: &str,
    mtu: u32,
) -> Result<(), rtnetlink::Error> {
    let mut message = LinkMessage::default();
    message
        .attributes
        .push(LinkAttribute::IfName(if_name.to_string()));
    message.attributes.push(LinkAttribute::Mtu(mtu));
    let request = handle.link().set(message);
    request.execute().await
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
        let (mut rt_connection, rt, mut messages) = new_connection().map_err(Error::from)?;

        use rtnetlink::proto::sys::{AsyncSocket, SocketAddr};
        let groups = nl_mgrp(rtnetlink::constants::RTMGRP_LINK);
        let addr = SocketAddr::new(0, groups);
        rt_connection
            .socket_mut()
            .socket_mut()
            .bind(&addr)
            .map_err(Error::from)?;
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

/// Query kernel for minimum route MTU to a specific destination
async fn query_route_mtu(
    handle: &rtnetlink::Handle,
    ip: std::net::IpAddr,
    prefix_len: u8,
) -> Option<u32> {
    let message = rtnetlink::RouteMessageBuilder::<std::net::IpAddr>::new()
        .destination_prefix(ip, prefix_len)
        .ok()?
        .build();

    let routes: Vec<RouteMessage> = handle
        .route()
        .get(message)
        .execute()
        .try_collect()
        .await
        .inspect_err(|e| log::debug!("Route query for {} failed: {}", ip, e))
        .ok()?;

    let mut min_mtu: Option<u32> = None;
    for route in &routes {
        if let Some(mtu) = get_route_mtu(handle, route).await {
            min_mtu = Some(min_mtu.map_or(mtu, |current| current.min(mtu)));
        }
    }
    min_mtu
}

async fn discover_and_apply_mtu(handle: rtnetlink::Handle, if_name: &str, config: &Config) {
    let endpoint_ips: Vec<IpAddr> = config
        .peers
        .iter()
        .filter_map(|peer| peer.endpoint.map(|ep| ep.ip()))
        .collect();
    let has_ipv6 = endpoint_ips.iter().any(IpAddr::is_ipv6);

    let overhead = if has_ipv6 {
        WG_MTU_OVERHEAD_IPV6
    } else {
        WG_MTU_OVERHEAD_IPV4
    };

    let mut best_mtu: Option<u32> = None;
    for ip in &endpoint_ips {
        let prefix_len = if ip.is_ipv4() {
            Ipv4Addr::BITS
        } else {
            Ipv6Addr::BITS
        } as u8;

        if let Some(mtu) = query_route_mtu(&handle, *ip, prefix_len).await {
            best_mtu = Some(best_mtu.map_or(mtu, |current| current.min(mtu)));
        }
    }

    // If no routes found, fall back to default routes
    if best_mtu.is_none() {
        for default_ip in [
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        ] {
            if let Some(mtu) = query_route_mtu(&handle, default_ip, 0).await {
                best_mtu = Some(best_mtu.map_or(mtu, |current| current.min(mtu)));
            }
        }
    }
    let base_mtu = best_mtu.unwrap_or(1500);
    let final_mtu = base_mtu.saturating_sub(overhead);
    if let Err(err) = set_mtu(handle, if_name, final_mtu).await {
        log::warn!(
            interface = if_name,
            mtu = final_mtu,
            error:? = err;
            "Failed to set auto-discovered MTU"
        );
    } else {
        log::debug!(
            interface = if_name,
            base_mtu = base_mtu,
            overhead = overhead,
            applied_mtu = final_mtu;
            "Applied auto-discovered MTU"
        );
    }
}

/// Extract MTU from a route, preferring route-specific MTU over interface MTU
async fn get_route_mtu(handle: &rtnetlink::Handle, route: &RouteMessage) -> Option<u32> {
    use rtnetlink::packet_route::route::RouteMetric;

    // First check for route-specific MTU in RTA_METRICS/RTAX_MTU
    for attr in &route.attributes {
        if let RouteAttribute::Metrics(metrics) = attr {
            for metric in metrics {
                if let RouteMetric::Mtu(mtu) = metric {
                    return Some(*mtu);
                }
            }
        }
    }

    // Fall back to output interface MTU
    let oif = route.attributes.iter().find_map(|attr| match attr {
        RouteAttribute::Oif(idx) => Some(*idx),
        _ => None,
    })?;

    get_link_mtu(handle, oif).await
}

/// Get MTU of a network interface by index
async fn get_link_mtu(handle: &rtnetlink::Handle, ifindex: u32) -> Option<u32> {
    let mut stream = handle.link().get().execute();
    while let Ok(Some(link)) = stream.try_next().await {
        if link.header.index == ifindex {
            return link.attributes.iter().find_map(|attr| match attr {
                LinkAttribute::Mtu(mtu) => Some(*mtu),
                _ => None,
            });
        }
    }
    None
}
