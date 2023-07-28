// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Errors returned by this library.

// NOTE: These errors are currently just toml::ser but in the future might need to return a
// toml_edit::ser::Error.

use std::{error, fmt};

/// An error serializing a summary to TOML.
#[derive(Debug, Clone)]
pub struct TomlSerializeError {
    error: SerializeErrorInner,
}

impl TomlSerializeError {
    pub(crate) fn new(error: impl Into<SerializeErrorInner>) -> Self {
        TomlSerializeError {
            error: error.into(),
        }
    }
}

impl fmt::Display for TomlSerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error serializing summary to TOML")
    }
}

impl error::Error for TomlSerializeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.error {
            SerializeErrorInner::Toml(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SerializeErrorInner {
    Toml(toml::ser::Error),
}

impl From<toml::ser::Error> for SerializeErrorInner {
    fn from(error: toml::ser::Error) -> Self {
        SerializeErrorInner::Toml(error)
    }
}

/// An error deserializing a summary from TOML.
#[derive(Debug, Clone)]
pub struct TomlDeserializeError {
    error: DeserializeErrorInner,
}

impl TomlDeserializeError {
    pub(crate) fn new(error: impl Into<DeserializeErrorInner>) -> Self {
        TomlDeserializeError {
            error: error.into(),
        }
    }
}

impl fmt::Display for TomlDeserializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error deserializing summary from TOML")
    }
}

impl error::Error for TomlDeserializeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.error {
            DeserializeErrorInner::Toml(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DeserializeErrorInner {
    Toml(toml::de::Error),
}

impl From<toml::de::Error> for DeserializeErrorInner {
    fn from(error: toml::de::Error) -> Self {
        DeserializeErrorInner::Toml(error)
    }
}
