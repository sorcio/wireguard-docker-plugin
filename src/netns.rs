use std::path::{Path, PathBuf};

const DEFAULT_NETNS_PATH: &str = "/parent-netns";

#[derive(Debug, Clone)]
pub(crate) struct NetworkNamespaceOptions {
    skip_if_not_exists: bool,
    path: Option<PathBuf>,
}

impl NetworkNamespaceOptions {
    pub(crate) fn auto() -> Self {
        NetworkNamespaceOptions {
            skip_if_not_exists: true,
            path: Some(DEFAULT_NETNS_PATH.into()),
        }
    }

    pub(crate) fn auto_with_path<P: Into<PathBuf>>(path: P) -> Self {
        NetworkNamespaceOptions {
            skip_if_not_exists: true,
            path: Some(path.into()),
        }
    }

    pub(crate) fn inherit() -> Self {
        NetworkNamespaceOptions {
            skip_if_not_exists: false,
            path: None,
        }
    }

    pub(crate) fn from_path<P: Into<PathBuf>>(path: P) -> Self {
        NetworkNamespaceOptions {
            skip_if_not_exists: false,
            path: Some(path.into()),
        }
    }

    pub(crate) fn from_env() -> Self {
        use std::env;
        match env::var_os("NETNS") {
            Some(path) => {
                if path == "auto" {
                    if let Some(path) = env::var_os("NETNS_AUTO_PATH") {
                        NetworkNamespaceOptions::auto_with_path(path)
                    } else {
                        NetworkNamespaceOptions::auto()
                    }
                } else if path == "inherit" {
                    NetworkNamespaceOptions::inherit()
                } else {
                    NetworkNamespaceOptions::from_path(path)
                }
            }
            None => Default::default(),
        }
    }

    fn skip_if_not_exists(&self) -> bool {
        self.skip_if_not_exists
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Default for NetworkNamespaceOptions {
    fn default() -> Self {
        NetworkNamespaceOptions::auto()
    }
}

pub(crate) struct Error;

pub(crate) fn enter_net_namespace(options: &NetworkNamespaceOptions) -> Result<(), Error> {
    use rustix::thread::{move_into_link_name_space, LinkNameSpaceType};
    use std::os::fd::AsFd;

    let Some(netns_path) = options.path() else {
        log::debug!("No network namespace path provided. Inheriting namespace.");
        return Ok(());
    };
    let netns_file = match std::fs::File::open(netns_path) {
        Ok(file) => file,
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound if options.skip_if_not_exists() => {
                log::debug!(
                    path:display = netns_path.to_string_lossy();
                    "Network namespace file not found. Inheriting namespace."
                );
                return Ok(());
            }
            _ => {
                log::error!(
                    err:display,
                    path:display = netns_path.to_string_lossy();
                    "Could not open network namespace path."
                );
                return Err(Error);
            }
        },
    };

    move_into_link_name_space(netns_file.as_fd(), Some(LinkNameSpaceType::Network)).map_err(|err| {
        log::error!(
            err:display;
            "Failed to enter network namespace"
        );
        Error
    })
}
