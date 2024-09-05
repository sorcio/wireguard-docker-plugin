use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::NetworkId;

pub(crate) struct Db {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Network {
    config: String,
}

impl Db {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn create_network(
        &self,
        network_id: NetworkId,
        config: String,
    ) -> Result<(), std::io::Error> {
        let network = Network { config };
        let network = serde_json::to_string(&network)?;
        let path = self.path.join(network_id).with_extension("json");
        // TODO: locking
        std::fs::write(path, network)
    }
}

pub(crate) fn open<P: AsRef<Path>>(path: P) -> Result<Db, std::io::Error> {
    let path = path.as_ref();
    std::fs::create_dir_all(path)?;
    Ok(Db::new(path.to_owned()))
}
