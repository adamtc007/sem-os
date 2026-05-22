//! Non-mutating state-transition simulation.
//!
//! The first implementation is deliberately small: it evaluates declared
//! Domain Pack transitions against a supplied state snapshot and returns the
//! state advance SemOS would emit if the matching runtime verb succeeded.

use crate::domain_pack::{DomainPackManifest, DomainTransition};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSimulationRequest {
    pub pack_id: String,
    pub transition_ref: String,
    pub entity_id: Uuid,
    pub entity_type: String,
    pub state_machine: String,
    pub current_state: String,
    pub requested_state: String,
    #[serde(default)]
    pub state_snapshot_id: Option<String>,
    #[serde(default)]
    pub configuration_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSimulationResult {
    pub transition_ref: String,
    pub entity_id: Uuid,
    pub entity_type: String,
    pub state_machine: String,
    pub from_state: String,
    pub to_state: String,
    pub verb: String,
    pub semantic_diff: SemanticStateDiff,
    pub predicted_advance: SimulatedStateAdvance,
    #[serde(default)]
    pub state_snapshot_id: Option<String>,
    #[serde(default)]
    pub configuration_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticStateDiff {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedStateAdvance {
    pub entity_id: Uuid,
    pub to_node: String,
    pub slot_path: String,
    pub reason: String,
    pub writes_since_push_delta: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateSimulationError {
    PackMismatch {
        expected: String,
        actual: String,
    },
    UnknownTransition {
        transition_ref: String,
    },
    DryRunDisabled {
        transition_ref: String,
    },
    TransitionShapeMismatch {
        transition_ref: String,
        field: String,
        expected: String,
        actual: String,
    },
    CurrentStateMismatch {
        transition_ref: String,
        expected: String,
        actual: String,
    },
    RequestedStateMismatch {
        transition_ref: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemoryStateSnapshot {
    states: BTreeMap<StateKey, String>,
}

impl InMemoryStateSnapshot {
    pub fn new(states: impl IntoIterator<Item = (StateKey, String)>) -> Self {
        Self {
            states: states.into_iter().collect(),
        }
    }

    pub fn get(&self, key: &StateKey) -> Option<&str> {
        self.states.get(key).map(String::as_str)
    }

    pub fn simulate_from_pack(
        &self,
        manifest: &DomainPackManifest,
        request: &StateSimulationRequest,
    ) -> Result<StateSimulationResult, StateSimulationError> {
        let key = StateKey {
            entity_id: request.entity_id,
            entity_type: request.entity_type.clone(),
            state_machine: request.state_machine.clone(),
        };
        let current_state = self
            .get(&key)
            .unwrap_or(request.current_state.as_str())
            .to_string();

        simulate_transition_from_pack(
            manifest,
            &StateSimulationRequest {
                current_state,
                ..request.clone()
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StateKey {
    pub entity_id: Uuid,
    pub entity_type: String,
    pub state_machine: String,
}

pub fn simulate_transition_from_pack(
    manifest: &DomainPackManifest,
    request: &StateSimulationRequest,
) -> Result<StateSimulationResult, StateSimulationError> {
    if request.pack_id != manifest.pack_id {
        return Err(StateSimulationError::PackMismatch {
            expected: manifest.pack_id.clone(),
            actual: request.pack_id.clone(),
        });
    }

    let transition = manifest
        .allowed_transitions
        .iter()
        .find(|transition| transition.transition_ref == request.transition_ref)
        .ok_or_else(|| StateSimulationError::UnknownTransition {
            transition_ref: request.transition_ref.clone(),
        })?;

    validate_transition_shape(transition, request)?;

    Ok(StateSimulationResult {
        transition_ref: transition.transition_ref.clone(),
        entity_id: request.entity_id,
        entity_type: request.entity_type.clone(),
        state_machine: request.state_machine.clone(),
        from_state: request.current_state.clone(),
        to_state: request.requested_state.clone(),
        verb: transition.verb.clone(),
        semantic_diff: SemanticStateDiff {
            field: "status".to_string(),
            before: request.current_state.clone(),
            after: request.requested_state.clone(),
        },
        predicted_advance: SimulatedStateAdvance {
            entity_id: request.entity_id,
            to_node: format!("kyc-case:{}", request.requested_state.to_lowercase()),
            slot_path: "kyc-case/workstream".to_string(),
            reason: format!(
                "{} - {} -> {}",
                transition.verb, request.current_state, request.requested_state
            ),
            writes_since_push_delta: 1,
        },
        state_snapshot_id: request.state_snapshot_id.clone(),
        configuration_version: request.configuration_version.clone(),
    })
}

fn validate_transition_shape(
    transition: &DomainTransition,
    request: &StateSimulationRequest,
) -> Result<(), StateSimulationError> {
    if !transition.dry_run_enabled {
        return Err(StateSimulationError::DryRunDisabled {
            transition_ref: transition.transition_ref.clone(),
        });
    }

    assert_transition_field(
        transition,
        "entity_type",
        &transition.entity_type,
        &request.entity_type,
    )?;
    assert_transition_field(
        transition,
        "state_machine",
        &transition.state_machine,
        &request.state_machine,
    )?;

    if transition.from_state != request.current_state {
        return Err(StateSimulationError::CurrentStateMismatch {
            transition_ref: transition.transition_ref.clone(),
            expected: transition.from_state.clone(),
            actual: request.current_state.clone(),
        });
    }

    if transition.to_state != request.requested_state {
        return Err(StateSimulationError::RequestedStateMismatch {
            transition_ref: transition.transition_ref.clone(),
            expected: transition.to_state.clone(),
            actual: request.requested_state.clone(),
        });
    }

    Ok(())
}

fn assert_transition_field(
    transition: &DomainTransition,
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), StateSimulationError> {
    if expected == actual {
        return Ok(());
    }

    Err(StateSimulationError::TransitionShapeMismatch {
        transition_ref: transition.transition_ref.clone(),
        field: field.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain_pack::{
        ClassificationLimit, ContextClassificationPolicy, DiscoveryProbe, DomainPackManifest,
        DomainTransition, PackCompatibilityTier, PackImplementationMode,
    };
    use uuid::uuid;

    const CASE_ID: Uuid = uuid!("11111111-1111-1111-1111-111111111111");

    fn manifest() -> DomainPackManifest {
        DomainPackManifest {
            pack_id: "ob-poc.kyc".to_string(),
            name: "ob-poc KYC".to_string(),
            version: "0.1.0".to_string(),
            implementation_mode: PackImplementationMode::NativeCompiled,
            compatibility_tier: PackCompatibilityTier::DryRunOnly,
            owned_dags: vec![],
            owned_packs: vec![],
            owned_state_machines: vec![],
            owned_constellation_maps: vec![],
            owned_constellation_families: vec![],
            owned_universes: vec![],
            owned_verb_prefixes: vec![],
            owned_entity_kinds: vec![],
            business_crates: vec![],
            owned_constellations: vec!["kyc.onboarding".to_string()],
            allowed_transitions: vec![DomainTransition {
                transition_ref: "kyc-case.intake-to-discovery".to_string(),
                entity_type: "kyc_case".to_string(),
                state_machine: "kyc_case_lifecycle".to_string(),
                verb: "kyc-case.update-status".to_string(),
                from_state: "INTAKE".to_string(),
                to_state: "DISCOVERY".to_string(),
                dry_run_enabled: true,
                mutation_enabled: false,
                hitl_required: true,
                evidence_refs_required: vec!["case_id".to_string()],
            }],
            discovery_probes: vec![DiscoveryProbe {
                probe_id: "kyc-case.read-state".to_string(),
                operation: "read_state".to_string(),
                target: "\"ob-poc\".cases.status".to_string(),
                idempotent: true,
                modeled: true,
                first_class_state_mutation: false,
            }],
            projection_catalog: vec![],
            mention_namespaces: vec![],
            declared_modes: vec![],
            workflow_phases: vec![],
            acp_personas: vec![],
            resource_uri_schemes: vec![],
            external_mcp_transports: vec![],
            typed_extension_points: vec![],
            classification_policy: ContextClassificationPolicy {
                max_prompt_classification: ClassificationLimit::Internal,
                allow_external_llm: false,
                required_redactions: vec!["pii".to_string()],
            },
        }
    }

    fn request(current_state: &str, requested_state: &str) -> StateSimulationRequest {
        StateSimulationRequest {
            pack_id: "ob-poc.kyc".to_string(),
            transition_ref: "kyc-case.intake-to-discovery".to_string(),
            entity_id: CASE_ID,
            entity_type: "kyc_case".to_string(),
            state_machine: "kyc_case_lifecycle".to_string(),
            current_state: current_state.to_string(),
            requested_state: requested_state.to_string(),
            state_snapshot_id: Some("snapshot-1".to_string()),
            configuration_version: Some("config-1".to_string()),
        }
    }

    #[test]
    fn simulates_kyc_update_status_without_mutation() {
        let snapshot = InMemoryStateSnapshot::new([(
            StateKey {
                entity_id: CASE_ID,
                entity_type: "kyc_case".to_string(),
                state_machine: "kyc_case_lifecycle".to_string(),
            },
            "INTAKE".to_string(),
        )]);

        let result = snapshot
            .simulate_from_pack(&manifest(), &request("STALE_CLIENT_INPUT", "DISCOVERY"))
            .expect("simulation succeeds");

        assert_eq!(result.from_state, "INTAKE");
        assert_eq!(result.to_state, "DISCOVERY");
        assert_eq!(result.verb, "kyc-case.update-status");
        assert_eq!(result.semantic_diff.field, "status");
        assert_eq!(result.predicted_advance.to_node, "kyc-case:discovery");
        assert_eq!(result.predicted_advance.slot_path, "kyc-case/workstream");

        let key = StateKey {
            entity_id: CASE_ID,
            entity_type: "kyc_case".to_string(),
            state_machine: "kyc_case_lifecycle".to_string(),
        };
        assert_eq!(snapshot.get(&key), Some("INTAKE"));
    }

    #[test]
    fn simulation_is_deterministic() {
        let manifest = manifest();
        let request = request("INTAKE", "DISCOVERY");

        let a = simulate_transition_from_pack(&manifest, &request).expect("first simulation");
        let b = simulate_transition_from_pack(&manifest, &request).expect("second simulation");

        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).expect("json"),
            serde_json::to_string(&b).expect("json")
        );
    }

    #[test]
    fn refuses_illegal_current_state() {
        let err = simulate_transition_from_pack(&manifest(), &request("REVIEW", "DISCOVERY"))
            .expect_err("transition refused");

        assert_eq!(
            err,
            StateSimulationError::CurrentStateMismatch {
                transition_ref: "kyc-case.intake-to-discovery".to_string(),
                expected: "INTAKE".to_string(),
                actual: "REVIEW".to_string(),
            }
        );
    }

    #[test]
    fn refuses_unknown_transition() {
        let mut request = request("INTAKE", "DISCOVERY");
        request.transition_ref = "kyc-case.review-to-approved".to_string();

        let err =
            simulate_transition_from_pack(&manifest(), &request).expect_err("transition refused");

        assert_eq!(
            err,
            StateSimulationError::UnknownTransition {
                transition_ref: "kyc-case.review-to-approved".to_string(),
            }
        );
    }

    #[test]
    fn refuses_dry_run_disabled_transition() {
        let mut manifest = manifest();
        manifest.allowed_transitions[0].dry_run_enabled = false;

        let err = simulate_transition_from_pack(&manifest, &request("INTAKE", "DISCOVERY"))
            .expect_err("transition refused");

        assert_eq!(
            err,
            StateSimulationError::DryRunDisabled {
                transition_ref: "kyc-case.intake-to-discovery".to_string(),
            }
        );
    }
}
