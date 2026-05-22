//! Pure grounding logic for constellation-bound action surfaces.
//!
//! This module is the Sem OS-owned replacement substrate for the legacy
//! `sem_reg::constellation` + `sem_reg::reducer` action-surface helpers.
//! It deliberately stays pure: callers provide authored constellation/state
//! definitions plus current slot states, and Sem OS returns valid and blocked
//! actions for a chosen slot.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::context_resolution::{
    BlockedActionOption, GroundedActionOption, GroundedConstraintSignal,
};
use sem_os_ontology::constellation_map_def::{
    ConstellationMapDefBody, DependencyEntry, SlotDef, VerbPaletteEntry,
};
use sem_os_ontology::state_machine_def::StateMachineDefBody;

/// Fully flattened constellation model used for grounding.
#[derive(Debug, Clone)]
pub struct ConstellationModel {
    pub constellation: String,
    pub slots: HashMap<String, ResolvedSlot>,
    pub state_machines: HashMap<String, StateMachineDefBody>,
}

/// Flattened slot with full path metadata.
#[derive(Debug, Clone)]
pub struct ResolvedSlot {
    pub name: String,
    pub def: SlotDef,
}

/// Slot-scoped action-surface result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotActionSurface {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_actions: Vec<GroundedActionOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_actions: Vec<BlockedActionOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraint_signals: Vec<GroundedConstraintSignal>,
}

impl ConstellationModel {
    /// Build a pure Sem OS grounding model from authored constellation/state payloads.
    ///
    /// # Examples
    /// ```rust
    /// use std::collections::HashMap;
    /// use sem_os_core::constellation_map_def::ConstellationMapDefBody;
    /// use sem_os_core::grounding::ConstellationModel;
    ///
    /// let map: ConstellationMapDefBody = serde_yaml::from_str(
    ///     r#"
    /// fqn: demo
    /// constellation: demo
    /// jurisdiction: LU
    /// slots:
    ///   cbu:
    ///     type: cbu
    ///     table: cbus
    ///     pk: cbu_id
    ///     cardinality: root
    /// "#,
    /// )
    /// .unwrap();
    ///
    /// let model = ConstellationModel::from_parts(map, HashMap::new());
    /// assert_eq!(model.constellation, "demo");
    /// ```
    pub fn from_parts(
        map: ConstellationMapDefBody,
        state_machines: HashMap<String, StateMachineDefBody>,
    ) -> Self {
        let mut slots = HashMap::new();
        flatten_slots(&map.slots, &mut slots, Vec::new());
        Self {
            constellation: map.constellation,
            slots,
            state_machines,
        }
    }
}

