use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    db::{open as db_open, Db},
    errors::Error,
    types::{ConfigName, EndpointId, NetworkId},
    wg::{CidrAddress, ConfigProvider, Wg},
};
use std::convert::TryFrom;

pub(crate) struct NetworkPluginService {
    pub(crate) db: Arc<Db>,
    pub(crate) wg: Wg,
    pub(crate) config_provider: ConfigProvider,
    dns_base_path: PathBuf,
}

impl NetworkPluginService {
    pub(crate) fn new(
        db_path: impl AsRef<std::path::Path>,
        config_provider: ConfigProvider,
    ) -> Result<Self, std::io::Error> {
        let db_path = db_path.as_ref();
        let db = Arc::new(db_open(db_path)?);
        let wg = Wg::new().expect("Failed to create WireGuard client");
        let dns_base_path = db_path.join("dns");
        std::fs::create_dir_all(&dns_base_path)?;
        Ok(Self {
            db,
            wg,
            config_provider,
            dns_base_path,
        })
    }

    pub(crate) async fn create_network(
        &self,
        options: CreateNetworkOptions<'_>,
    ) -> Result<(), Error> {
        tokio::task::block_in_place(|| {
            self.db
                .create_network(options.network_id, options.config_name)
        })
        .map_err(Error::from)
    }

    pub(crate) async fn delete_network(
        &self,
        options: DeleteNetworkOptions<'_>,
    ) -> Result<(), Error> {
        tokio::task::block_in_place(|| self.db.delete_network(options.network_id))
            .map_err(Error::from)
    }

    pub(crate) async fn create_endpoint(
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

    pub(crate) async fn setup_container(
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
            .create_interface(options.endpoint_id, config.clone())
            .await?;
        let routes = config.routes().cloned().collect();

        // Update DNS resolv.conf if volume exists
        self.update_dns_for_network(network.config_name(), &config)
            .await?;

        Ok(CreatedInterface { if_name, routes })
    }

    pub(crate) async fn teardown_container(&self, options: LeaveOptions<'_>) -> Result<(), Error> {
        self.wg.delete_interface(options.endpoint_id).await;
        Ok(())
    }

    // Volume Plugin Methods

    pub(crate) async fn create_volume(&self, name: &str) -> Result<(), Error> {
        let config_name = parse_config_name_from_volume(name)?;
        let volume_path = self.dns_base_path.join(config_name.as_ref());

        tokio::fs::create_dir_all(&volume_path).await?;

        let resolv_conf_path = volume_path.join("resolv.conf");
        // Create empty resolv.conf file
        tokio::fs::write(&resolv_conf_path, b"").await?;

        log::info!(volume_name = name, path:? = volume_path; "Created DNS volume");
        Ok(())
    }

    pub(crate) async fn remove_volume(&self, name: &str) -> Result<(), Error> {
        let config_name = parse_config_name_from_volume(name)?;
        let volume_path = self.dns_base_path.join(config_name.as_ref());

        tokio::fs::remove_dir_all(&volume_path).await?;

        log::info!(volume_name = name, path:? = volume_path; "Removed DNS volume");
        Ok(())
    }

    pub(crate) fn get_volume_path(&self, name: &str) -> Result<PathBuf, Error> {
        let config_name = parse_config_name_from_volume(name)?;
        Ok(self.dns_base_path.join(config_name.as_ref()))
    }

    pub(crate) async fn list_volumes(&self) -> Result<Vec<(String, PathBuf)>, Error> {
        let mut volumes = Vec::new();

        let mut entries = tokio::fs::read_dir(&self.dns_base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Some(config_name) = path.file_name().and_then(|n| n.to_str()) {
                    let volume_name = format!("wireguard-dns-{}", config_name);
                    volumes.push((volume_name, path));
                }
            }
        }

        Ok(volumes)
    }

    async fn update_dns_for_network(
        &self,
        config_name: &ConfigName,
        config: &crate::wg::Config,
    ) -> Result<(), Error> {
        let volume_path = self.dns_base_path.join(config_name.as_ref());
        let resolv_conf_path = volume_path.join("resolv.conf");

        // Only update if the volume exists
        if tokio::fs::try_exists(&resolv_conf_path)
            .await
            .unwrap_or(false)
        {
            let content = config.format_resolv_conf();
            tokio::fs::write(&resolv_conf_path, content.as_bytes()).await?;
            log::debug!(config:? = config_name.as_ref(), dns_count = config.dns_servers().len(); "Updated resolv.conf for config");
        }

        Ok(())
    }
}

