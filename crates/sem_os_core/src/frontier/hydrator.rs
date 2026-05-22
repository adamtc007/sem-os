use dsl_core::{
    config::{
        dag::ClosureType,
        predicate::{
            parse_green_when, AttrValue, CmpOp, EntityRef as PredicateEntityRef, EntitySetRef,
            Predicate, Validity,
        },
    },
    frontier::{
        CompletenessAssertionStatus, DiscretionaryReason, EntityRef, FrontierFact, FrontierFacts,
        GreenWhenStatus, HydrateFrontierError, InstanceFrontier, InvalidFact, InvalidFactDetail,
        MissingFact, ReachableDestination,
    },
    parser::parse_single_verb,
    resolver::{ResolvedSlot, ResolvedTemplate},
};
use dsl_types::constellation_map_def::Cardinality;

/// Hydrate the current action frontier from caller-supplied predicate facts.
///
/// This Phase 3 hydrator evaluates an already-resolved template against synthetic
/// in-memory facts. It does not read substrate tables directly; the substrate
/// reader is expected to supply `EntityRef::facts` before calling this function.
///
/// # Examples
/// ```rust,ignore
/// use dsl_core::frontier::EntityRef;
/// use sem_os_core::frontier::hydrate_frontier;
///
/// let frontier = hydrate_frontier(entity_ref, &resolved_template)?;
/// assert_eq!(frontier.current_state, "PENDING");
/// # Ok::<(), dsl_core::frontier::HydrateFrontierError>(())
/// ```
pub fn hydrate_frontier(
    entity_ref: EntityRef,
    resolved_template: &ResolvedTemplate,
) -> Result<InstanceFrontier, HydrateFrontierError> {
    let slot = resolved_template
        .slot(&entity_ref.slot_id)
        .ok_or_else(|| HydrateFrontierError::SlotNotFound(entity_ref.slot_id.clone()))?;

    let reachable = resolved_template
        .transitions
        .iter()
        .filter(|transition| {
            transition.slot_id == slot.id && transition.from == entity_ref.current_state
        })
        .filter(|transition| {
            transition
                .via
                .as_deref()
                .is_some_and(is_agent_actionable_verb)
        })
        .map(|transition| ReachableDestination {
            destination_state: transition.to.clone(),
            via_verb: transition.via.clone(),
            status: evaluate_destination(
                slot,
                resolved_template,
                &entity_ref,
                transition.destination_green_when.as_deref(),
            ),
        })
        .collect();

    Ok(InstanceFrontier {
        current_state: entity_ref.current_state.clone(),
        entity_ref,
        reachable,
    })
}

fn is_agent_actionable_verb(via: &str) -> bool {
    let via = via.trim();
    if via.is_empty() {
        return false;
    }
    parse_single_verb(&format!("({via})")).is_ok()
}

fn evaluate_destination(
    slot: &ResolvedSlot,
    resolved_template: &ResolvedTemplate,
    entity_ref: &EntityRef,
    green_when: Option<&str>,
) -> GreenWhenStatus {
    let aggregate_status = evaluate_green_when(slot, resolved_template, entity_ref, green_when);
    if matches!(aggregate_status, GreenWhenStatus::Red { .. }) {
        return aggregate_status;
    }

    if slot.justification_required == Some(true)
        || slot.role_guard.is_some()
        || slot.audit_class.is_some()
    {
        return GreenWhenStatus::Discretionary(DiscretionaryReason {
            reason: discretionary_reason(slot),
        });
    }

    if slot.closure == Some(ClosureType::Open) {
        let assertion = slot
            .completeness_assertion
            .as_ref()
            .and_then(|assertion| assertion.predicate.as_deref())
            .unwrap_or("missing completeness_assertion");
        let satisfied = slot
            .completeness_assertion
            .as_ref()
            .and_then(|assertion| assertion.predicate.as_deref())
            .is_some_and(|predicate| {
                matches!(
                    evaluate_green_when(slot, resolved_template, entity_ref, Some(predicate)),
                    GreenWhenStatus::Green
                )
            });
        if !satisfied {
            return GreenWhenStatus::AwaitingCompleteness(CompletenessAssertionStatus {
                assertion: assertion.to_string(),
                satisfied,
            });
        }
    }

    GreenWhenStatus::Green
}