/// Compute valid and blocked actions for a resolved slot using current slot states.
///
/// # Examples
/// ```rust
/// use std::collections::HashMap;
/// use sem_os_core::constellation_map_def::ConstellationMapDefBody;
/// use sem_os_core::grounding::{compute_slot_action_surface, ConstellationModel};
/// use sem_os_core::state_machine_def::StateMachineDefBody;
///
/// let map: ConstellationMapDefBody = serde_yaml::from_str(
///     r#"
/// fqn: demo
/// constellation: demo
/// jurisdiction: LU
/// slots:
///   cbu:
///     type: cbu
///     table: cbus
///     pk: cbu_id
///     cardinality: root
///     verbs:
///       show: cbu.read
///   case:
///     type: case
///     table: cases
///     pk: case_id
///     cardinality: optional
///     state_machine: case_machine
///     depends_on:
///       - slot: cbu
///         min_state: filled
///     verbs:
///       open:
///         verb: case.open
///         when: intake
/// "#,
/// )
/// .unwrap();
///
/// let mut states = HashMap::new();
/// states.insert("cbu".to_string(), "filled".to_string());
/// states.insert("case".to_string(), "intake".to_string());
///
/// let mut machines = HashMap::new();
/// machines.insert(
///     "case_machine".to_string(),
///     StateMachineDefBody {
///         fqn: "case_machine".to_string(),
///         state_machine: "case_machine".to_string(),
///         description: None,
///         states: vec!["intake".to_string(), "review".to_string()],
///         initial: "intake".to_string(),
///         transitions: vec![],
///         reducer: None,
///     },
/// );
///
/// let model = ConstellationModel::from_parts(map, machines);
/// let surface = compute_slot_action_surface(&model, &states, "case").unwrap();
/// assert_eq!(surface.valid_actions[0].action_id, "case.open");
/// ```
pub fn compute_slot_action_surface(
    model: &ConstellationModel,
    slot_states: &HashMap<String, String>,
    slot_name: &str,
) -> Result<SlotActionSurface, String> {
    let resolved = model
        .slots
        .get(slot_name)
        .ok_or_else(|| format!("unknown slot '{slot_name}'"))?;
    let current_state = slot_states
        .get(slot_name)
        .cloned()
        .or_else(|| {
            resolved
                .def
                .state_machine
                .as_ref()
                .and_then(|name| model.state_machines.get(name))
                .map(|machine| machine.initial.clone())
        })
        .unwrap_or_else(|| "empty".to_string());

    let mut valid_actions = Vec::new();
    let mut blocked_actions = Vec::new();
    let mut constraint_signals = Vec::new();
    let mut seen = HashSet::new();

    for entry in resolved.def.verbs.values() {
        let action_id = entry.verb_fqn().to_string();
        let state_allowed = match entry {
            VerbPaletteEntry::Simple(_) => true,
            VerbPaletteEntry::Gated { when, .. } => when
                .to_vec()
                .into_iter()
                .any(|required| state_at_least(model, slot_name, &current_state, &required)),
        };
        let dependency_blocks = resolved
            .def
            .depends_on
            .iter()
            .filter_map(|dep| dependency_block_detail(model, slot_states, &resolved.name, dep))
            .collect::<Vec<_>>();
        let dependency_reasons = dependency_blocks
            .iter()
            .map(|signal| signal.message.clone())
            .collect::<Vec<_>>();
        constraint_signals.extend(dependency_blocks);

        if state_allowed && dependency_reasons.is_empty() {
            if seen.insert(action_id.clone()) {
                valid_actions.push(GroundedActionOption {
                    action_id,
                    action_kind: action_kind_for(entry).to_string(),
                    description: format!("Valid action for slot '{}'", resolved.name),
                    requires_subject: true,
                    rationale: Some(format!(
                        "slot '{}' is in state '{}'",
                        resolved.name, current_state
                    )),
                });
            }
        } else {
            let mut reasons = dependency_reasons;
            if !state_allowed {
                reasons.push(format!(
                    "slot '{}' is in state '{}' which does not satisfy action gating",
                    resolved.name, current_state
                ));
            }
            blocked_actions.push(BlockedActionOption {
                action_id,
                action_kind: action_kind_for(entry).to_string(),
                description: format!("Blocked action for slot '{}'", resolved.name),
                reasons,
            });
        }
    }

    Ok(SlotActionSurface {
        valid_actions,
        blocked_actions,
        constraint_signals,
    })
}

fn flatten_slots(
    slots: &std::collections::BTreeMap<String, SlotDef>,
    out: &mut HashMap<String, ResolvedSlot>,
    prefix: Vec<String>,
) {
    for (name, def) in slots {
        let mut path = prefix.clone();
        path.push(name.clone());
        let slot_name = path.join(".");
        out.insert(
            slot_name.clone(),
            ResolvedSlot {
                name: slot_name.clone(),
                def: def.clone(),
            },
        );
        if !def.children.is_empty() {
            flatten_slots(&def.children, out, path);
        }
    }
}

fn dependency_block_detail(
    model: &ConstellationModel,
    slot_states: &HashMap<String, String>,
    slot_name: &str,
    dep: &DependencyEntry,
) -> Option<GroundedConstraintSignal> {
    match slot_states.get(dep.slot_name()) {
        Some(state) if state_at_least(model, dep.slot_name(), state, dep.min_state()) => None,
        Some(state) => Some(GroundedConstraintSignal {
            kind: "dependency_block".to_string(),
            slot_path: slot_name.to_string(),
            related_slot: Some(dep.slot_name().to_string()),
            required_state: Some(dep.min_state().to_string()),
            actual_state: Some(state.clone()),
            message: format!(
                "dependency '{}' is in state '{}' but requires '{}'",
                dep.slot_name(),
                state,
                dep.min_state()
            ),
        }),
        None => Some(GroundedConstraintSignal {
            kind: "dependency_block".to_string(),
            slot_path: slot_name.to_string(),
            related_slot: Some(dep.slot_name().to_string()),
            required_state: Some(dep.min_state().to_string()),
            actual_state: None,
            message: format!(
                "dependency '{}' is missing; requires '{}'",
                dep.slot_name(),
                dep.min_state()
            ),
        }),
    }
}

