use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{ConfigName, EndpointId, NetworkId};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub(crate) struct SandboxKey<'a>(&'a str);

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct CreateNetworkRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: &'a NetworkId,
    // #[serde(default, rename = "IPv4Data")]
    // pub(crate) ipv4_data: Vec<IpamDataV4<'a>>,
    // #[serde(default, rename = "IPv6Data")]
    // pub(crate) ipv6_data: Vec<IpamDataV6<'a>>,
    #[serde(default, borrow)]
    pub(crate) options: CreateNetworRequestkOptions<'a>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct CreateNetworRequestkOptions<'a> {
    #[serde(default, borrow, rename = "com.docker.network.generic")]
    pub(crate) generic: CreateNetworkRequestGenericOptions<'a>,
    #[serde(default, rename = "com.docker.network.enable_ipv6")]
    pub(crate) enable_ipv6: Option<bool>,
    // Other options are ignored
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(crate) struct CreateNetworkRequestGenericOptions<'a> {
    #[serde(rename = "wireguard-config", borrow)]
    pub(crate) config: Option<&'a ConfigName>,
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
    pub(crate) network_id: &'a NetworkId,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: &'a EndpointId,
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
    pub(crate) network_id: &'a NetworkId,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: &'a EndpointId,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct LeaveRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: &'a NetworkId,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: &'a EndpointId,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct DeleteNetworkRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: &'a NetworkId,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all(deserialize = "PascalCase"))]
pub(crate) struct JoinRequest<'a> {
    #[serde(borrow, rename = "NetworkID")]
    pub(crate) network_id: &'a NetworkId,
    #[serde(borrow, rename = "EndpointID")]
    pub(crate) endpoint_id: &'a EndpointId,
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

pub(crate) trait Validate {
    type Output;
    type Error;
    fn validate(&self) -> Result<Self::Output, Self::Error>;
}

impl<'a> Validate for CreateNetworkRequest<'a> {
    type Output = crate::service::CreateNetworkOptions<'a>;
    type Error = crate::errors::Error;

    fn validate(&self) -> Result<Self::Output, Self::Error> {
        let network_id = self.network_id;
        let mut missing_fields = vec![];
        if self.options.generic.config.is_none() {
            missing_fields.push("wireguard-config");
        }
        if !missing_fields.is_empty() {
            return Err(crate::errors::Error::MissingConfig(missing_fields));
        }
        let config_name = self.options.generic.config.unwrap();
        Ok(crate::service::CreateNetworkOptions {
            network_id,
            config_name,
        })
    }
}

impl<'a> Validate for DeleteNetworkRequest<'a> {
    type Output = crate::service::DeleteNetworkOptions<'a>;
    type Error = crate::errors::Error;

    fn validate(&self) -> Result<Self::Output, Self::Error> {
        Ok(crate::service::DeleteNetworkOptions {
            network_id: self.network_id,
        })
    }
}

impl<'a> Validate for CreateEndpointRequest<'a> {
    type Output = crate::service::CreateEndpointOptions<'a>;
    type Error = crate::errors::Error;

    fn validate(&self) -> Result<Self::Output, Self::Error> {
        Ok(crate::service::CreateEndpointOptions {
            network_id: self.network_id,
            endpoint_id: self.endpoint_id,
        })
    }
}

impl<'a> Validate for JoinRequest<'a> {
    type Output = crate::service::JoinOptions<'a>;
    type Error = crate::errors::Error;

    fn validate(&self) -> Result<Self::Output, Self::Error> {
        Ok(crate::service::JoinOptions {
            network_id: self.network_id,
            endpoint_id: self.endpoint_id,
        })
    }
}

impl<'a> Validate for LeaveRequest<'a> {
    type Output = crate::service::LeaveOptions<'a>;
    type Error = crate::errors::Error;

    fn validate(&self) -> Result<Self::Output, Self::Error> {
        Ok(crate::service::LeaveOptions {
            network_id: self.network_id,
            endpoint_id: self.endpoint_id,
        })
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
            req.network_id.as_str(),
            "ec22489c52c934f9f788cc99483deb35070eae17b7712e12e569f8a39e0b9a4b"
        );
        // assert_eq!(req.ipv4_data.len(), 1);
        // assert_eq!(req.ipv6_data.len(), 0);
        assert_eq!(req.options.enable_ipv6, Some(false));
        assert_eq!(
            req.options.generic.config.map(ConfigName::as_str),
            Some("foo-bar")
        );
    }
}
