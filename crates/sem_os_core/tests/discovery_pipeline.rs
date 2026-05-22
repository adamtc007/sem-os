use std::collections::{HashMap, HashSet};

use sem_os_ontology::verb_contract::{
    ActionClass, HarmClass, VerbArgDef, VerbArgLookup, VerbContractBody, VerbCrudMapping,
};
use sem_os_policy::affinity::{
    discover_dsl, AffinityEdge, AffinityGraph, AffinityKind, AffinityProvenance, DataRef, TableRef,
};

fn verb_contracts() -> Vec<VerbContractBody> {
    vec![
        VerbContractBody {
            fqn: "cbu-role.assign".to_owned(),
            domain: "cbu-role".to_owned(),
            action: "assign".to_owned(),
            description: "Assign a depositary role".to_owned(),
            behavior: "plugin".to_owned(),
            args: vec![VerbArgDef {
                name: "entity-id".to_owned(),
                arg_type: "uuid".to_owned(),
                required: true,
                description: Some("Target entity".to_owned()),
                lookup: Some(VerbArgLookup {
                    table: "entities".to_owned(),
                    entity_type: "entity".to_owned(),
                    schema: Some("ob-poc".to_owned()),
                    search_key: Some("entity_name".to_owned()),
                    primary_key: Some("entity_id".to_owned()),
                }),
                valid_values: None,
                default: None,
                maps_to: None,
            }],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec!["entity".to_owned()],
            invocation_phrases: vec!["set up depositary".to_owned()],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: Some(HarmClass::Reversible),
            action_class: Some(ActionClass::Assign),
            precondition_states: vec![],
            requires_subject: false,
            produces_focus: false,
            metadata: None,
            crud_mapping: None,
            reads_from: vec![],
            writes_to: vec![],
            outputs: vec![],
            produces_shared_facts: vec![],
        },
        VerbContractBody {
            fqn: "entity.create".to_owned(),
            domain: "entity".to_owned(),
            action: "create".to_owned(),
            description: "Create an entity".to_owned(),
            behavior: "crud".to_owned(),
            args: vec![VerbArgDef {
                name: "entity-name".to_owned(),
                arg_type: "string".to_owned(),
                required: true,
                description: None,
                lookup: None,
                valid_values: None,
                default: None,
                maps_to: None,
            }],
            returns: None,
            preconditions: vec![],
            postconditions: vec![],
            produces: None,
            consumes: vec![],
            invocation_phrases: vec!["create legal entity".to_owned()],
            subject_kinds: vec![],
            phase_tags: vec![],
            harm_class: Some(HarmClass::Reversible),
            action_class: Some(ActionClass::Create),
            precondition_states: vec![],
            requires_subject: false,
            produces_focus: false,
            metadata: None,
            crud_mapping: Some(VerbCrudMapping {
                operation: "insert".to_owned(),
                table: Some("entities".to_owned()),
                schema: Some("ob-poc".to_owned()),
                key_column: Some("entity_id".to_owned()),
                ..Default::default()
            }),
            reads_from: vec![],
            writes_to: vec!["ob-poc.entities".to_owned()],
            outputs: vec![],
            produces_shared_facts: vec![],
        },
    ]
}

fn affinity_graph() -> AffinityGraph {
    let edges = vec![
        AffinityEdge {
            verb_fqn: "cbu-role.assign".to_owned(),
            data_ref: DataRef::Table(TableRef::new("ob-poc", "entities")),
            affinity_kind: AffinityKind::ArgLookup {
                arg_name: "entity-id".to_owned(),
            },
            provenance: AffinityProvenance::VerbArgLookup,
        },
        AffinityEdge {
            verb_fqn: "entity.create".to_owned(),
            data_ref: DataRef::Table(TableRef::new("ob-poc", "entities")),
            affinity_kind: AffinityKind::CrudInsert,
            provenance: AffinityProvenance::VerbCrudMapping,
        },
    ];

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
        .verb_to_data
        .insert("entity.create".to_owned(), vec![1]);
    graph
        .data_to_verb
        .insert("table:ob-poc:entities".to_owned(), vec![0, 1]);
    graph.known_verbs.insert("cbu-role.assign".to_owned());
    graph.known_verbs.insert("entity.create".to_owned());

    graph
}

#[test]
fn discover_dsl_prefers_assign_and_synthesizes_entity_create_prereq() {
    let graph = affinity_graph();
    let verbs = verb_contracts();

    let discovery = discover_dsl("set up depositary", &graph, &verbs, None, 4, None, None);

    assert_eq!(discovery.intent_matches[0].verb, "cbu-role.assign");
    let chain_verbs: Vec<&str> = discovery
        .suggested_sequence
        .iter()
        .map(|s| s.verb.as_str())
        .collect();

    assert!(chain_verbs.contains(&"entity.create"));
    assert!(chain_verbs.contains(&"cbu-role.assign"));

    let assign_idx = chain_verbs
        .iter()
        .position(|v| *v == "cbu-role.assign")
        .expect("primary verb should exist");
    let create_idx = chain_verbs
        .iter()
        .position(|v| *v == "entity.create")
        .expect("prereq verb should exist");
    assert!(create_idx < assign_idx, "prereq must come before primary");
}