fn state_at_least(
    model: &ConstellationModel,
    slot_name: &str,
    current: &str,
    min_state: &str,
) -> bool {
    if let Some(order) = model
        .slots
        .get(slot_name)
        .and_then(|slot| slot.def.state_machine.as_ref())
        .and_then(|name| model.state_machines.get(name))
        .map(|machine| &machine.states)
    {
        let current_rank = order.iter().position(|state| state == current);
        let min_rank = order.iter().position(|state| state == min_state);
        if let (Some(current_rank), Some(min_rank)) = (current_rank, min_rank) {
            return current_rank >= min_rank;
        }
    }
    state_rank_fallback(current) >= state_rank_fallback(min_state)
}

fn state_rank_fallback(state: &str) -> usize {
    match state {
        "empty" => 0,
        "placeholder" => 1,
        "filled" => 2,
        "intake" => 2,
        "workstream_open" | "discovery" | "alleged" => 3,
        "screening_complete" | "evidence_collected" | "assessment" | "provable" => 4,
        "review" | "verified" | "proved" => 5,
        "approved" => 6,
        _ => 0,
    }
}

fn action_kind_for(entry: &VerbPaletteEntry) -> &'static str {
    let verb = entry.verb_fqn();
    if verb.split('.').count() > 2 {
        "macro"
    } else {
        "primitive"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_ontology::constellation_map_def::ConstellationMapDefBody;
    use sem_os_ontology::state_machine_def::StateMachineDefBody;

    #[test]
    fn computes_valid_action_for_slot_with_satisfied_dependencies() {
        let map: ConstellationMapDefBody = serde_yaml::from_str(
            r#"
fqn: demo
constellation: demo
jurisdiction: LU
slots:
  cbu:
    type: cbu
    table: cbus
    pk: cbu_id
    cardinality: root
  case:
    type: case
    table: cases
    pk: case_id
    cardinality: optional
    depends_on:
      - slot: cbu
        min_state: filled
    state_machine: case_machine
    verbs:
      open:
        verb: case.open
        when: intake
"#,
        )
        .unwrap();

        let mut machines = HashMap::new();
        machines.insert(
            "case_machine".to_string(),
            StateMachineDefBody {
                fqn: "case_machine".to_string(),
                state_machine: "case_machine".to_string(),
                description: None,
                states: vec!["intake".to_string(), "review".to_string()],
                initial: "intake".to_string(),
                transitions: vec![],
                reducer: None,
            },
        );

        let model = ConstellationModel::from_parts(map, machines);
        let states = HashMap::from([
            ("cbu".to_string(), "filled".to_string()),
            ("case".to_string(), "intake".to_string()),
        ]);

        let surface = compute_slot_action_surface(&model, &states, "case").unwrap();
        assert_eq!(surface.valid_actions.len(), 1);
        assert!(surface.blocked_actions.is_empty());
        assert_eq!(surface.valid_actions[0].action_id, "case.open");
    }

    #[test]
    fn blocks_action_when_dependency_is_missing() {
        let map: ConstellationMapDefBody = serde_yaml::from_str(
            r#"
fqn: demo
constellation: demo
jurisdiction: LU
slots:
  cbu:
    type: cbu
    table: cbus
    pk: cbu_id
    cardinality: root
  mandate:
    type: mandate
    table: mandates
    pk: mandate_id
    cardinality: optional
    depends_on:
      - slot: case
        min_state: intake
    verbs:
      create: mandate.create
"#,
        )
        .unwrap();

        let model = ConstellationModel::from_parts(map, HashMap::new());
        let states = HashMap::new();
        let surface = compute_slot_action_surface(&model, &states, "mandate").unwrap();
        assert!(surface.valid_actions.is_empty());
        assert_eq!(surface.blocked_actions.len(), 1);
        assert!(surface.blocked_actions[0].reasons[0].contains("dependency 'case'"));
        assert_eq!(surface.constraint_signals.len(), 1);
        assert_eq!(surface.constraint_signals[0].kind, "dependency_block");
        assert_eq!(
            surface.constraint_signals[0].related_slot.as_deref(),
            Some("case")
        );
    }
}
