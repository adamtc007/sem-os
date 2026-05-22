//! Projection functions — pure transforms from existing SemOS types
//! to Observatory orientation types.
//!
//! No DB calls. No new data sources. Pure functions only.

use chrono::Utc;

use crate::context_resolution::{
    ContextResolutionResponse, GroundedActionOption, GroundedActionSurface, VerbCandidate,
};
use crate::stewardship::types::{FocusState, OverlayMode};
use sem_os_types::agent_mode::AgentMode;

use super::orientation::*;

/// Project an OrientationContract from existing SemOS resolution and session state.
///
/// All inputs are existing types. The output is a packaging struct.
pub fn project_orientation(
    response: Option<&ContextResolutionResponse>,
    focus: &FocusState,
    level: ViewLevel,
    agent_mode: AgentMode,
    entry_reason: EntryReason,
    business_label: Option<&str>,
) -> OrientationContract {
    let focus_kind = focus_kind_from_focus(focus);
    let focus_identity = focus_identity_from_focus(focus, business_label);
    let scope = scope_from_level(level);
    let lens = lens_from_focus(focus);
    let available_actions = actions_from_resolution(response);

    OrientationContract {
        session_mode: agent_mode,
        view_level: level,
        focus_kind,
        focus_identity,
        scope,
        lens,
        entry_reason,
        available_actions,
        delta_from_previous: None,
        computed_at: Utc::now(),
    }
}

/// Compute the delta between two OrientationContracts.
pub fn compute_delta(prev: &OrientationContract, curr: &OrientationContract) -> OrientationDelta {
    let mode_changed = if prev.session_mode != curr.session_mode {
        Some(ModeChange {
            from: prev.session_mode,
            to: curr.session_mode,
        })
    } else {
        None
    };

    let level_changed = if prev.view_level != curr.view_level {
        Some(LevelChange {
            from: prev.view_level,
            to: curr.view_level,
        })
    } else {
        None
    };

    let focus_changed = if prev.focus_kind != curr.focus_kind
        || prev.focus_identity.canonical_id != curr.focus_identity.canonical_id
    {
        Some(FocusChange {
            from_kind: prev.focus_kind.clone(),
            to_kind: curr.focus_kind.clone(),
            from_label: prev.focus_identity.business_label.clone(),
            to_label: curr.focus_identity.business_label.clone(),
        })
    } else {
        None
    };

    let lens_changed = compute_lens_change(&prev.lens, &curr.lens);
    let scope_changed = prev.scope != curr.scope;

    let prev_actions: std::collections::HashSet<&str> = prev
        .available_actions
        .iter()
        .map(|a| a.action_id.as_str())
        .collect();
    let curr_actions: std::collections::HashSet<&str> = curr
        .available_actions
        .iter()
        .map(|a| a.action_id.as_str())
        .collect();
    let actions_added = curr_actions.difference(&prev_actions).count();
    let actions_removed = prev_actions.difference(&curr_actions).count();

    let mut summary_parts = Vec::new();
    if mode_changed.is_some() {
        summary_parts.push(format!(
            "Mode: {} → {}",
            prev.session_mode.as_ref(),
            curr.session_mode.as_ref()
        ));
    }
    if let Some(ref lc) = level_changed {
        summary_parts.push(format!("Level: {:?} → {:?}", lc.from, lc.to));
    }
    if let Some(ref fc) = focus_changed {
        summary_parts.push(format!("Focus: {} → {}", fc.from_label, fc.to_label));
    }
    if summary_parts.is_empty() {
        summary_parts.push("No significant changes".into());
    }

    OrientationDelta {
        mode_changed,
        level_changed,
        focus_changed,
        lens_changed,
        scope_changed,
        actions_added,
        actions_removed,
        summary: summary_parts.join("; "),
    }
}

// ── Internal helpers ─────────────────────────────────────────

