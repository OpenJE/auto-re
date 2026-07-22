//! Observation payload parsing — JSON → [`ObservationEntity`] records.
//!
//! Accepted shapes:
//!
//! 1. A JSON array of entity objects.
//! 2. A JSON object with a single array-typed value (e.g. `{ "entities": [..] }`).
//! 3. A single entity object.

use std::collections::HashMap;

use autore_core::Result;

use super::IdentityError;

/// A single parsed entity from an `ObservationProduced` payload.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ObservationEntity {
    pub address_space: u32,
    pub entry_address: u64,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Anything that isn't `address_space` / `entry_address` /
    /// `display_name` is captured as provider-native extension metadata
    /// (IDA `ea`, `ida_function_row_uuid`, …).
    #[serde(flatten)]
    pub extension: HashMap<String, serde_json::Value>,
}

/// Parses an observation payload into one or more entity records.
pub fn parse_observation_payload(payload: &[u8]) -> Result<Vec<ObservationEntity>> {
    let v: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| IdentityError::InvalidPayload(e.to_string()))?;

    if let serde_json::Value::Array(arr) = &v {
        return arr
            .iter()
            .enumerate()
            .map(|(i, it)| {
                serde_json::from_value(it.clone())
                    .map_err(|e| IdentityError::InvalidPayload(format!("[{i}]: {e}")).into())
            })
            .collect();
    }

    if let serde_json::Value::Object(obj) = &v {
        for val in obj.values() {
            if let serde_json::Value::Array(arr) = val {
                return arr
                    .iter()
                    .enumerate()
                    .map(|(i, it)| {
                        serde_json::from_value(it.clone()).map_err(|e| {
                            IdentityError::InvalidPayload(format!("[{i}]: {e}")).into()
                        })
                    })
                    .collect();
            }
        }
    }

    let single: ObservationEntity =
        serde_json::from_value(v).map_err(|e| IdentityError::InvalidPayload(e.to_string()))?;
    Ok(vec![single])
}
