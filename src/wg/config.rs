use std::{net::SocketAddr, num::NonZeroU16, path::PathBuf};

use super::{WgError, WgErrorInner};

#[derive(Debug, Clone)]
pub(crate) struct Key([u8; 32]);

impl From<Key> for [u8; 32] {
    fn from(key: Key) -> Self {
        key.0
    }
}

impl Key {
    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::str::FromStr for Key {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        use base64::prelude::*;
        let mut bytes = [0; 32];
        BASE64_STANDARD
            .decode_slice(s, &mut bytes)
            .map_err(|_| ())?;
        Ok(Self(bytes))
    }
}

pub(crate) struct Config {
    pub(super) private_key: Key,
    pub(super) listen_port: Option<u16>,
    pub(super) fw_mark: Option<u32>,
    pub(super) peers: Vec<Peer>,
}

pub(crate) struct Peer {
    pub(super) public_key: Key,
    pub(super) preshared_key: Option<Key>,
    pub(super) endpoint: Option<SocketAddr>,
    pub(super) allowed_ips: Vec<AllowedIp>,
    pub(super) persistent_keepalive: Option<NonZeroU16>,
}

pub(crate) struct AllowedIp {
    ip: std::net::IpAddr,
    cidr: u8,
}

impl AllowedIp {
    pub(crate) fn ip(&self) -> &std::net::IpAddr {
        &self.ip
    }

    pub(crate) fn cidr(&self) -> u8 {
        self.cidr
    }
}

impl std::str::FromStr for AllowedIp {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '/');
        let ip = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let cidr = if let Some(part) = parts.next() {
            part.parse().map_err(|_| ())?
        } else {
            match ip {
                std::net::IpAddr::V4(_) => std::net::Ipv4Addr::BITS as u8,
                std::net::IpAddr::V6(_) => std::net::Ipv6Addr::BITS as u8,
            }
        };
        Ok(Self { ip, cidr })
    }
}

async fn load_config_from_path(path: impl AsRef<std::path::Path>) -> Result<Config, WgError> {
    let text = tokio::fs::read_to_string(path.as_ref())
        .await
        .map_err(WgErrorInner::from)?;

    parse_config(&text)
}

