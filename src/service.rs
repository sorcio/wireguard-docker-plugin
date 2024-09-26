use std::sync::Arc;

use crate::{
    db::{open as db_open, Db},
    errors::Error,
    types::{ConfigName, EndpointId, NetworkId},
    wg::{CidrAddress, ConfigProvider, Wg},
};

pub(crate) struct NetworkPluginService {
    pub(crate) db: Arc<Db>,
    pub(crate) wg: Wg,
    pub(crate) config_provider: ConfigProvider,
}

impl NetworkPluginService {
    pub(crate) fn new(
        db_path: impl AsRef<std::path::Path>,
        config_provider: ConfigProvider,
    ) -> Result<Self, std::io::Error> {
        let db = Arc::new(db_open(db_path)?);
        let wg = Wg::new().expect("Failed to create WireGuard client");
        Ok(Self {
            db,
            wg,
            config_provider,
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
        Ok(CreatedInterface { if_name, routes })
    }

    pub(crate) async fn teardown_container(&self, options: LeaveOptions<'_>) -> Result<(), Error> {
        self.wg.delete_interface(options.endpoint_id).await;
        Ok(())
    }
}

pub(crate) struct CreateNetworkOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    pub(crate) config_name: &'a ConfigName,
}

pub(crate) struct DeleteNetworkOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
}

pub(crate) struct CreateEndpointOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    #[expect(unused)]
    pub(crate) endpoint_id: &'a EndpointId,
}

pub(crate) struct JoinOptions<'a> {
    pub(crate) network_id: &'a NetworkId,
    pub(crate) endpoint_id: &'a EndpointId,
}

pub(crate) struct LeaveOptions<'a> {
    #[expect(unused)]
    pub(crate) network_id: &'a NetworkId,
    pub(crate) endpoint_id: &'a EndpointId,
}

pub(crate) struct CreatedInterface {
    pub(crate) if_name: String,
    pub(crate) routes: Vec<CidrAddress>,
}
