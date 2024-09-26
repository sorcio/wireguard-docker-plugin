use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::types::{ConfigName, NetworkId};

pub(crate) struct Db {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Network<'a> {
    config: Cow<'a, ConfigName>,
}

impl Network<'_> {
    pub(crate) fn config_name(&self) -> &ConfigName {
        self.config.as_ref()
    }
}

impl Db {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn network_path(&self, network_id: &NetworkId) -> PathBuf {
        self.path.join(network_id).with_extension("json")
    }

    pub(crate) fn create_network(
        &self,
        network_id: &NetworkId,
        config: &ConfigName,
    ) -> Result<(), std::io::Error> {
        let network = Network {
            config: Cow::Borrowed(config),
        };
        let network = serde_json::to_string(&network)?;
        let path = self.network_path(network_id);
        // TODO: locking
        std::fs::write(path, network)
    }

    pub(crate) fn delete_network(&self, network_id: &NetworkId) -> Result<(), std::io::Error> {
        let path = self.network_path(network_id);
        // TODO: locking
        std::fs::remove_file(path)
    }

    pub(crate) fn get_network(
        &self,
        network_id: &NetworkId,
    ) -> Result<Network<'static>, std::io::Error> {
        let path = self.network_path(network_id);
        let network = match std::fs::read_to_string(path) {
            Ok(network) => network,
            Err(err) => return Err(err),
        };
        let network = serde_json::from_str(&network)?;
        Ok(network)
    }
}

pub(crate) fn open<P: AsRef<Path>>(path: P) -> Result<Db, std::io::Error> {
    let path = path.as_ref();
    std::fs::create_dir_all(path)?;
    Ok(Db::new(path.to_owned()))
}