fn parse_config_name_from_volume(volume_name: &str) -> Result<&ConfigName, Error> {
    const PREFIX: &str = "wireguard-dns-";
    let config_name_str = volume_name
        .strip_prefix(PREFIX)
        .ok_or_else(|| Error::InvalidInput(format!("Volume name must start with '{}'", PREFIX)))?;

    // Check that first character is alphanumeric (not .- or _)
    if let Some(first_char) = config_name_str.chars().next() {
        if !first_char.is_ascii_alphanumeric() {
            return Err(Error::InvalidInput(
                "Config name must start with an alphanumeric character".to_string(),
            ));
        }
    }

    // Use ConfigName's TryFrom for validation (handles empty, invalid chars, etc.)
    <&ConfigName>::try_from(config_name_str)
        .map_err(|e| Error::InvalidInput(format!("Invalid config name: {}", e.0)))
}

#[derive(Debug)]
pub(crate) struct CreateNetworkOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    pub(crate) config_name: &'a ConfigName,
}

#[derive(Debug)]
pub(crate) struct DeleteNetworkOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
}

#[derive(Debug)]
pub(crate) struct CreateEndpointOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    #[expect(unused)]
    pub(crate) endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub(crate) struct JoinOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    pub(crate) endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub(crate) struct LeaveOptions<'a> {
    #[expect(unused)]
    pub(crate) network_id: &'a NetworkId,
    pub(crate) endpoint_id: &'a EndpointId,
}

#[derive(Debug)]
pub(crate) struct CreatedInterface {
    pub(crate) if_name: String,
    pub(crate) routes: Vec<CidrAddress>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_name_from_volume_valid() {
        assert_eq!(
            parse_config_name_from_volume("wireguard-dns-myconfig")
                .unwrap()
                .as_ref(),
            std::path::Path::new("myconfig")
        );
        assert_eq!(
            parse_config_name_from_volume("wireguard-dns-my-config")
                .unwrap()
                .as_ref(),
            std::path::Path::new("my-config")
        );
        assert_eq!(
            parse_config_name_from_volume("wireguard-dns-my_config")
                .unwrap()
                .as_ref(),
            std::path::Path::new("my_config")
        );
        assert_eq!(
            parse_config_name_from_volume("wireguard-dns-my.config")
                .unwrap()
                .as_ref(),
            std::path::Path::new("my.config")
        );
        assert_eq!(
            parse_config_name_from_volume("wireguard-dns-config123")
                .unwrap()
                .as_ref(),
            std::path::Path::new("config123")
        );
    }

    #[test]
    fn test_parse_config_name_from_volume_starts_with_alphanumeric() {
        // First character must be alphanumeric
        assert!(parse_config_name_from_volume("wireguard-dns--invalid").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-_invalid").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-.invalid").is_err());
        // But these are valid
        assert!(parse_config_name_from_volume("wireguard-dns-a-config").is_ok());
        assert!(parse_config_name_from_volume("wireguard-dns-1config").is_ok());
    }

    #[test]
    fn test_parse_config_name_from_volume_invalid_prefix() {
        assert!(parse_config_name_from_volume("myconfig").is_err());
        assert!(parse_config_name_from_volume("wireguard-myconfig").is_err());
        assert!(parse_config_name_from_volume("dns-myconfig").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_empty_config() {
        assert!(parse_config_name_from_volume("wireguard-dns-").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_path_traversal() {
        assert!(parse_config_name_from_volume("wireguard-dns-../etc/passwd").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-foo/../bar").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-foo/bar").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-/absolute").is_err());
    }

    #[test]
    fn test_parse_config_name_from_volume_invalid_chars() {
        assert!(parse_config_name_from_volume("wireguard-dns-foo bar").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-foo!bar").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-foo@bar").is_err());
        assert!(parse_config_name_from_volume("wireguard-dns-foo\\bar").is_err());
    }
}
