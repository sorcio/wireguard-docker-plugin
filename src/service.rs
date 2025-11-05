use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    db::{open as db_open, Db},
    errors::Error,
    types::{ConfigName, EndpointId, NetworkId},
    wg::{CidrAddress, Config, ConfigProvider, Wg, WgDefault},
};
use std::convert::TryFrom;

pub struct NetworkPluginService<WgImpl = WgDefault> {
    db: Arc<Db>,
    wg: WgImpl,
    config_provider: ConfigProvider,
    dnsfs_path: PathBuf,
    rt: Option<tokio::runtime::Handle>,
}

impl<WgImpl: Wg> NetworkPluginService<WgImpl> {
    pub fn new(
        db_path: PathBuf,
        dnsfs_path: PathBuf,
        config_provider: ConfigProvider,
    ) -> Result<Self, std::io::Error> {
        let db = Arc::new(db_open(db_path)?);
        let wg = WgImpl::new().expect("Failed to create WireGuard client");
        std::fs::create_dir_all(&dnsfs_path)?;
        Ok(Self {
            db,
            wg,
            config_provider,
            dnsfs_path,
            rt: tokio::runtime::Handle::try_current().ok(),
        })
    }

    pub async fn create_network(&self, options: CreateNetworkOptions<'_>) -> Result<(), Error> {
        tokio::task::block_in_place(|| {
            self.db
                .create_network(options.network_id, options.config_name)
        })
        .map_err(Error::from)
    }

    pub async fn delete_network(&self, options: DeleteNetworkOptions<'_>) -> Result<(), Error> {
        tokio::task::block_in_place(|| self.db.delete_network(options.network_id))
            .map_err(Error::from)
    }

    pub async fn create_endpoint(
        &self,
        options: CreateEndpointOptions<'_>,
    ) -> Result<crate::wg::Config, Error> {
        let network = tokio::task::block_in_place(|| self.db.get_network(options.network_id))?;
        let config = self
            .config_provider
            .get_config(network.config_name())
            .await?;
        Ok(config)
    }

    pub async fn setup_container(
        &self,
        options: JoinOptions<'_>,
    ) -> Result<CreatedInterface, Error> {
        let network = tokio::task::block_in_place(|| self.db.get_network(options.network_id))?;
        let config = self
            .config_provider
            .get_config(network.config_name())
            .await?;
        let if_name = self
            .wg
            .create_interface(
                options.endpoint_id,
                config.clone(),
                options.network_id.as_str(),
            )
            .await?;
        let routes = config.routes().cloned().collect();
        Ok(CreatedInterface { if_name, routes })
    }

    pub async fn teardown_container(&self, options: LeaveOptions<'_>) -> Result<(), Error> {
        self.wg.delete_interface(options.endpoint_id).await;
        Ok(())
    }

    // Volume Plugin Methods

    pub async fn create_volume(&self, name: &str) -> Result<(), Error> {
        // This is actually a noop for us. If Docker wants to keep track of volumes,
        // it can do it for us, but we don't need to do anything here.
        let volume_name = parse_volume_name(name)?;
        log::info!(raw_volume_name = name, volume_name:?; "Created DNS volume");
        Ok(())
    }

    pub async fn remove_volume(&self, name: &str) -> Result<(), Error> {
        // See comment in create_volume().
        let volume_name = parse_volume_name(name)?;
        log::info!(raw_volume_name = name, volume_name:?; "Removed DNS volume");
        Ok(())
    }

    pub async fn get_volume_path(&self, name: &str) -> Result<PathBuf, Error> {
        if name == "wireguard-dns" {
            return Ok("/mounts/dnsfs/magic".into());
        }
        #[cfg(debug_assertions)]
        if name == "wireguard-debug-dns" {
            return Ok("/mounts/dnsfs/".into());
        }
        let volume_name = parse_volume_name(name)?;
        let volume_path = self.path_for_dns_config(volume_name);
        Ok(volume_path)
    }

    fn path_for_dns_config(&self, volume_name: VolumeName<'_>) -> PathBuf {
        match volume_name {
            VolumeName::Magic => self.dnsfs_path.join("magic"),
            VolumeName::Config(config_name) => self
                .dnsfs_path
                .join(config_name.as_ref())
                .with_added_extension("resolv.conf"),
            #[cfg(debug_assertions)]
            VolumeName::Debug => self.dnsfs_path.clone(),
        }
    }

    pub async fn list_volumes(&self) -> Result<Vec<(String, PathBuf)>, Error> {
        let mut volumes = vec![(
            VolumeName::Magic.to_volume_name_string(),
            self.path_for_dns_config(VolumeName::Magic),
        )];
        let mut entries = tokio::fs::read_dir(&self.dnsfs_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(config_name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".resolv.conf"))
                .and_then(|n| <&ConfigName>::try_from(n).ok())
            {
                let volume_name = VolumeName::Config(config_name);
                let volume_name_str = volume_name.to_volume_name_string();
                let mount_path = self.path_for_dns_config(volume_name);
                volumes.push((volume_name_str, mount_path));
            }
        }
        Ok(volumes)
    }

    // Methods used by ResolveConfFS (FUSE filesystem):

    pub fn set_tokio_runtime(&mut self, rt: tokio::runtime::Handle) {
        self.rt = Some(rt);
    }

    fn rt(&self) -> &tokio::runtime::Handle {
        self.rt
            .as_ref()
            .expect("Tokio runtime not set. Ensure set_tokio_runtime() is called")
    }

    pub fn lookup_config(&self, name: &str) -> Option<Config> {
        let config_name = <&ConfigName>::try_from(name).ok()?;
        self.rt()
            .block_on(async move { self.config_provider.get_config(config_name).await.ok() })
    }

    pub fn lookup_config_by_network_id(&self, network_id: &NetworkId) -> Option<Config> {
        let network = self.db.get_network(network_id).ok()?;
        log::debug!(network_id = network_id.as_str(); "lookup_config_by_network_id: found network");
        let config_name = network.config_name();
        log::debug!(network_id = network_id.as_str(), config = config_name.as_str(); "lookup_config_by_network_id: found config");
        self.rt()
            .block_on(async move { self.config_provider.get_config(config_name).await.ok() })
    }
}