fn evaluate_green_when(
    slot: &ResolvedSlot,
    resolved_template: &ResolvedTemplate,
    entity_ref: &EntityRef,
    green_when: Option<&str>,
) -> GreenWhenStatus {
    let Some(green_when) = green_when.filter(|value| !value.trim().is_empty()) else {
        return GreenWhenStatus::Green;
    };
    let bound_entities = slot
        .predicate_bindings
        .iter()
        .map(|binding| binding.entity.as_str())
        .collect::<Vec<_>>();

    match parse_green_when(green_when) {
        Ok(predicate) => {
            let mut ctx = EvalContext {
                root_entity_id: &entity_ref.entity_id,
                current_state: &entity_ref.current_state,
                slot,
                resolved_template,
                facts: &entity_ref.facts,
                missing: Vec::new(),
                invalid: Vec::new(),
            };
            if eval_predicate(&predicate, &mut ctx, None) {
                GreenWhenStatus::Green
            } else {
                if ctx.missing.is_empty() && ctx.invalid.is_empty() {
                    debug_assert!(
                        false,
                        "predicate variant returned false without structured diagnostics"
                    );
                    ctx.invalid.push(invalid_fact(
                        "predicate",
                        "predicate failed without structured diagnostic",
                        InvalidFactDetail::PredicateFailureWithoutDiagnostic,
                    ));
                }
                GreenWhenStatus::Red {
                    missing: ctx.missing,
                    invalid: ctx.invalid,
                }
            }
        }
        Err(err) => GreenWhenStatus::Red {
            missing: Vec::new(),
            invalid: vec![invalid_fact(
                bound_entities.join(","),
                err.to_string(),
                InvalidFactDetail::PredicateParseError {
                    reason: err.to_string(),
                },
            )],
        },
    }
}

fn discretionary_reason(slot: &ResolvedSlot) -> String {
    if let Some(audit_class) = &slot.audit_class {
        return format!(
            "slot {} requires discretionary audit class {audit_class}",
            slot.id
        );
    }
    if slot.role_guard.is_some() {
        return format!(
            "slot {} requires role-guarded discretionary handling",
            slot.id
        );
    }
    format!("slot {} requires justification", slot.id)
}

struct EvalContext<'a> {
    root_entity_id: &'a str,
    current_state: &'a str,
    slot: &'a ResolvedSlot,
    resolved_template: &'a ResolvedTemplate,
    facts: &'a FrontierFacts,
    missing: Vec<MissingFact>,
    invalid: Vec<InvalidFact>,
}

fn eval_predicate(
    predicate: &Predicate,
    ctx: &mut EvalContext<'_>,
    this_fact: Option<&FrontierFact>,
) -> bool {
    match predicate {
        Predicate::And(items) => items
            .iter()
            .all(|item| eval_predicate(item, ctx, this_fact)),
        Predicate::Exists { entity } => eval_exists(entity, ctx, this_fact),
        Predicate::StateIn { entity, state_set } => {
            eval_state_in(entity, state_set, ctx, this_fact)
        }
        Predicate::AttrCmp {
            entity,
            attr,
            op,
            value,
        } => eval_attr_cmp(entity, attr, *op, value, ctx, this_fact),
        Predicate::Every { set, condition } => {
            let facts = set_facts(set, ctx);
            if has_blocking_set_invalid(ctx, set) {
                return false;
            }
            if facts.is_empty() {
                ctx.missing.push(MissingFact {
                    entity: set.kind.clone(),
                    reason: "set is empty".to_string(),
                });
                return false;
            }
            facts
                .iter()
                .all(|fact| eval_predicate(condition, ctx, Some(fact)))
        }
        Predicate::NoneExists { set, condition } => {
            let facts = set_facts(set, ctx);
            if has_blocking_set_invalid(ctx, set) {
                return false;
            }

            let mut forbidden = false;
            for fact in &facts {
                let missing_len = ctx.missing.len();
                let invalid_len = ctx.invalid.len();
                if eval_predicate(condition, ctx, Some(fact)) {
                    forbidden = true;
                    let fact_id = fact.attrs.get("id").cloned();
                    ctx.invalid.push(invalid_fact(
                        set.kind.clone(),
                        format!("forbidden member present: {fact_id:?}"),
                        InvalidFactDetail::ForbiddenMemberPresent {
                            kind: set.kind.clone(),
                            fact_id,
                        },
                    ));
                } else {
                    ctx.missing.truncate(missing_len);
                    ctx.invalid.truncate(invalid_len);
                }
            }
            !forbidden
        }
        Predicate::AtLeastOne { set, condition } => {
            let facts = set_facts(set, ctx);
            if has_blocking_set_invalid(ctx, set) {
                return false;
            }
            let matched = facts
                .iter()
                .any(|fact| eval_predicate(condition, ctx, Some(fact)));
            if !matched {
                ctx.missing.push(MissingFact {
                    entity: set.kind.clone(),
                    reason: "no set member satisfied condition".to_string(),
                });
            }
            matched
        }
        Predicate::Count {
            set,
            condition,
            op,
            threshold,
        } => {
            let facts = set_facts(set, ctx);
            if has_blocking_set_invalid(ctx, set) {
                return false;
            }
            let count = facts
                .iter()
                .filter(|fact| {
                    condition
                        .as_ref()
                        .is_none_or(|condition| eval_predicate(condition, ctx, Some(fact)))
                })
                .count() as u64;
            let ok = cmp_u64(count, *op, *threshold);
            if !ok {
                ctx.invalid.push(invalid_fact(
                    set.kind.clone(),
                    format!("count {count} did not satisfy {op:?} {threshold}"),
                    InvalidFactDetail::CountThresholdFailed {
                        kind: set.kind.clone(),
                        observed: count,
                        op: *op,
                        threshold: *threshold,
                    },
                ));
            }
            ok
        }
        Predicate::Obtained { entity, validity } => match validity {
            Validity::StateIn(state_set) => eval_state_in(entity, state_set, ctx, this_fact),
            Validity::DelegatedToEntityDag => eval_exists(entity, ctx, this_fact),
        },
    }
}

