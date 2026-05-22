//! Utterance -> intent -> DSL discovery helpers built on top of `AffinityGraph`.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::affinity::{AffinityGraph, AffinityKind, DataRef};
use sem_os_ontology::verb_contract::VerbContractBody;

/// Full discovery payload for `registry.discover-dsl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    pub intent_matches: Vec<IntentMatch>,
    pub suggested_sequence: Vec<VerbChainSuggestion>,
    pub disambiguation_needed: Vec<DisambiguationPrompt>,
    pub governance_context: GovernanceContext,
}

/// Ranked intent candidate for an utterance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMatch {
    pub verb: String,
    pub score: f32,
    pub matched_phrase: String,
}

/// Suggested step in a DSL chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbChainSuggestion {
    pub verb: String,
    pub rationale: String,
    pub args: HashMap<String, String>,
    pub data_footprint: Vec<String>,
}

/// Prompt for missing user context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisambiguationPrompt {
    pub question: String,
    pub lookup: Option<String>,
    pub options: Vec<String>,
}

/// Governance posture of the proposed chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceContext {
    pub all_tables_governed: bool,
    pub required_mode: String,
    pub policy_check: Option<String>,
}

/// Run full utterance -> intent -> chain discovery.
///
/// # Examples
/// ```
/// use sem_os_core::affinity::{AffinityGraph, discovery::discover_dsl};
/// use sem_os_core::verb_contract::VerbContractBody;
///
/// let graph = AffinityGraph::build(&[]);
/// let verbs: Vec<VerbContractBody> = Vec::new();
/// let response = discover_dsl("create entity", &graph, &verbs, None, 3, None, None);
/// assert!(response.intent_matches.is_empty());
/// ```
pub fn discover_dsl(
    utterance: &str,
    graph: &AffinityGraph,
    verb_contracts: &[VerbContractBody],
    subject_id: Option<Uuid>,
    max_chain_length: usize,
    allowed_verbs: Option<&HashSet<String>>,
    policy_check: Option<String>,
) -> DiscoveryResponse {
    let intent_matches = match_intent(utterance, graph, verb_contracts)
        .into_iter()
        .filter(|m| {
            allowed_verbs
                .map(|set| set.contains(m.verb.as_str()))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    let mut suggested_sequence = Vec::new();
    let mut disambiguation_needed = Vec::new();

    if let Some(primary) = intent_matches.first() {
        let mut chain = synthesize_chain(&primary.verb, graph, verb_contracts, max_chain_length);
        if let Some(allowed) = allowed_verbs {
            chain.retain(|step| allowed.contains(step.verb.as_str()));
        }
        suggested_sequence = chain;

        if let Some(primary_body) = verb_contracts.iter().find(|v| v.fqn == primary.verb) {
            disambiguation_needed = generate_disambiguation(primary_body, subject_id);
        }
    }

    let governance_context =
        build_governance_context(graph, &suggested_sequence, subject_id, policy_check);

    DiscoveryResponse {
        intent_matches,
        suggested_sequence,
        disambiguation_needed,
        governance_context,
    }
}

/// Match an utterance against active verb contracts.
///
/// # Examples
/// ```
/// use sem_os_core::affinity::{AffinityGraph, discovery::match_intent};
/// use sem_os_core::verb_contract::VerbContractBody;
///
/// let graph = AffinityGraph::build(&[]);
/// let verbs: Vec<VerbContractBody> = Vec::new();
/// let matches = match_intent("assign role", &graph, &verbs);
/// assert!(matches.is_empty());
/// ```
pub fn match_intent(
    utterance: &str,
    graph: &AffinityGraph,
    verb_contracts: &[VerbContractBody],
) -> Vec<IntentMatch> {
    let utterance_norm = normalize(utterance);
    if utterance_norm.is_empty() {
        return Vec::new();
    }
    let utter_tokens = token_set(&utterance_norm);

    let mut matches = Vec::new();

    for verb in verb_contracts {
        if !graph.known_verbs.contains(verb.fqn.as_str()) {
            continue;
        }

        let mut best_score = 0.0f32;
        let mut best_phrase = String::new();

        let mut candidates = Vec::new();
        candidates.extend(verb.invocation_phrases.iter().cloned());
        candidates.push(verb.fqn.replace('.', " "));
        candidates.push(format!("{} {}", verb.domain, verb.action));

        for phrase in candidates {
            let phrase_norm = normalize(&phrase);
            if phrase_norm.is_empty() {
                continue;
            }
            let phrase_tokens = token_set(&phrase_norm);

            let union = utter_tokens.union(&phrase_tokens).count() as f32;
            if union <= f32::EPSILON {
                continue;
            }
            let overlap = utter_tokens.intersection(&phrase_tokens).count() as f32;
            let mut score = overlap / union;

            if utterance_norm.contains(phrase_norm.as_str()) {
                score += 0.25;
            }
            if phrase_norm.contains(utterance_norm.as_str()) {
                score += 0.15;
            }
            if utter_tokens.contains(verb.action.as_str()) {
                score += 0.08;
            }
            if utter_tokens.contains(verb.domain.as_str()) {
                score += 0.08;
            }

            score = score.min(1.0);
            if score > best_score {
                best_score = score;
                best_phrase = phrase;
            }
        }

        if best_score > 0.0 {
            matches.push(IntentMatch {
                verb: verb.fqn.clone(),
                score: (best_score * 1000.0).round() / 1000.0,
                matched_phrase: best_phrase,
            });
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.verb.cmp(&b.verb))
    });
    matches.truncate(8);
    matches
}

/// Synthesize a DSL chain for a primary verb.
///
/// # Examples
/// ```
/// use sem_os_core::affinity::{AffinityGraph, discovery::synthesize_chain};
/// use sem_os_core::verb_contract::VerbContractBody;
///
/// let graph = AffinityGraph::build(&[]);
/// let verbs: Vec<VerbContractBody> = Vec::new();
/// let chain = synthesize_chain("missing.verb", &graph, &verbs, 4);
/// assert!(chain.is_empty());
/// ```
pub fn synthesize_chain(
    primary_verb: &str,
    graph: &AffinityGraph,
    verb_contracts: &[VerbContractBody],
    max_chain_length: usize,
) -> Vec<VerbChainSuggestion> {
    if max_chain_length == 0 {
        return Vec::new();
    }

    let primary_body = verb_contracts.iter().find(|v| v.fqn == primary_verb);
    if primary_body.is_none() && !graph.known_verbs.contains(primary_verb) {
        return Vec::new();
    }

    let lookup_keys: HashSet<String> = graph
        .data_for_verb(primary_verb)
        .into_iter()
        .filter_map(|da| match da.affinity_kind {
            AffinityKind::ArgLookup { .. } => Some(da.data_ref.index_key()),
            _ => None,
        })
        .collect();

    let mut prerequisites = graph
        .adjacent_verbs(primary_verb)
        .into_iter()
        .map(|(verb, shared_refs)| {
            let data_aff = graph.data_for_verb(&verb);
            let mut produces_lookup_hits = 0usize;
            for edge in &data_aff {
                let key = edge.data_ref.index_key();
                if !lookup_keys.contains(&key) {
                    continue;
                }
                let contributes = matches!(
                    edge.affinity_kind,
                    AffinityKind::Produces
                        | AffinityKind::CrudInsert
                        | AffinityKind::CrudUpdate
                        | AffinityKind::ProducesAttribute
                );
                if contributes {
                    produces_lookup_hits += 1;
                }
            }

            (
                verb,
                shared_refs,
                produces_lookup_hits,
                data_aff
                    .iter()
                    .map(|d| d.data_ref.index_key())
                    .collect::<BTreeSet<_>>(),
            )
        })
        .filter(|(verb, shared_refs, produces_lookup_hits, _)| {
            verb != primary_verb && (*produces_lookup_hits > 0 || !shared_refs.is_empty())
        })
        .collect::<Vec<_>>();

    prerequisites.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| b.1.len().cmp(&a.1.len()))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut chain = Vec::new();
    let prereq_limit = max_chain_length.saturating_sub(1);
    for (verb, shared_refs, produces_lookup_hits, footprint) in
        prerequisites.into_iter().take(prereq_limit)
    {
        let args = verb_contracts
            .iter()
            .find(|v| v.fqn == verb)
            .map(required_arg_placeholders)
            .unwrap_or_default();

        let rationale = if produces_lookup_hits > 0 {
            format!(
                "Produces data looked up by '{primary_verb}' ({} lookup overlaps)",
                produces_lookup_hits
            )
        } else {
            format!(
                "Shares {} data dependencies with '{primary_verb}'",
                shared_refs.len()
            )
        };

        chain.push(VerbChainSuggestion {
            verb,
            rationale,
            args,
            data_footprint: footprint.into_iter().collect(),
        });
    }

    let primary_args = primary_body
        .map(required_arg_placeholders)
        .unwrap_or_default();
    let primary_footprint = graph
        .data_for_verb(primary_verb)
        .into_iter()
        .map(|d| d.data_ref.index_key())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    chain.push(VerbChainSuggestion {
        verb: primary_verb.to_owned(),
        rationale: "Primary intent match from utterance scoring".to_owned(),
        args: primary_args,
        data_footprint: primary_footprint,
    });

    chain
}

