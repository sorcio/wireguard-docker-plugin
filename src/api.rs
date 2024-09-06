use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub(crate) struct NetworkId<'a>(&'a str);

impl AsRef<Path> for NetworkId<'_> {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub(crate) struct EndpointId<'a>(&'a str);

impl std::fmt::Display for EndpointId<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub(crate) struct SandboxKey<'a>(&'a str);

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct CreateNetworkRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: NetworkId<'a>,
    #[serde(default, rename = "IPv4Data")]
    pub(crate) ipv4_data: Vec<IpamDataV4<'a>>,
    #[serde(default, rename = "IPv6Data")]
    pub(crate) ipv6_data: Vec<IpamDataV6<'a>>,
    #[serde(default, borrow)]
    pub(crate) options: CreateNetworkOptions<'a>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct CreateNetworkOptions<'a> {
    #[serde(default, borrow, rename = "com.docker.network.generic")]
    pub(crate) generic: CreateNetworkGenericOptions<'a>,
    #[serde(default, rename = "com.docker.network.enable_ipv6")]
    pub(crate) enable_ipv6: Option<bool>,
    // Other options are ignored
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct CreateNetworkGenericOptions<'a> {
    #[serde(rename = "wireguard-config")]
    pub(crate) config: Option<&'a str>,
    // Other options are ignored
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct IpamDataV4<'a> {
    pub(crate) address_space: &'a str,
    pub(crate) gateway: &'a str,
    pub(crate) pool: &'a str,
    #[serde(default)]
    pub(crate) aux_addresses: HashMap<&'a str, &'a str>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct IpamDataV6<'a> {
    pub(crate) address_space: &'a str,
    pub(crate) gateway: &'a str,
    pub(crate) pool: &'a str,
    pub(crate) aux_addresses: HashMap<&'a str, &'a str>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct CreateEndpointRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: NetworkId<'a>,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: EndpointId<'a>,
    #[serde(borrow, default)]
    pub(crate) interface: Interface<'a>,
    #[serde(default)]
    pub(crate) options: CreateEndpointOptions,
}

#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct Interface<'a> {
    address: Option<&'a str>,
    #[serde(rename = "AddressIPv6")]
    address_ipv6: Option<&'a str>,
    mac_address: Option<&'a str>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct CreateEndpointOptions {
    // Options are ignored altogether for now
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct DeleteEndpointRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: NetworkId<'a>,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: EndpointId<'a>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct DeleteNetworkRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: NetworkId<'a>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct JoinRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: NetworkId<'a>,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: EndpointId<'a>,
    #[serde(borrow)]
    pub(crate) sandbox_key: SandboxKey<'a>,
    #[serde(default)]
    pub(crate) options: JoinOptions,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct JoinOptions {
    // Options are ignored altogether for now
}

#[derive(Serialize, Debug)]
pub(crate) struct ErrorResponse<'a> {
    pub(crate) err: &'a str,
}

impl<'a> ErrorResponse<'a> {
    pub(crate) fn new(err: &'a str) -> Self {
        Self { err }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_create_network_request() {
        let value = json!({
            "NetworkID":"ec22489c52c934f9f788cc99483deb35070eae17b7712e12e569f8a39e0b9a4b",
            "Options":{
                "com.docker.network.enable_ipv6":false,
                "com.docker.network.generic":{"wireguard-config":"foo-bar"}},
            "IPv4Data":[{"AddressSpace":"LocalDefault","Gateway":"172.23.0.1/16","Pool":"172.23.0.0/16"}],
            "IPv6Data":[]
        });
        let s = value.to_string();
        let req: CreateNetworkRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(
            req.network_id,
            NetworkId("ec22489c52c934f9f788cc99483deb35070eae17b7712e12e569f8a39e0b9a4b")
        );
        assert_eq!(req.ipv4_data.len(), 1);
        assert_eq!(req.ipv6_data.len(), 0);
        assert_eq!(req.options.enable_ipv6, Some(false));
        assert_eq!(req.options.generic.config, Some("foo-bar"));
    }
}