fn eval_exists(
    entity: &PredicateEntityRef,
    ctx: &mut EvalContext<'_>,
    _this_fact: Option<&FrontierFact>,
) -> bool {
    match entity {
        PredicateEntityRef::This => true,
        PredicateEntityRef::Named(kind)
        | PredicateEntityRef::Parent(kind)
        | PredicateEntityRef::Scoped { kind, .. } => {
            let exists = ctx.facts.get(kind).is_some_and(|facts| !facts.is_empty());
            if !exists {
                ctx.missing.push(MissingFact {
                    entity: kind.clone(),
                    reason: "no bound fact found".to_string(),
                });
            }
            exists
        }
    }
}

fn eval_state_in(
    entity: &PredicateEntityRef,
    state_set: &[String],
    ctx: &mut EvalContext<'_>,
    this_fact: Option<&FrontierFact>,
) -> bool {
    match entity {
        PredicateEntityRef::This => {
            let state = this_fact
                .and_then(|fact| fact.state.as_deref())
                .unwrap_or(ctx.current_state);
            let ok = state_set.iter().any(|allowed| allowed == state);
            if !ok {
                ctx.invalid.push(invalid_fact(
                    "this",
                    format!("state {state} not in {state_set:?}"),
                    InvalidFactDetail::StateNotInSet {
                        state: state.to_string(),
                        allowed: state_set.to_vec(),
                    },
                ));
            }
            ok
        }
        PredicateEntityRef::Named(kind)
        | PredicateEntityRef::Parent(kind)
        | PredicateEntityRef::Scoped { kind, .. } => {
            let facts = ctx.facts.get(kind).cloned().unwrap_or_default();
            if facts.is_empty() {
                ctx.missing.push(MissingFact {
                    entity: kind.clone(),
                    reason: "no bound fact found".to_string(),
                });
                return false;
            }
            let ok = facts.iter().any(|fact| {
                fact.state
                    .as_ref()
                    .is_some_and(|state| state_set.iter().any(|allowed| allowed == state))
            });
            if !ok {
                ctx.invalid.push(invalid_fact(
                    kind.clone(),
                    format!("no fact state in {state_set:?}"),
                    InvalidFactDetail::StateNotInSet {
                        state: "<none matched>".to_string(),
                        allowed: state_set.to_vec(),
                    },
                ));
            }
            ok
        }
    }
}

fn eval_attr_cmp(
    entity: &PredicateEntityRef,
    attr: &str,
    op: CmpOp,
    value: &AttrValue,
    ctx: &mut EvalContext<'_>,
    this_fact: Option<&FrontierFact>,
) -> bool {
    let expected = attr_value_to_string(value);
    let values = match entity {
        PredicateEntityRef::This => this_fact
            .and_then(|fact| fact.attrs.get(attr))
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        PredicateEntityRef::Named(kind)
        | PredicateEntityRef::Parent(kind)
        | PredicateEntityRef::Scoped { kind, .. } => ctx
            .facts
            .get(kind)
            .into_iter()
            .flatten()
            .filter_map(|fact| fact.attrs.get(attr).cloned())
            .collect(),
    };
    let ok = values
        .iter()
        .any(|actual| cmp_string_or_number(actual, op, &expected));
    if !ok {
        ctx.invalid.push(invalid_fact(
            entity_name(entity),
            format!("attribute {attr} did not satisfy comparison"),
            InvalidFactDetail::AttributeComparisonFailed {
                attr: attr.to_string(),
            },
        ));
    }
    ok
}