/// Generate follow-up prompts for missing required arguments.
///
/// # Examples
/// ```
/// use sem_os_core::affinity::discovery::generate_disambiguation;
/// use sem_os_core::verb_contract::VerbContractBody;
///
/// let verb = VerbContractBody {
///     fqn: "x.y".into(),
///     domain: "x".into(),
///     action: "y".into(),
///     description: "d".into(),
///     behavior: "plugin".into(),
///     args: vec![],
///     returns: None,
///     preconditions: vec![],
///     postconditions: vec![],
///     produces: None,
///     consumes: vec![],
///     invocation_phrases: vec![],
///     subject_kinds: vec![],
///     phase_tags: vec![],
///     harm_class: None,
///     action_class: None,
///     precondition_states: vec![],
///     requires_subject: true,
///     produces_focus: false,
///     metadata: None,
///     crud_mapping: None,
///     reads_from: vec![],
///     writes_to: vec![],
///     outputs: vec![],
///     produces_shared_facts: vec![],
/// };
/// let prompts = generate_disambiguation(&verb, None);
/// assert!(!prompts.is_empty());
/// ```
pub fn generate_disambiguation(
    verb: &VerbContractBody,
    subject_id: Option<Uuid>,
) -> Vec<DisambiguationPrompt> {
    let mut prompts = Vec::new();

    if verb.requires_subject && subject_id.is_none() {
        prompts.push(DisambiguationPrompt {
            question: format!("What subject entity should '{}' run against?", verb.fqn),
            lookup: Some("entities".to_owned()),
            options: Vec::new(),
        });
    }

    for arg in &verb.args {
        if !arg.required {
            continue;
        }

        if let Some(lookup) = &arg.lookup {
            let schema = lookup.schema.clone().unwrap_or_else(|| "public".to_owned());
            prompts.push(DisambiguationPrompt {
                question: format!(
                    "Which {} should be used for '{}' ?",
                    lookup.entity_type, arg.name
                ),
                lookup: Some(format!("{}.{}", schema, lookup.table)),
                options: arg.valid_values.clone().unwrap_or_default(),
            });
            continue;
        }

        if let Some(valid_values) = &arg.valid_values {
            prompts.push(DisambiguationPrompt {
                question: format!("Select a value for required argument '{}'.", arg.name),
                lookup: None,
                options: valid_values.clone(),
            });
            continue;
        }

        prompts.push(DisambiguationPrompt {
            question: format!("Provide a value for required argument '{}'.", arg.name),
            lookup: None,
            options: Vec::new(),
        });
    }

    prompts
}