fn focus_kind_from_focus(focus: &FocusState) -> FocusKind {
    if let Some(ref tax) = focus.taxonomy_focus {
        if tax.node_id.is_some() {
            return FocusKind::TaxonomyNode;
        }
    }
    if let Some(first) = focus.object_refs.first() {
        return match first.object_type.as_str() {
            "entity_type_def" | "entity" => FocusKind::Entity,
            "cbu" => FocusKind::Cbu,
            "document_type_def" | "document" => FocusKind::Document,
            "changeset" => FocusKind::ChangeSet,
            "guardrail" => FocusKind::Guardrail,
            "constellation_map" => FocusKind::Constellation,
            "view_def" => FocusKind::View,
            other => FocusKind::Other(other.to_string()),
        };
    }
    if focus.changeset_id.is_some() {
        return FocusKind::ChangeSet;
    }
    FocusKind::Constellation
}

fn focus_identity_from_focus(focus: &FocusState, business_label: Option<&str>) -> FocusIdentity {
    if let Some(first) = focus.object_refs.first() {
        FocusIdentity {
            canonical_id: first.fqn.clone(),
            business_label: business_label.unwrap_or(&first.fqn).to_string(),
            object_type: Some(first.object_type.clone()),
        }
    } else if let Some(changeset_id) = focus.changeset_id {
        FocusIdentity {
            canonical_id: changeset_id.to_string(),
            business_label: business_label.unwrap_or("Changeset").to_string(),
            object_type: Some("changeset".into()),
        }
    } else {
        FocusIdentity {
            canonical_id: focus.session_id.to_string(),
            business_label: business_label.unwrap_or("Session").to_string(),
            object_type: None,
        }
    }
}

pub fn scope_from_level(level: ViewLevel) -> ObservatoryScope {
    match level {
        ViewLevel::Universe => ObservatoryScope::Universe,
        ViewLevel::Cluster => ObservatoryScope::Cluster,
        ViewLevel::System => ObservatoryScope::Constellation,
        ViewLevel::Planet => ObservatoryScope::SingleObject,
        ViewLevel::Surface => ObservatoryScope::SingleObject,
        ViewLevel::Core => ObservatoryScope::GraphNeighbourhood,
    }
}

fn lens_from_focus(focus: &FocusState) -> LensState {
    let overlay = match &focus.overlay_mode {
        OverlayMode::ActiveOnly => OverlayState::ActiveOnly,
        OverlayMode::DraftOverlay { changeset_id } => OverlayState::DraftOverlay {
            changeset_id: *changeset_id,
        },
    };

    LensState {
        overlay,
        depth_probe: None,
        cluster_mode: ClusterMode::Jurisdiction,
        active_filters: vec![],
    }
}

fn actions_from_resolution(response: Option<&ContextResolutionResponse>) -> Vec<ActionDescriptor> {
    let Some(response) = response else {
        return vec![];
    };

    let mut actions = Vec::new();

    // From grounded action surface (preferred — already resolved)
    if let Some(ref gas) = response.grounded_action_surface {
        actions.extend(actions_from_grounded(gas));
    }

    // From candidate verbs (fallback if not grounded)
    if actions.is_empty() {
        actions.extend(actions_from_candidates(&response.candidate_verbs));
    }

    actions
}

fn actions_from_grounded(gas: &GroundedActionSurface) -> Vec<ActionDescriptor> {
    let mut actions: Vec<ActionDescriptor> = gas
        .valid_actions
        .iter()
        .map(|a| action_from_grounded_option(a, true))
        .collect();

    actions.extend(gas.blocked_actions.iter().map(|a| ActionDescriptor {
        action_id: a.action_id.clone(),
        label: a.description.clone(),
        action_kind: a.action_kind.clone(),
        enabled: false,
        disabled_reason: Some(a.reasons.join("; ")),
        rank_score: 0.0,
    }));

    actions
}

fn action_from_grounded_option(a: &GroundedActionOption, enabled: bool) -> ActionDescriptor {
    ActionDescriptor {
        action_id: a.action_id.clone(),
        label: a.description.clone(),
        action_kind: a.action_kind.clone(),
        enabled,
        disabled_reason: None,
        rank_score: 1.0,
    }
}