fn parse_config(text: &str) -> Result<Config, WgError> {
    let parser = ini_core::Parser::new(text)
        .comment_char(b'#')
        .auto_trim(true);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Section {
        None,
        Interface,
        Peer,
    }

    let mut current_section = Section::None;

    let mut private_key = None;
    let mut listen_port = None;
    let mut fw_mark = None;
    let mut peers = Vec::new();
    let mut public_key = None;
    let mut preshared_key = None;
    let mut endpoint = None;
    let mut allowed_ips = Vec::new();
    let mut persistent_keepalive = None;
    let mut peer_section_line = 1;

    for (i, item) in parser.enumerate() {
        let line = i + 1;
        match item {
            ini_core::Item::Error(s) => {
                return Err(WgErrorInner::ConfigParse(format!("line {line}: {s}")).into());
            }
            ini_core::Item::Section(section_name) => match section_name {
                "Interface" => current_section = Section::Interface,
                "Peer" => {
                    peer_section_line = line;
                    current_section = Section::Peer;
                }
                _ => {
                    return Err(WgErrorInner::ConfigParse(format!(
                        "line {line}: unexpected section {section_name}"
                    ))
                    .into())
                }
            },
            ini_core::Item::SectionEnd => {
                if let Section::Peer = current_section {
                    peers.push(Peer {
                        public_key: public_key.ok_or_else(|| {
                            WgErrorInner::ConfigParse(format!(
                                "line {peer_section_line}: Peer section missing PublicKey"
                            ))
                        })?,
                        preshared_key,
                        endpoint,
                        allowed_ips,
                        persistent_keepalive,
                    });
                    public_key = None;
                    preshared_key = None;
                    endpoint = None;
                    allowed_ips = Vec::new();
                    persistent_keepalive = None;
                }
                current_section = Section::None;
            }
            ini_core::Item::Property(property, Some(value)) => match (current_section, property) {
                (Section::Interface, "PrivateKey") => {
                    let key: Key = value.parse().map_err(|_| {
                        WgErrorInner::ConfigParse(format!(
                            "line {line}: key should be a valid 256-bit base64 string"
                        ))
                    })?;
                    private_key = Some(key);
                }
                (Section::Interface, "ListenPort") => {
                    listen_port = if let Some(hex) = value.strip_prefix("0x") {
                        let port: u16 = u16::from_str_radix(hex, 16).map_err(|_| {
                            WgErrorInner::ConfigParse(format!(
                                "line {line}: ListenPort should be a valid port number"
                            ))
                        })?;
                        Some(port)
                    } else if value == "off" {
                        None
                    } else {
                        let port: u16 = value.parse().map_err(|_| {
                            WgErrorInner::ConfigParse(format!(
                                "line {line}: ListenPort should be a valid port number"
                            ))
                        })?;
                        Some(port)
                    };
                }
                (Section::Interface, "FwMark") => {
                    let mark: u32 = value.parse().map_err(|_| {
                        WgErrorInner::ConfigParse(format!(
                            "line {line}: FwMark should be a valid integer"
                        ))
                    })?;
                    fw_mark = Some(mark);
                }
                (Section::Peer, "PublicKey") => {
                    let key: Key = value.parse().map_err(|_| {
                        WgErrorInner::ConfigParse(format!(
                            "line {line}: PublicKey should be a valid 256-bit base64 string"
                        ))
                    })?;
                    public_key = Some(key);
                }
                (Section::Peer, "PresharedKey") => {
                    let key: Key = value.parse().map_err(|_| {
                        WgErrorInner::ConfigParse(format!(
                            "line {line}: PresharedKey should be a valid 256-bit base64 string"
                        ))
                    })?;
                    preshared_key = Some(key);
                }
                (Section::Peer, "Endpoint") => {
                    endpoint = Some(value.parse().map_err(|_| {
                        WgErrorInner::ConfigParse(format!(
                            "line {line}: Endpoint should be a valid address:port string"
                        ))
                    })?);
                }
                (Section::Peer, "AllowedIPs") => {
                    allowed_ips.extend(
                        value
                            .split(',')
                            .map(|s| {
                                s.parse().map_err(|_| {
                                    WgErrorInner::ConfigParse(format!(
                                        "line {line}: AllowedIPs should be a valid CIDR string"
                                    ))
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                (Section::Peer, "PersistentKeepalive") => {
                    persistent_keepalive = if let Ok(interval) = value.parse() {
                        NonZeroU16::new(interval)
                    } else if value == "off" {
                        None
                    } else {
                        return Err(WgErrorInner::ConfigParse(format!(
                            "line {line}: PersistentKeepalive should be a valid integer"
                        ))
                        .into());
                    };
                }
                (_, _) => {
                    return Err(WgErrorInner::ConfigParse(format!(
                        "line {line}: unexpected property {property}"
                    ))
                    .into())
                }
            },
            ini_core::Item::Property(property, None) => {
                return Err(WgErrorInner::ConfigParse(format!("line {line}: {property}")).into())
            }
            ini_core::Item::Comment(_) => {}
            ini_core::Item::Blank => {}
        }
    }

    Ok(Config {
        private_key: private_key
            .ok_or_else(|| WgErrorInner::ConfigParse("PrivateKey is required".to_string()))?,
        listen_port,
        fw_mark,
        peers,
    })
}

pub(crate) struct ConfigProvider {
    inner: ConfigProviderInner,
}

impl ConfigProvider {
    pub fn new_file(base_path: PathBuf) -> Self {
        Self {
            inner: ConfigProviderInner::File { base_path },
        }
    }

    pub async fn get_config(&self, name: &str) -> Result<Config, WgError> {
        match &self.inner {
            ConfigProviderInner::File { base_path } => {
                let path = base_path.join(name).with_extension("conf");
                load_config_from_path(path).await
            }
        }
    }
}

enum ConfigProviderInner {
    File { base_path: PathBuf },
}
