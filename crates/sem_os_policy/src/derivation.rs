//! Derivation function registry and evaluation — pure logic, no DB dependency.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::security::compute_inherited_label;
use sem_os_ontology::derivation_spec::{DerivationExpression, DerivationSpecBody, NullSemantics};
use sem_os_types::SecurityLabel;

// ── Derivation function trait ─────────────────────────────────

/// A pure function that computes a derived attribute value from inputs.
pub trait DerivationFn: Send + Sync {
    fn evaluate(&self, inputs: &serde_json::Value) -> Result<serde_json::Value, DerivationError>;
}

impl<F> DerivationFn for F
where
    F: Fn(&serde_json::Value) -> Result<serde_json::Value, DerivationError> + Send + Sync,
{
    fn evaluate(&self, inputs: &serde_json::Value) -> Result<serde_json::Value, DerivationError> {
        (self)(inputs)
    }
}

// ── Errors ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivationError {
    FunctionNotFound { ref_name: String },
    NullRequiredInput { attribute_fqn: String },
    ExecutionFailed { message: String },
    StalenessViolation { max_age_seconds: u64 },
}

impl std::fmt::Display for DerivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FunctionNotFound { ref_name } => {
                write!(
                    f,
                    "Derivation function '{}' not found in registry",
                    ref_name
                )
            }
            Self::NullRequiredInput { attribute_fqn } => {
                write!(f, "Required input '{}' is null", attribute_fqn)
            }
            Self::ExecutionFailed { message } => {
                write!(f, "Derivation execution failed: {}", message)
            }
            Self::StalenessViolation { max_age_seconds } => {
                write!(
                    f,
                    "Input data exceeds freshness rule ({} seconds)",
                    max_age_seconds
                )
            }
        }
    }
}

impl std::error::Error for DerivationError {}

// ── Derivation result ─────────────────────────────────────────

/// Result of evaluating a derivation spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct DerivationResult {
    pub value: serde_json::Value,
    pub spec_snapshot_id: Uuid,
    pub input_snapshot_ids: Vec<Uuid>,
    pub inherited_label: SecurityLabel,
    pub evaluated_at: DateTime<Utc>,
}

// ── Function registry ─────────────────────────────────────────

/// Registry mapping function names to evaluation implementations.
pub struct DerivationFunctionRegistry {
    functions: HashMap<String, Arc<dyn DerivationFn>>,
}

impl DerivationFunctionRegistry {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, func: Arc<dyn DerivationFn>) {
        self.functions.insert(name.to_string(), func);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn DerivationFn>> {
        self.functions.get(name)
    }

    /// Evaluate a derivation spec against provided inputs.
    pub fn evaluate(
        &self,
        spec: &DerivationSpecBody,
        inputs: &serde_json::Value,
        input_labels: &[SecurityLabel],
        spec_snapshot_id: Uuid,
        input_snapshot_ids: Vec<Uuid>,
    ) -> Result<DerivationResult, DerivationError> {
        // Check null semantics for required inputs
        for input_def in &spec.inputs {
            if input_def.required {
                let val = inputs.get(&input_def.attribute_fqn);
                if val.is_none() || val == Some(&serde_json::Value::Null) {
                    match &spec.null_semantics {
                        NullSemantics::Error => {
                            return Err(DerivationError::NullRequiredInput {
                                attribute_fqn: input_def.attribute_fqn.clone(),
                            });
                        }
                        NullSemantics::Propagate => {
                            return Ok(DerivationResult {
                                value: serde_json::Value::Null,
                                spec_snapshot_id,
                                input_snapshot_ids,
                                inherited_label: compute_inherited_label(input_labels),
                                evaluated_at: Utc::now(),
                            });
                        }
                        NullSemantics::Skip => {
                            return Ok(DerivationResult {
                                value: serde_json::Value::Null,
                                spec_snapshot_id,
                                input_snapshot_ids,
                                inherited_label: compute_inherited_label(input_labels),
                                evaluated_at: Utc::now(),
                            });
                        }
                        NullSemantics::Default(default_val) => {
                            // Continue with default — would need to inject into inputs
                            // For now, proceed and let the function handle it
                            let _ = default_val;
                        }
                    }
                }
            }
        }

        // Dispatch to registered function
        let func = match &spec.expression {
            DerivationExpression::FunctionRef { ref_name } => {
                self.get(ref_name)
                    .ok_or_else(|| DerivationError::FunctionNotFound {
                        ref_name: ref_name.clone(),
                    })?
            }
        };

        let value = func.evaluate(inputs)?;
        let inherited_label = compute_inherited_label(input_labels);

        Ok(DerivationResult {
            value,
            spec_snapshot_id,
            input_snapshot_ids,
            inherited_label,
            evaluated_at: Utc::now(),
        })
    }
}

impl Default for DerivationFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_ontology::derivation_spec::{
        DerivationExpression, DerivationInput, DerivationSpecBody, NullSemantics,
    };
    use sem_os_types::Classification;

    fn make_spec(fn_name: &str, null_sem: NullSemantics) -> DerivationSpecBody {
        DerivationSpecBody {
            fqn: "test.derived".into(),
            name: "Test".into(),
            description: "test".into(),
            output_attribute_fqn: "out.val".into(),
            inputs: vec![DerivationInput {
                attribute_fqn: "in.val".into(),
                role: "primary".into(),
                required: true,
            }],
            expression: DerivationExpression::FunctionRef {
                ref_name: fn_name.into(),
            },
            null_semantics: null_sem,
            freshness_rule: None,
            security_inheritance: Default::default(),
            evidence_grade: Default::default(),
            tests: vec![],
        }
    }

    fn empty_label() -> SecurityLabel {
        SecurityLabel {
            classification: Classification::Public,
            pii: false,
            jurisdictions: vec![],
            purpose_limitation: vec![],
            handling_controls: vec![],
        }
    }

    #[test]
    fn register_and_evaluate() {
        let mut reg = DerivationFunctionRegistry::new();
        reg.register(
            "double",
            Arc::new(|inputs: &serde_json::Value| {
                let n = inputs.get("in.val").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(serde_json::json!(n * 2))
            }),
        );
        let spec = make_spec("double", NullSemantics::Error);
        let inputs = serde_json::json!({"in.val": 5});
        let result = reg
            .evaluate(&spec, &inputs, &[empty_label()], Uuid::nil(), vec![])
            .unwrap();
        assert_eq!(result.value, serde_json::json!(10));
    }

    #[test]
    fn function_not_found() {
        let reg = DerivationFunctionRegistry::new();
        let spec = make_spec("missing_fn", NullSemantics::Error);
        let inputs = serde_json::json!({"in.val": 1});
        let err = reg
            .evaluate(&spec, &inputs, &[], Uuid::nil(), vec![])
            .unwrap_err();
        assert_eq!(
            err,
            DerivationError::FunctionNotFound {
                ref_name: "missing_fn".into()
            }
        );
    }

    #[test]
    fn null_required_input_error_semantics() {
        let mut reg = DerivationFunctionRegistry::new();
        reg.register(
            "noop",
            Arc::new(|_: &serde_json::Value| Ok(serde_json::json!(null))),
        );
        let spec = make_spec("noop", NullSemantics::Error);
        let inputs = serde_json::json!({});
        let err = reg
            .evaluate(&spec, &inputs, &[], Uuid::nil(), vec![])
            .unwrap_err();
        assert_eq!(
            err,
            DerivationError::NullRequiredInput {
                attribute_fqn: "in.val".into()
            }
        );
    }
}
