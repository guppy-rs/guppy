// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Serde helpers for types that need custom serialization.

/// Serialize/deserialize a `BTreeMap` with non-string keys as a
/// sequence of `(key, value)` pairs for JSON compatibility.
pub(crate) mod btree_map_as_seq {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S, K, V>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize + Ord,
        V: Serialize,
    {
        let entries: Vec<(&K, &V)> = map.iter().collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
    {
        let entries = Vec::<(K, V)>::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}