const VOLUME_NAME_PREFIX: &str = "wireguard-dns";
const DEBUG_VOLUME_NAME: &str = "wireguard-dnsfs-debug";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VolumeName<'a> {
    Magic,
    Config(&'a ConfigName),
    #[cfg(debug_assertions)]
    Debug,
}

impl VolumeName<'_> {
    fn to_volume_name_string(self) -> String {
        match self {
            VolumeName::Magic => VOLUME_NAME_PREFIX.to_string(),
            VolumeName::Config(config_name) => {
                format!("{VOLUME_NAME_PREFIX}-{}", config_name.as_str())
            }
            #[cfg(debug_assertions)]
            VolumeName::Debug => DEBUG_VOLUME_NAME.to_string(),
        }
    }
}

fn parse_volume_name(volume_name: &str) -> Result<VolumeName<'_>, Error> {
    #[cfg(debug_assertions)]
    if volume_name == DEBUG_VOLUME_NAME {
        return Ok(VolumeName::Debug);
    }
    let stripped = volume_name
        .strip_prefix(VOLUME_NAME_PREFIX)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "Volume name must start with '{VOLUME_NAME_PREFIX}'"
            ))
        })?;
    if stripped.is_empty() {
        Ok(VolumeName::Magic)
    } else if let Some(config_name_str) = stripped.strip_prefix('-') {
        let config_name = <&ConfigName>::try_from(config_name_str)
            .map_err(|e| Error::InvalidInput(format!("Invalid config name: {}", e.0)))?;
        Ok(VolumeName::Config(config_name))
    } else {
        Err(Error::InvalidInput(format!(
            "Volume name must start with '{VOLUME_NAME_PREFIX}-'"
        )))
    }
}

#[derive(Debug)]
pub struct CreateNetworkOptions<'a> {
    pub network_id: &'a NetworkId,
    pub config_name: &'a ConfigName,
}

#[derive(Debug)]
pub struct DeleteNetworkOptions<'a> {
    pub network_id: &'a NetworkId,
}

#[derive(Debug)]
pub struct CreateEndpointOptions<'a> {
    pub network_id: &'a NetworkId,
    pub endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub struct JoinOptions<'a> {
    pub network_id: &'a NetworkId,
    pub endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub struct LeaveOptions<'a> {
    pub network_id: &'a NetworkId,
    pub endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub struct CreatedInterface {
    pub if_name: String,
    pub routes: Vec<CidrAddress>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_name_from_volume_valid() {
        assert_eq!(
            parse_volume_name("wireguard-dns-myconfig").unwrap(),
            VolumeName::Config("myconfig".try_into().unwrap())
        );
        assert_eq!(
            parse_volume_name("wireguard-dns-my-config").unwrap(),
            VolumeName::Config("my-config".try_into().unwrap())
        );
        assert_eq!(
            parse_volume_name("wireguard-dns-my_config").unwrap(),
            VolumeName::Config("my_config".try_into().unwrap())
        );
        assert_eq!(
            parse_volume_name("wireguard-dns-my.config").unwrap(),
            VolumeName::Config("my.config".try_into().unwrap())
        );
        assert_eq!(
            parse_volume_name("wireguard-dns-config123").unwrap(),
            VolumeName::Config("config123".try_into().unwrap())
        );
    }

    #[test]
    fn test_parse_volume_name_magic() {
        assert_eq!(
            parse_volume_name("wireguard-dns").unwrap(),
            VolumeName::Magic,
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_parse_volume_name_debug() {
        assert_eq!(
            parse_volume_name("wireguard-dnsfs-debug").unwrap(),
            VolumeName::Debug,
        );
    }

    #[test]
    fn test_parse_config_name_from_volume_starts_with_alphanumeric() {
        // First character must be alphanumeric
        assert!(parse_volume_name("wireguard-dns--invalid").is_err());
        assert!(parse_volume_name("wireguard-dns-_invalid").is_err());
        assert!(parse_volume_name("wireguard-dns-.invalid").is_err());
        // But these are valid
        assert!(parse_volume_name("wireguard-dns-a-config").is_ok());
        assert!(parse_volume_name("wireguard-dns-1config").is_ok());
    }

    #[test]
    fn test_parse_config_name_from_volume_invalid_prefix() {
        assert!(parse_volume_name("myconfig").is_err());
        assert!(parse_volume_name("wireguard-myconfig").is_err());
        assert!(parse_volume_name("dns-myconfig").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_empty_config() {
        assert!(parse_volume_name("wireguard-dns-").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_path_traversal() {
        assert!(parse_volume_name("wireguard-dns-../etc/passwd").is_err());
        assert!(parse_volume_name("wireguard-dns-foo/../bar").is_err());
        assert!(parse_volume_name("wireguard-dns-foo/bar").is_err());
        assert!(parse_volume_name("wireguard-dns-/absolute").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_invalid_chars() {
        assert!(parse_volume_name("wireguard-dns-foo bar").is_err());
        assert!(parse_volume_name("wireguard-dns-foo!bar").is_err());
        assert!(parse_volume_name("wireguard-dns-foo@bar").is_err());
        assert!(parse_volume_name("wireguard-dns-foo\\bar").is_err());
    }
}
