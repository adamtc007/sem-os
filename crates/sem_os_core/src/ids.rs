//! Deterministic identity generation for Semantic Registry objects.
//!
//! Uses UUID v5 (SHA-1 based, deterministic) to ensure the same
//! `(object_type, fqn)` pair always produces the same `object_id`,
//! regardless of which machine or scan run produces it.

use std::collections::BTreeMap;
use uuid::Uuid;

use crate::types::ObjectType;

/// UUID v5 namespace for Semantic Registry object IDs.
///
/// Generated once, never changed. All SemReg object IDs derive from this namespace.
/// Value: UUID v5 of "semantic-os:ob-poc:sem_reg" under the DNS namespace.
const SEM_REG_NAMESPACE: Uuid = Uuid::from_bytes([
    0x7a, 0x3b, 0x9f, 0x42, 0xe1, 0xd4, 0x5a, 0x8b, 0x91, 0x0c, 0x4f, 0x2d, 0x6e, 0x8a, 0x1b, 0x3c,
]);

/// Compute a deterministic `object_id` for a Semantic Registry object.
///
/// The identity is derived from `"{object_type}:{fqn}"` using UUID v5.
/// This means the same verb/attribute/entity scanned on any machine
/// will always produce the same `object_id`.
///
/// # Examples
///
/// ```
/// use sem_os_core::ids::object_id_for;
/// use sem_os_core::types::ObjectType;
///
/// let id1 = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
/// let id2 = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
/// assert_eq!(id1, id2); // deterministic
///
/// let id3 = object_id_for(ObjectType::AttributeDef, "kyc.resolve_ubo");
/// assert_ne!(id1, id3); // different object_type → different id
/// ```
pub fn object_id_for(object_type: ObjectType, fqn: &str) -> Uuid {
    let input = format!("{}:{}", object_type, fqn);
    Uuid::new_v5(&SEM_REG_NAMESPACE, input.as_bytes())
}

/// Compute a stable content hash for a definition JSON value.
///
/// Uses canonical JSON serialization (sorted keys) followed by SHA-256.
/// This detects definition drift even when field order changes.
pub fn definition_hash(definition: &serde_json::Value) -> String {
    // Canonicalize by round-tripping through BTreeMap (sorted keys)
    let canonical = canonicalize_json(definition);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();

    // Use the first 16 bytes of the UUID v5 hash as a stable content fingerprint.
    // This is NOT cryptographic — it's a change-detection hash.
    let hash_uuid = Uuid::new_v5(&SEM_REG_NAMESPACE, &bytes);
    hash_uuid.to_string()
}

/// Recursively sort JSON object keys for canonical serialization.
fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize_json(v)))
                .collect();
            serde_json::to_value(sorted).unwrap_or(serde_json::Value::Null)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalize_json).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_same_input() {
        let id1 = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
        let id2 = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_object_type_different_id() {
        let verb_id = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
        let attr_id = object_id_for(ObjectType::AttributeDef, "kyc.resolve_ubo");
        assert_ne!(verb_id, attr_id);
    }

    #[test]
    fn test_different_fqn_different_id() {
        let id1 = object_id_for(ObjectType::VerbContract, "kyc.resolve_ubo");
        let id2 = object_id_for(ObjectType::VerbContract, "kyc.check_sanctions");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_definition_hash_stable() {
        let def = serde_json::json!({"fqn": "test.verb", "name": "Test", "domain": "test"});
        let h1 = definition_hash(&def);
        let h2 = definition_hash(&def);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_definition_hash_key_order_independent() {
        let def1 = serde_json::json!({"a": 1, "b": 2});
        let def2 = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(definition_hash(&def1), definition_hash(&def2));
    }

    #[test]
    fn test_definition_hash_different_content() {
        let def1 = serde_json::json!({"fqn": "v1"});
        let def2 = serde_json::json!({"fqn": "v2"});
        assert_ne!(definition_hash(&def1), definition_hash(&def2));
    }
}
