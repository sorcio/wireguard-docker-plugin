use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Test fixture configurations
pub struct TestFixtures {
    /// Temporary directory for WireGuard configs
    pub config_dir: TempDir,
}

impl TestFixtures {
    /// Create a new test fixtures instance with a temporary config directory
    pub fn new() -> std::io::Result<Self> {
        let config_dir = TempDir::new()?;
        Ok(Self { config_dir })
    }

    /// Write a custom config file
    pub fn write_config(&self, name: &str, content: &str) -> std::io::Result<PathBuf> {
        let path = self.config_dir.path().join(format!("{}.conf", name));
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Get the path to the config directory
    pub fn config_path(&self) -> &Path {
        self.config_dir.path()
    }

    /// Get the path to a specific config file
    pub fn config_file_path(&self, name: &str) -> PathBuf {
        self.config_dir.path().join(format!("{}.conf", name))
    }
}

impl Default for TestFixtures {
    fn default() -> Self {
        Self::new().expect("Failed to create test fixtures")
    }
}

/// Expected resolv.conf content for different test configs
pub mod expected_content {
    pub const SIMPLE_DNS: &str = "nameserver 1.1.1.1\n";

    pub const MULTIPLE_DNS: &str = "nameserver 1.1.1.1\nnameserver 8.8.8.8\nnameserver 9.9.9.9\n";

    pub const IPV6_DNS: &str = "nameserver 2606:4700:4700::1111\n";

    pub const NO_DNS: &str = "";

    pub const MIXED_DNS: &str = "nameserver 1.1.1.1\nnameserver 2606:4700:4700::1111\n";
}

/// Sample config content generators
pub mod config_content {
    pub fn simple_dns() -> String {
        r#"[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.100.0.1/24
DNS = 1.1.1.1

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.0.2.1:51820
AllowedIPs = 0.0.0.0/0
"#
        .to_string()
    }

    pub fn multiple_dns() -> String {
        r#"[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.100.0.2/24
DNS = 1.1.1.1, 8.8.8.8, 9.9.9.9

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.0.2.1:51820
AllowedIPs = 0.0.0.0/0
"#
        .to_string()
    }

    pub fn no_dns() -> String {
        r#"[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.100.0.1/24

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.0.2.1:51820
AllowedIPs = 0.0.0.0/0
"#
        .to_string()
    }

    pub fn ipv6_dns() -> String {
        r#"[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.100.0.1/24
DNS = 2606:4700:4700::1111

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.0.2.1:51820
AllowedIPs = 0.0.0.0/0
"#
        .to_string()
    }

    pub fn mixed_dns() -> String {
        r#"[Interface]
PrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
Address = 10.100.0.1/24
DNS = 1.1.1.1, 2606:4700:4700::1111

[Peer]
PublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
Endpoint = 192.0.2.1:51820
AllowedIPs = 0.0.0.0/0
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixtures_creates_temp_dir() {
        let fixtures = TestFixtures::new().unwrap();
        assert!(fixtures.config_path().exists());
    }

    #[test]
    fn test_write_config() {
        let fixtures = TestFixtures::new().unwrap();
        let path = fixtures.write_config("test", "content").unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "content");
    }

    #[test]
    fn test_config_file_path() {
        let fixtures = TestFixtures::new().unwrap();
        let path = fixtures.config_file_path("myconfig");
        assert_eq!(path.file_name().unwrap(), "myconfig.conf");
    }
}
