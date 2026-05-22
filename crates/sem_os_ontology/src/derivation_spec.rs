//! Derivation specification body type.
//!
//! A `DerivationSpecBody` describes a recipe for computing a derived attribute
//! from one or more input attributes. Stored as JSONB in `sem_reg.snapshots`
//! with `object_type = 'derivation_spec'`.
//!
//! MVP: Only `FunctionRef` expressions — Rust function dispatch via the
//! `DerivationFunctionRegistry`. AST-based expressions can be added later.

use serde::{Deserialize, Serialize};

use sem_os_types::EvidenceGrade;

fn default_true() -> bool {
    true
}

/// Body type for `ObjectType::DerivationSpec`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationSpecBody {
    /// Fully qualified name, e.g. `"risk.composite_score"`.
    pub fqn: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the derivation.
    pub description: String,
    /// FQN of the output attribute produced by this derivation.
    pub output_attribute_fqn: String,
    /// Input attributes consumed by this derivation.
    pub inputs: Vec<DerivationInput>,
    /// The expression that computes the output from inputs.
    pub expression: DerivationExpression,
    /// How null inputs are handled.
    #[serde(default)]
    pub null_semantics: NullSemantics,
    /// Optional freshness constraint on input data.
    #[serde(default)]
    pub freshness_rule: Option<FreshnessRule>,
    /// How security labels are inherited from inputs.
    #[serde(default)]
    pub security_inheritance: SecurityInheritanceMode,
    /// Whether this derivation may be used as regulatory evidence.
    #[serde(default)]
    pub evidence_grade: EvidenceGrade,
    /// Inline test cases for validation.
    #[serde(default)]
    pub tests: Vec<DerivationTestCase>,
}

/// A single input to a derivation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationInput {
    /// FQN of the input attribute.
    pub attribute_fqn: String,
    /// Role of this input (e.g., `"primary"`, `"secondary"`, `"weight"`).
    pub role: String,
    /// Whether this input is required (affects null semantics).
    #[serde(default = "default_true")]
    pub required: bool,
}

/// The computation expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DerivationExpression {
    /// MVP: dispatch to a named Rust function via `DerivationFunctionRegistry`.
    FunctionRef {
        /// Name of the registered function, e.g. `"weighted_average"`.
        ref_name: String,
    },
    // Future variants:
    // ExpressionAst { ast: serde_json::Value },
    // QueryPlan { sql: String },
}

/// How null inputs are handled.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NullSemantics {
    /// Null propagates: if any required input is null, output is null.
    #[default]
    Propagate,
    /// Use a default value when inputs are null.
    Default(serde_json::Value),
    /// Skip derivation entirely when inputs are null.
    Skip,
    /// Error when required inputs are null.
    Error,
}

/// How security labels are inherited from inputs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityInheritanceMode {
    /// Default: compute inherited label using `compute_inherited_label()`.
    #[default]
    Strict,
    /// Allow declared override with steward approval.
    DeclaredOverride,
}

/// Freshness constraint on input data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreshnessRule {
    /// Maximum age of input data in seconds.
    pub max_age_seconds: u64,
}

/// Inline test case for derivation validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationTestCase {
    /// Input values keyed by attribute FQN or role.
    pub inputs: serde_json::Value,
    /// Expected output value.
    pub expected: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = DerivationSpecBody {
            fqn: "derived.total_aum".into(),
            name: "Total AUM".into(),
            description: "Sum of holdings".into(),
            output_attribute_fqn: "cbu.total_aum".into(),
            inputs: vec![DerivationInput {
                attribute_fqn: "holding.amount".into(),
                role: "addend".into(),
                required: true,
            }],
            expression: DerivationExpression::FunctionRef {
                ref_name: "sum_holdings".into(),
            },
            null_semantics: NullSemantics::Skip,
            freshness_rule: Some(FreshnessRule {
                max_age_seconds: 3600,
            }),
            security_inheritance: SecurityInheritanceMode::Strict,
            evidence_grade: EvidenceGrade::AllowedWithConstraints,
            tests: vec![DerivationTestCase {
                inputs: serde_json::json!({"holding.amount": [100, 200]}),
                expected: serde_json::json!(300),
            }],
        };
        let json = serde_json::to_value(&val).unwrap();
        // Check tagged enum FunctionRef serialization
        assert_eq!(json["expression"]["type"], "function_ref");
        // Check NullSemantics::Skip
        assert_eq!(json["null_semantics"], "skip");
        // Check defaults: default_true on DerivationInput.required
        let input: DerivationInput =
            serde_json::from_str(r#"{"attribute_fqn":"x","role":"y"}"#).unwrap();
        assert!(input.required);
        // Round-trip
        let back: DerivationSpecBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }
}
