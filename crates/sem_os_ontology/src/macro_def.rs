//! Macro definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

/// Body of a `macro_def` registry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDefBody {
    pub fqn: String,
    pub kind: String,
    #[serde(default)]
    pub ui: Option<serde_json::Value>,
    #[serde(default)]
    pub routing: Option<serde_json::Value>,
    #[serde(default)]
    pub target: Option<serde_json::Value>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub prereqs: Vec<serde_json::Value>,
    #[serde(default)]
    pub expands_to: Vec<serde_json::Value>,
    #[serde(default)]
    pub sets_state: Vec<serde_json::Value>,
    #[serde(default)]
    pub unlocks: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = MacroDefBody {
            fqn: "case.open".into(),
            kind: "macro".into(),
            ui: Some(serde_json::json!({"label": "Open Case"})),
            routing: Some(serde_json::json!({"operator-domain": "case"})),
            target: None,
            args: None,
            prereqs: vec![],
            expands_to: vec![serde_json::json!({"verb": "kyc-case.create"})],
            sets_state: vec![],
            unlocks: vec!["case.submit".into()],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: MacroDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