fn actions_from_candidates(candidates: &[VerbCandidate]) -> Vec<ActionDescriptor> {
    candidates
        .iter()
        .map(|vc| ActionDescriptor {
            action_id: vc.fqn.clone(),
            label: vc.description.clone(),
            action_kind: "primitive".into(),
            enabled: vc.preconditions_met,
            disabled_reason: if vc.preconditions_met {
                None
            } else {
                Some("Preconditions not met".into())
            },
            rank_score: vc.rank_score,
        })
        .collect()
}

fn compute_lens_change(prev: &LensState, curr: &LensState) -> Option<LensChange> {
    let overlay_changed = prev.overlay != curr.overlay;
    let depth_changed = prev.depth_probe != curr.depth_probe;
    let cluster_changed = prev.cluster_mode != curr.cluster_mode;
    let filters_changed = prev.active_filters.len() != curr.active_filters.len();

    if overlay_changed || depth_changed || cluster_changed || filters_changed {
        Some(LensChange {
            overlay_changed,
            depth_changed,
            cluster_changed,
            filters_changed,
        })
    } else {
        None
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_focus() -> FocusState {
        FocusState {
            session_id: Uuid::new_v4(),
            changeset_id: None,
            overlay_mode: OverlayMode::ActiveOnly,
            object_refs: vec![],
            taxonomy_focus: None,
            resolution_context: None,
            updated_at: Utc::now(),
            updated_by: crate::stewardship::types::FocusUpdateSource::Agent,
        }
    }

    #[test]
    fn test_project_orientation_default() {
        let focus = make_focus();
        let contract = project_orientation(
            None,
            &focus,
            ViewLevel::Universe,
            AgentMode::Governed,
            EntryReason::SessionStart,
            None,
        );

        assert_eq!(contract.view_level, ViewLevel::Universe);
        assert_eq!(contract.session_mode, AgentMode::Governed);
        assert!(contract.available_actions.is_empty());
        assert!(matches!(contract.entry_reason, EntryReason::SessionStart));
    }

    #[test]
    fn test_compute_delta_no_change() {
        let focus = make_focus();
        let c1 = project_orientation(
            None,
            &focus,
            ViewLevel::Universe,
            AgentMode::Governed,
            EntryReason::SessionStart,
            None,
        );
        let c2 = project_orientation(
            None,
            &focus,
            ViewLevel::Universe,
            AgentMode::Governed,
            EntryReason::SessionStart,
            None,
        );

        let delta = compute_delta(&c1, &c2);
        assert!(delta.mode_changed.is_none());
        assert!(delta.level_changed.is_none());
        assert!(delta.focus_changed.is_none());
        assert!(delta.lens_changed.is_none());
    }

    #[test]
    fn test_compute_delta_level_change() {
        let focus = make_focus();
        let c1 = project_orientation(
            None,
            &focus,
            ViewLevel::Universe,
            AgentMode::Governed,
            EntryReason::SessionStart,
            None,
        );
        let c2 = project_orientation(
            None,
            &focus,
            ViewLevel::Cluster,
            AgentMode::Governed,
            EntryReason::DrillDown {
                from_level: ViewLevel::Universe,
                from_id: "lu".into(),
            },
            None,
        );

        let delta = compute_delta(&c1, &c2);
        assert!(delta.level_changed.is_some());
        let lc = delta.level_changed.unwrap();
        assert_eq!(lc.from, ViewLevel::Universe);
        assert_eq!(lc.to, ViewLevel::Cluster);
    }

    #[test]
    fn test_compute_delta_mode_change() {
        let focus = make_focus();
        let c1 = project_orientation(
            None,
            &focus,
            ViewLevel::Surface,
            AgentMode::Governed,
            EntryReason::SessionStart,
            None,
        );
        let c2 = project_orientation(
            None,
            &focus,
            ViewLevel::Surface,
            AgentMode::Maintenance,
            EntryReason::DirectNavigation,
            None,
        );

        let delta = compute_delta(&c1, &c2);
        assert!(delta.mode_changed.is_some());
        assert!(delta.summary.contains("Mode"));
    }
}