fn set_facts(set: &EntitySetRef, ctx: &mut EvalContext<'_>) -> Vec<FrontierFact> {
    let Some(facts) = ctx.facts.get(&set.kind) else {
        return Vec::new();
    };
    let Some(set_slot) = ctx.resolved_template.slot(&set.kind) else {
        return facts.clone();
    };
    if !matches!(set_slot.cardinality, Some(Cardinality::Recursive)) {
        return facts.clone();
    }
    if !facts
        .iter()
        .any(|fact| fact.attrs.contains_key("parent_id"))
    {
        return facts.clone();
    }

    let mut out = Vec::new();
    let mut path = Vec::new();
    let mut recursive = RecursiveCollectContext {
        kind: &set.kind,
        facts,
        max_depth: set_slot.max_depth.or(ctx.slot.max_depth),
        invalid: &mut ctx.invalid,
    };
    collect_recursive_set(ctx.root_entity_id, 1, &mut path, &mut out, &mut recursive);
    out
}

fn has_blocking_set_invalid(ctx: &EvalContext<'_>, set: &EntitySetRef) -> bool {
    ctx.invalid.iter().any(|invalid| {
        invalid.entity == set.kind
            && matches!(
                invalid.detail,
                InvalidFactDetail::CycleDetected { .. }
                    | InvalidFactDetail::MaxDepthExceeded { .. }
                    | InvalidFactDetail::RecursiveFactMissingId
            )
    })
}

struct RecursiveCollectContext<'a, 'b> {
    kind: &'a str,
    facts: &'a [FrontierFact],
    max_depth: Option<usize>,
    invalid: &'b mut Vec<InvalidFact>,
}

fn collect_recursive_set(
    parent_id: &str,
    depth: usize,
    path: &mut Vec<String>,
    out: &mut Vec<FrontierFact>,
    ctx: &mut RecursiveCollectContext<'_, '_>,
) {
    if let Some(max_depth) = ctx.max_depth {
        if depth > max_depth {
            ctx.invalid.push(invalid_fact(
                ctx.kind.to_string(),
                format!("max_depth {max_depth} exceeded at depth {depth}"),
                InvalidFactDetail::MaxDepthExceeded {
                    kind: ctx.kind.to_string(),
                    depth,
                    max_depth,
                },
            ));
            return;
        }
    }

    for fact in ctx.facts.iter().filter(|fact| {
        fact.attrs
            .get("parent_id")
            .is_some_and(|value| value == parent_id)
    }) {
        let Some(id) = fact.attrs.get("id").cloned() else {
            ctx.invalid.push(invalid_fact(
                ctx.kind.to_string(),
                "recursive fact missing id",
                InvalidFactDetail::RecursiveFactMissingId,
            ));
            continue;
        };
        if path.contains(&id) {
            let mut cycle = path.clone();
            cycle.push(id);
            ctx.invalid.push(invalid_fact(
                ctx.kind.to_string(),
                format!("cycle detected: {}", cycle.join(" -> ")),
                InvalidFactDetail::CycleDetected { entities: cycle },
            ));
            continue;
        }

        out.push(fact.clone());
        path.push(id.clone());
        collect_recursive_set(&id, depth + 1, path, out, ctx);
        path.pop();
    }
}

fn invalid_fact(
    entity: impl Into<String>,
    reason: impl Into<String>,
    detail: InvalidFactDetail,
) -> InvalidFact {
    InvalidFact {
        entity: entity.into(),
        reason: reason.into(),
        detail,
    }
}

fn cmp_u64(left: u64, op: CmpOp, right: u64) -> bool {
    match op {
        CmpOp::Eq => left == right,
        CmpOp::Ne => left != right,
        CmpOp::Lt => left < right,
        CmpOp::Le => left <= right,
        CmpOp::Gt => left > right,
        CmpOp::Ge => left >= right,
    }
}

fn cmp_string_or_number(left: &str, op: CmpOp, right: &str) -> bool {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => match op {
            CmpOp::Eq => left == right,
            CmpOp::Ne => left != right,
            CmpOp::Lt => left < right,
            CmpOp::Le => left <= right,
            CmpOp::Gt => left > right,
            CmpOp::Ge => left >= right,
        },
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => match op {
            CmpOp::Eq => false,
            CmpOp::Ne => true,
            CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => false,
        },
        _ => match op {
            CmpOp::Eq => left == right,
            CmpOp::Ne => left != right,
            CmpOp::Lt => left < right,
            CmpOp::Le => left <= right,
            CmpOp::Gt => left > right,
            CmpOp::Ge => left >= right,
        },
    }
}

fn attr_value_to_string(value: &AttrValue) -> String {
    match value {
        AttrValue::String(value) | AttrValue::Number(value) | AttrValue::Symbol(value) => {
            value.clone()
        }
        AttrValue::Bool(value) => value.to_string(),
    }
}

fn entity_name(entity: &PredicateEntityRef) -> String {
    match entity {
        PredicateEntityRef::This => "this".to_string(),
        PredicateEntityRef::Named(kind)
        | PredicateEntityRef::Parent(kind)
        | PredicateEntityRef::Scoped { kind, .. } => kind.clone(),
    }
}
