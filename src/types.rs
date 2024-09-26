use std::path::Path;

/// A generic identifier that is safe to use as a path component.
///
/// The identifier must be non-empty and consist only of [a-zA-Z0-9_.-]
/// characters.
macro_rules! identifier_newtype {
    ($vis:vis &$ref_name:ident, $owned_name:ident) => {
        #[derive(Debug, ::serde::Serialize, PartialEq, Eq, Hash)]
        #[serde(transparent)]
        #[repr(transparent)]
        $vis struct $ref_name(str);

        #[derive(Debug, ::serde::Serialize, Clone, PartialEq, Eq, Hash)]
        #[serde(transparent)]
        $vis struct $owned_name(Box<str>);

        impl<'de: 'a, 'a> ::serde::Deserialize<'de> for &'a $ref_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let s: &str = ::serde::Deserialize::deserialize(deserializer)?;
                TryFrom::try_from(s).map_err(::serde::de::Error::custom)
            }
        }

        impl<'de: 'a, 'a> ::serde::Deserialize<'de> for $owned_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let s: &str = ::serde::Deserialize::deserialize(deserializer)?;
                Ok(<&$ref_name as TryFrom<&str>>::try_from(s)
                    .map_err(::serde::de::Error::custom)?
                    .to_owned())
            }
        }

        impl<'a> TryFrom<&'a str> for &'a $ref_name {
            type Error = $crate::errors::ValidationError;
            fn try_from(value: &'a str) -> Result<Self, Self::Error> {
                if value.is_empty() {
                    Err($crate::errors::ValidationError("Identifier cannot be empty"))
                } else if value
                    .chars()
                    .any(|c| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '_')
                {
                    Err($crate::errors::ValidationError(
                        "Identifier must consist only of [a-zA-Z0-9_.-] characters",
                    ))
                } else {
                    // SAFETY: $ref_name is a repr(transparent) on str
                    let new_ref = unsafe { std::mem::transmute::<&str, &$ref_name>(value) };
                    Ok(new_ref)
                }
            }
        }

        impl core::borrow::Borrow<$ref_name> for $owned_name {
            fn borrow(&self) -> &$ref_name {
                // SAFETY: $ref_name is a repr(transparent) on str
                unsafe { std::mem::transmute::<&str, &$ref_name>(self.0.borrow()) }
            }
        }

        impl ToOwned for $ref_name {
            type Owned = $owned_name;
            fn to_owned(&self) -> Self::Owned {
                $owned_name(self.0.to_owned().into_boxed_str())
            }
        }
    };
}

/////////////////

identifier_newtype!(pub(crate) &NetworkId, NetworkIdOwned);

#[cfg(test)]
impl NetworkId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for NetworkId {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

identifier_newtype!(pub(crate) &EndpointId, EndpointIdOwned);

impl EndpointId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

identifier_newtype!(pub(crate) &ConfigName, ConfigNameOwned);

#[cfg(test)]
impl ConfigName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for ConfigName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

#[cfg(test)]
mod identifier_tests {
    use serde::{Deserialize, Serialize};

    mod inner {
        identifier_newtype!(pub(super) &TestId, TestIdOwned);

        impl TestId {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    }

    use inner::{TestId, TestIdOwned};

    /// Test that the newtype can be serialized and deserialized.
    #[derive(Debug, Serialize, Deserialize)]
    struct TestSerde<'a> {
        #[serde(borrow)]
        value: &'a TestId,
    }

    /// Test that the newtype can be serialized and deserialized as owned.
    #[derive(Debug, Serialize, Deserialize)]
    struct TestSerdeOwned(TestIdOwned);

    /// Test that the newtype can be serialized and deserialized with Cow.
    #[derive(Debug, Serialize, Deserialize)]
    struct TestSerdeCow<'a>(std::borrow::Cow<'a, TestId>);

    #[test]
    fn dont_accept_empty() {
        assert!(<&TestId>::try_from("").is_err());
    }

    #[test]
    fn dont_accept_invalid_chars() {
        assert!(<&TestId>::try_from("test!").is_err());
        assert!(<&TestId>::try_from(" ").is_err());
        assert!(<&TestId>::try_from("foo bar").is_err());
        assert!(<&TestId>::try_from("foo/bar").is_err());
        assert!(<&TestId>::try_from("/foo").is_err());
        assert!(<&TestId>::try_from("foo\\bar").is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let id = <&TestId>::try_from("test").unwrap();
        let ser = serde_json::to_string(&TestSerde { value: id }).unwrap();
        assert_eq!(ser, r#"{"value":"test"}"#);
        let de: TestSerde = serde_json::from_str(&ser).unwrap();
        assert_eq!(de.value.as_str(), "test");
    }
}