fn build_governance_context(
    graph: &AffinityGraph,
    chain: &[VerbChainSuggestion],
    subject_id: Option<Uuid>,
    policy_check: Option<String>,
) -> GovernanceContext {
    let tables = chain
        .iter()
        .flat_map(|step| step.data_footprint.iter())
        .filter_map(|item| item.strip_prefix("table:"))
        .collect::<Vec<_>>();

    let mut all_tables_governed = true;
    for table_ref in tables {
        let parts = table_ref.split(':').collect::<Vec<_>>();
        if parts.len() != 2 {
            all_tables_governed = false;
            break;
        }
        let verbs = graph.verbs_for_table(parts[0], parts[1]);
        if verbs.is_empty() {
            all_tables_governed = false;
            break;
        }
    }

    GovernanceContext {
        all_tables_governed,
        required_mode: if subject_id.is_some() {
            "subject-scoped".to_owned()
        } else {
            "global".to_owned()
        },
        policy_check,
    }
}

fn required_arg_placeholders(verb: &VerbContractBody) -> HashMap<String, String> {
    verb.args
        .iter()
        .filter(|a| a.required)
        .map(|a| {
            let value = a
                .default
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<required>".to_owned());
            (a.name.clone(), value)
        })
        .collect()
}

fn normalize(text: &str) -> String {
    let lower = text.to_lowercase();
    lower
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_set(text: &str) -> HashSet<String> {
    text.split_whitespace().map(ToOwned::to_owned).collect()
}

/// Build discovery map edges (utterance -> verb -> data) for visualization.
///
/// # Examples
/// ```
/// use sem_os_core::affinity::discovery::discovery_edges;
/// use sem_os_core::affinity::AffinityGraph;
///
/// let graph = AffinityGraph::build(&[]);
/// let edges = discovery_edges("hello", &graph, &[]);
/// assert!(edges.is_empty());
/// ```
pub fn discovery_edges(
    utterance: &str,
    graph: &AffinityGraph,
    matches: &[IntentMatch],
) -> Vec<(String, String)> {
    let utter_node = format!("utterance:{}", normalize(utterance));
    let mut edges = Vec::new();

    for m in matches {
        let verb_node = format!("verb:{}", m.verb);
        edges.push((utter_node.clone(), verb_node.clone()));

        for da in graph.data_for_verb(&m.verb) {
            let data_node = format!("data:{}", data_label(&da.data_ref));
            edges.push((verb_node.clone(), data_node));
        }
    }

    edges
}

fn data_label(data_ref: &DataRef) -> String {
    match data_ref {
        DataRef::Table(t) => format!("{}:{}", t.schema, t.table),
        DataRef::Column(c) => format!("{}:{}.{}", c.schema, c.table, c.column),
        DataRef::EntityType { fqn } => format!("entity:{}", fqn),
        DataRef::Attribute { fqn } => format!("attribute:{}", fqn),
    }
}

#[cfg(test)]
mod tests {
    use crate::affinity::{AffinityEdge, AffinityProvenance, TableRef};

    use super::*;

    fn sample_verb(fqn: &str, phrase: &str) -> VerbContractBody {
        let mut parts = fqn.split('.');
        let domain = parts.next().unwrap_or("x").to_owned();
        let action = parts.next().unwrap_or("y").to_owned();
        VerbContractBody {
            fqn: fqn.to_owned(),
            domain,
            action,
            description: "desc".to_owned(),
            behavior: "plugin".to_owned(),
            args: vec![],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec![phrase.to_owned()],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: None,
            action_class: None,
            precondition_states: vec![],
            requires_subject: false,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        }
    }

    fn sample_graph() -> AffinityGraph {
        let edges = vec![AffinityEdge {
            verb_fqn: "cbu-role.assign".to_owned(),
            data_ref: DataRef::Table(TableRef::new("ob-poc", "entities")),
            affinity_kind: AffinityKind::ArgLookup {
                arg_name: "entity-id".to_owned(),
            },
            provenance: AffinityProvenance::VerbArgLookup,
        }];

        let mut graph = AffinityGraph {
            edges,
            verb_to_data: HashMap::new(),
            data_to_verb: HashMap::new(),
            entity_to_table: HashMap::new(),
            table_to_entity: HashMap::new(),
            attribute_to_column: HashMap::new(),
            derivation_edges: vec![],
            entity_relationships: vec![],
            known_verbs: HashSet::new(),
        };

        graph
            .verb_to_data
            .insert("cbu-role.assign".to_owned(), vec![0]);
        graph
            .data_to_verb
            .insert("table:ob-poc:entities".to_owned(), vec![0]);
        graph.known_verbs.insert("cbu-role.assign".to_owned());
        graph
    }

    #[test]
    fn match_intent_ranks_expected_verb() {
        let graph = sample_graph();
        let verbs = vec![
            sample_verb("cbu-role.assign", "set up depositary"),
            sample_verb("entity.create", "create legal entity"),
        ];

        let matches = match_intent("set up depositary", &graph, &verbs);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].verb, "cbu-role.assign");
    }

    #[test]
    fn generate_disambiguation_for_required_args_and_subject() {
        let mut verb = sample_verb("cbu-role.assign", "set up depositary");
        verb.requires_subject = true;
        verb.args.push(sem_os_ontology::verb_contract::VerbArgDef {
            name: "entity-id".to_owned(),
            arg_type: "uuid".to_owned(),
            required: true,
            description: None,
            lookup: Some(sem_os_ontology::verb_contract::VerbArgLookup {
                table: "entities".to_owned(),
                entity_type: "entity".to_owned(),
                schema: Some("ob-poc".to_owned()),
                search_key: None,
                primary_key: None,
            }),
            valid_values: None,
            default: None,
            maps_to: None,
        });

        let prompts = generate_disambiguation(&verb, None);
        assert!(prompts.len() >= 2);
    }
}
