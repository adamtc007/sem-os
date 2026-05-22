//! Membership rules for the semantic registry.
//!
//! A membership rule binds a registry object (attribute, verb, entity type)
//! to a taxonomy node. This enables classification queries such as
//! "which attributes belong to KYC High Risk?".

use serde::{Deserialize, Serialize};

/// The kind of membership relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipKind {
    /// Object is directly classified under this node
    Direct,
    /// Object inherits membership through a parent relationship
    Inherited,
    /// Object is conditionally classified (rule must be evaluated)
    Conditional,
    /// Object is excluded from this node (negative membership)
    Excluded,
}

/// A condition that must hold for conditional membership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipCondition {
    /// Type of condition: `attribute_equals`, `entity_has_role`, etc.
    pub kind: String,
    /// The field or attribute to check
    pub field: String,
    /// Operator: `eq`, `ne`, `in`, `not_in`, `gt`, `lt`
    #[serde(default = "default_eq")]
    pub operator: String,
    /// Expected value(s)
    pub value: serde_json::Value,
}

fn default_eq() -> String {
    "eq".into()
}

/// Body for a membership rule snapshot.
///
/// Links a target registry object to a taxonomy node with a membership kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipRuleBody {
    /// Fully qualified name for this rule
    pub fqn: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: Option<String>,
    /// FQN of the taxonomy this rule operates within
    pub taxonomy_fqn: String,
    /// FQN of the specific taxonomy node
    pub node_fqn: String,
    /// Kind of membership
    pub membership_kind: MembershipKind,
    /// What type of registry object is being classified
    /// (`attribute_def`, `verb_contract`, `entity_type_def`)
    pub target_type: String,
    /// FQN of the target registry object
    pub target_fqn: String,
    /// Conditions (for `MembershipKind::Conditional`)
    #[serde(default)]
    pub conditions: Vec<MembershipCondition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = MembershipRuleBody {
            fqn: "mem.kyc_fund".into(),
            name: "KYC Fund".into(),
            description: Some("Fund membership rule".into()),
            taxonomy_fqn: "tax.kyc".into(),
            node_fqn: "tax.kyc.fund".into(),
            membership_kind: MembershipKind::Conditional,
            target_type: "entity_type_def".into(),
            target_fqn: "entity.fund".into(),
            conditions: vec![MembershipCondition {
                kind: "attribute_equals".into(),
                field: "entity.type".into(),
                operator: "eq".into(),
                value: serde_json::json!("fund"),
            }],
        };
        let json = serde_json::to_value(&val).unwrap();
        let back: MembershipRuleBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn membership_kind_snake_case() {
        assert_eq!(
            serde_json::to_value(MembershipKind::Direct).unwrap(),
            "direct"
        );
        assert_eq!(
            serde_json::to_value(MembershipKind::Inherited).unwrap(),
            "inherited"
        );
        assert_eq!(
            serde_json::to_value(MembershipKind::Conditional).unwrap(),
            "conditional"
        );
        assert_eq!(
            serde_json::to_value(MembershipKind::Excluded).unwrap(),
            "excluded"
        );
    }

    #[test]
    fn default_operator_eq() {
        let cond: MembershipCondition = serde_json::from_value(serde_json::json!({
            "kind": "attr", "field": "f", "value": true
        }))
        .unwrap();
        assert_eq!(cond.operator, "eq");
    }
}
