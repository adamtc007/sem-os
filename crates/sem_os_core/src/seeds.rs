//! SeedBundle — canonical, hashable bootstrap payload.
//! The adapter (sem_os_obpoc_adapter) builds a SeedBundle from ob-poc config;
//! the core crate owns the type and the hashing logic.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedBundle {
    /// SHA-256 of the canonical JSON serialisation of this bundle (fields sorted, deterministic).
    /// Computed by the adapter via `SeedBundle::compute_hash()`.
    /// Used as the idempotency key on POST /bootstrap/seed_bundle.
    /// Prefixed with "v1:" to allow future hash algorithm migration.
    pub bundle_hash: String,
    pub verb_contracts: Vec<VerbContractSeed>,
    #[serde(default)]
    pub macro_defs: Vec<MacroDefSeed>,
    #[serde(default)]
    pub universes: Vec<UniverseSeed>,
    #[serde(default)]
    pub constellation_families: Vec<ConstellationFamilySeed>,
    #[serde(default)]
    pub constellation_maps: Vec<ConstellationMapSeed>,
    #[serde(default)]
    pub state_machines: Vec<StateMachineSeed>,
    #[serde(default)]
    pub state_graphs: Vec<StateGraphSeed>,
    #[serde(default)]
    pub dag_taxonomies: Vec<DagTaxonomySeed>,
    #[serde(default)]
    pub domain_packs: Vec<DomainPackSeed>,
    pub attributes: Vec<AttributeSeed>,
    pub entity_types: Vec<EntityTypeSeed>,
    pub taxonomies: Vec<TaxonomySeed>,
    pub policies: Vec<PolicySeed>,
    pub views: Vec<ViewSeed>,
    #[serde(default)]
    pub derivation_specs: Vec<DerivationSpecSeed>,
    #[serde(default)]
    pub requirement_profiles: Vec<RequirementProfileSeed>,
    #[serde(default)]
    pub proof_obligations: Vec<ProofObligationSeed>,
    #[serde(default)]
    pub evidence_strategies: Vec<EvidenceStrategySeed>,
}

impl SeedBundle {
    /// Compute a stable, version-prefixed SHA-256 hash of the bundle contents.
    /// Sort all vecs by their FQN field before hashing to ensure determinism
    /// regardless of source ordering.
    ///
    /// Returns `Err` if canonical JSON serialization fails (should not happen
    /// in practice, but avoids a panic in production).
    ///
    /// # Examples
    /// ```rust
    /// use sem_os_core::seeds::SeedBundle;
    ///
    /// let bundle = SeedBundle {
    ///     bundle_hash: String::new(),
    ///     verb_contracts: vec![],
    ///     macro_defs: vec![],
    ///     universes: vec![],
    ///     constellation_families: vec![],
    ///     constellation_maps: vec![],
    ///     state_machines: vec![],
    ///     state_graphs: vec![],
    ///     dag_taxonomies: vec![],
    ///     domain_packs: vec![],
    ///     attributes: vec![],
    ///     entity_types: vec![],
    ///     taxonomies: vec![],
    ///     policies: vec![],
    ///     views: vec![],
    ///     derivation_specs: vec![],
    ///     requirement_profiles: vec![],
    ///     proof_obligations: vec![],
    ///     evidence_strategies: vec![],
    /// };
    ///
    /// let hash = SeedBundle::compute_hash(&bundle).unwrap();
    /// assert!(hash.starts_with("v1:"));
    /// ```
    pub fn compute_hash(bundle: &Self) -> std::result::Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Canonical<'a> {
            verb_contracts: Vec<&'a VerbContractSeed>,
            macro_defs: Vec<&'a MacroDefSeed>,
            universes: Vec<&'a UniverseSeed>,
            constellation_families: Vec<&'a ConstellationFamilySeed>,
            constellation_maps: Vec<&'a ConstellationMapSeed>,
            state_machines: Vec<&'a StateMachineSeed>,
            state_graphs: Vec<&'a StateGraphSeed>,
            dag_taxonomies: Vec<&'a DagTaxonomySeed>,
            domain_packs: Vec<&'a DomainPackSeed>,
            attributes: Vec<&'a AttributeSeed>,
            entity_types: Vec<&'a EntityTypeSeed>,
            taxonomies: Vec<&'a TaxonomySeed>,
            policies: Vec<&'a PolicySeed>,
            views: Vec<&'a ViewSeed>,
            derivation_specs: Vec<&'a DerivationSpecSeed>,
            requirement_profiles: Vec<&'a RequirementProfileSeed>,
            proof_obligations: Vec<&'a ProofObligationSeed>,
            evidence_strategies: Vec<&'a EvidenceStrategySeed>,
        }

        let mut vc: Vec<&VerbContractSeed> = bundle.verb_contracts.iter().collect();
        vc.sort_by_key(|s| &s.fqn);

        let mut md: Vec<&MacroDefSeed> = bundle.macro_defs.iter().collect();
        md.sort_by_key(|s| &s.fqn);

        let mut un: Vec<&UniverseSeed> = bundle.universes.iter().collect();
        un.sort_by_key(|s| &s.fqn);

        let mut cf: Vec<&ConstellationFamilySeed> = bundle.constellation_families.iter().collect();
        cf.sort_by_key(|s| &s.fqn);

        let mut cm: Vec<&ConstellationMapSeed> = bundle.constellation_maps.iter().collect();
        cm.sort_by_key(|s| &s.fqn);

        let mut sm: Vec<&StateMachineSeed> = bundle.state_machines.iter().collect();
        sm.sort_by_key(|s| &s.fqn);

        let mut sg: Vec<&StateGraphSeed> = bundle.state_graphs.iter().collect();
        sg.sort_by_key(|s| &s.fqn);

        let mut dt: Vec<&DagTaxonomySeed> = bundle.dag_taxonomies.iter().collect();
        dt.sort_by_key(|s| &s.fqn);

        let mut dp: Vec<&DomainPackSeed> = bundle.domain_packs.iter().collect();
        dp.sort_by_key(|s| &s.fqn);

        let mut at: Vec<&AttributeSeed> = bundle.attributes.iter().collect();
        at.sort_by_key(|s| &s.fqn);

        let mut et: Vec<&EntityTypeSeed> = bundle.entity_types.iter().collect();
        et.sort_by_key(|s| &s.fqn);

        let mut tx: Vec<&TaxonomySeed> = bundle.taxonomies.iter().collect();
        tx.sort_by_key(|s| &s.fqn);

        let mut ps: Vec<&PolicySeed> = bundle.policies.iter().collect();
        ps.sort_by_key(|s| &s.fqn);

        let mut vi: Vec<&ViewSeed> = bundle.views.iter().collect();
        vi.sort_by_key(|s| &s.fqn);

        let mut ds: Vec<&DerivationSpecSeed> = bundle.derivation_specs.iter().collect();
        ds.sort_by_key(|s| &s.fqn);

        let mut rp: Vec<&RequirementProfileSeed> = bundle.requirement_profiles.iter().collect();
        rp.sort_by_key(|s| &s.fqn);

        let mut pob: Vec<&ProofObligationSeed> = bundle.proof_obligations.iter().collect();
        pob.sort_by_key(|s| &s.fqn);

        let mut es: Vec<&EvidenceStrategySeed> = bundle.evidence_strategies.iter().collect();
        es.sort_by_key(|s| &s.fqn);

        let canonical = Canonical {
            verb_contracts: vc,
            macro_defs: md,
            universes: un,
            constellation_families: cf,
            constellation_maps: cm,
            state_machines: sm,
            state_graphs: sg,
            dag_taxonomies: dt,
            domain_packs: dp,
            attributes: at,
            entity_types: et,
            taxonomies: tx,
            policies: ps,
            views: vi,
            derivation_specs: ds,
            requirement_profiles: rp,
            proof_obligations: pob,
            evidence_strategies: es,
        };
        let json = serde_json::to_string(&canonical)?;
        let hash = Sha256::digest(json.as_bytes());
        Ok(format!("v1:{}", hex::encode(hash)))
    }
}

// Seed DTOs — pure data, no SQLx derives, no ob-poc config types.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbContractSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDefSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstellationFamilySeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstellationMapSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachineSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateGraphSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagTaxonomySeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainPackSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTypeSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomySeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivationSpecSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementProfileSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofObligationSeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStrategySeed {
    pub fqn: String,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_verb(fqn: &str) -> VerbContractSeed {
        VerbContractSeed {
            fqn: fqn.into(),
            payload: json!({"desc": fqn}),
        }
    }

    fn make_attr(fqn: &str) -> AttributeSeed {
        AttributeSeed {
            fqn: fqn.into(),
            payload: json!({"type": "string"}),
        }
    }

    fn empty_hash(verbs: &[VerbContractSeed], attrs: &[AttributeSeed]) -> String {
        SeedBundle::compute_hash(&SeedBundle {
            bundle_hash: String::new(),
            verb_contracts: verbs.to_vec(),
            macro_defs: vec![],
            universes: vec![],
            constellation_families: vec![],
            constellation_maps: vec![],
            state_machines: vec![],
            state_graphs: vec![],
            dag_taxonomies: vec![],
            domain_packs: vec![],
            attributes: attrs.to_vec(),
            entity_types: vec![],
            taxonomies: vec![],
            policies: vec![],
            views: vec![],
            derivation_specs: vec![],
            requirement_profiles: vec![],
            proof_obligations: vec![],
            evidence_strategies: vec![],
        })
        .unwrap()
    }

    #[test]
    fn compute_hash_determinism() {
        let verbs = vec![make_verb("cbu.create"), make_verb("cbu.delete")];
        let attrs = vec![make_attr("cbu.name")];
        let h1 = empty_hash(&verbs, &attrs);
        let h2 = empty_hash(&verbs, &attrs);
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_hash_order_independence() {
        let v_ab = vec![make_verb("a.verb"), make_verb("b.verb")];
        let v_ba = vec![make_verb("b.verb"), make_verb("a.verb")];
        let h_ab = empty_hash(&v_ab, &[]);
        let h_ba = empty_hash(&v_ba, &[]);
        assert_eq!(h_ab, h_ba, "hash must be order-independent (sorted by fqn)");
    }

    #[test]
    fn compute_hash_has_v1_prefix() {
        let h = empty_hash(&[], &[]);
        assert!(h.starts_with("v1:"), "expected v1: prefix, got: {h}");
        // SHA-256 hex = 64 chars, plus "v1:" = 67
        assert_eq!(h.len(), 67);
    }

    #[test]
    fn compute_hash_changes_with_content() {
        let h_empty = empty_hash(&[], &[]);
        let h_one = empty_hash(&[make_verb("x.y")], &[]);
        assert_ne!(
            h_empty, h_one,
            "different content must produce different hash"
        );
    }

    #[test]
    fn seed_bundle_serde_round_trip() {
        let bundle = SeedBundle {
            bundle_hash: "v1:abc123".into(),
            verb_contracts: vec![make_verb("cbu.create")],
            macro_defs: vec![],
            universes: vec![],
            constellation_families: vec![],
            constellation_maps: vec![],
            state_machines: vec![],
            state_graphs: vec![],
            dag_taxonomies: vec![],
            domain_packs: vec![],
            attributes: vec![make_attr("cbu.name")],
            entity_types: vec![EntityTypeSeed {
                fqn: "cbu".into(),
                payload: json!({}),
            }],
            taxonomies: vec![TaxonomySeed {
                fqn: "domain.kyc".into(),
                payload: json!({}),
            }],
            policies: vec![PolicySeed {
                fqn: "p.rule".into(),
                payload: json!({}),
            }],
            views: vec![ViewSeed {
                fqn: "v.default".into(),
                payload: json!({}),
            }],
            derivation_specs: vec![DerivationSpecSeed {
                fqn: "d.spec".into(),
                payload: json!({}),
            }],
            requirement_profiles: vec![RequirementProfileSeed {
                fqn: "doc.requirement_profile.test".into(),
                payload: json!({}),
            }],
            proof_obligations: vec![ProofObligationSeed {
                fqn: "doc.proof_obligation.test".into(),
                payload: json!({}),
            }],
            evidence_strategies: vec![EvidenceStrategySeed {
                fqn: "doc.evidence_strategy.test".into(),
                payload: json!({}),
            }],
        };
        let json_str = serde_json::to_string(&bundle).unwrap();
        let restored: SeedBundle = serde_json::from_str(&json_str).unwrap();
        assert_eq!(restored.bundle_hash, bundle.bundle_hash);
        assert_eq!(restored.verb_contracts.len(), 1);
        assert!(restored.macro_defs.is_empty());
        assert!(restored.universes.is_empty());
        assert!(restored.constellation_families.is_empty());
        assert!(restored.constellation_maps.is_empty());
        assert!(restored.state_machines.is_empty());
        assert!(restored.state_graphs.is_empty());
        assert!(restored.dag_taxonomies.is_empty());
        assert!(restored.domain_packs.is_empty());
        assert_eq!(restored.attributes.len(), 1);
        assert_eq!(restored.entity_types.len(), 1);
        assert_eq!(restored.taxonomies.len(), 1);
        assert_eq!(restored.policies.len(), 1);
        assert_eq!(restored.views.len(), 1);
        assert_eq!(restored.derivation_specs.len(), 1);
        assert_eq!(restored.requirement_profiles.len(), 1);
        assert_eq!(restored.proof_obligations.len(), 1);
        assert_eq!(restored.evidence_strategies.len(), 1);
    }

    #[test]
    fn individual_seed_types_serde_round_trip() {
        let seeds: Vec<serde_json::Value> = vec![
            serde_json::to_value(VerbContractSeed {
                fqn: "v.c".into(),
                payload: json!(1),
            })
            .unwrap(),
            serde_json::to_value(AttributeSeed {
                fqn: "a.s".into(),
                payload: json!(2),
            })
            .unwrap(),
            serde_json::to_value(MacroDefSeed {
                fqn: "m.d".into(),
                payload: json!(20),
            })
            .unwrap(),
            serde_json::to_value(ConstellationMapSeed {
                fqn: "c.m".into(),
                payload: json!(21),
            })
            .unwrap(),
            serde_json::to_value(StateMachineSeed {
                fqn: "s.m".into(),
                payload: json!(22),
            })
            .unwrap(),
            serde_json::to_value(StateGraphSeed {
                fqn: "s.g".into(),
                payload: json!(23),
            })
            .unwrap(),
            serde_json::to_value(DagTaxonomySeed {
                fqn: "d.t".into(),
                payload: json!(24),
            })
            .unwrap(),
            serde_json::to_value(DomainPackSeed {
                fqn: "d.p".into(),
                payload: json!(25),
            })
            .unwrap(),
            serde_json::to_value(EntityTypeSeed {
                fqn: "e.t".into(),
                payload: json!(3),
            })
            .unwrap(),
            serde_json::to_value(TaxonomySeed {
                fqn: "t.x".into(),
                payload: json!(4),
            })
            .unwrap(),
            serde_json::to_value(PolicySeed {
                fqn: "p.r".into(),
                payload: json!(5),
            })
            .unwrap(),
            serde_json::to_value(ViewSeed {
                fqn: "v.d".into(),
                payload: json!(6),
            })
            .unwrap(),
            serde_json::to_value(DerivationSpecSeed {
                fqn: "d.s".into(),
                payload: json!(7),
            })
            .unwrap(),
            serde_json::to_value(RequirementProfileSeed {
                fqn: "rp.s".into(),
                payload: json!(8),
            })
            .unwrap(),
            serde_json::to_value(ProofObligationSeed {
                fqn: "po.s".into(),
                payload: json!(9),
            })
            .unwrap(),
            serde_json::to_value(EvidenceStrategySeed {
                fqn: "es.s".into(),
                payload: json!(10),
            })
            .unwrap(),
        ];
        // Deserialize each back to its concrete type
        let _: VerbContractSeed = serde_json::from_value(seeds[0].clone()).unwrap();
        let _: AttributeSeed = serde_json::from_value(seeds[1].clone()).unwrap();
        let _: EntityTypeSeed = serde_json::from_value(seeds[2].clone()).unwrap();
        let _: TaxonomySeed = serde_json::from_value(seeds[3].clone()).unwrap();
        let _: PolicySeed = serde_json::from_value(seeds[4].clone()).unwrap();
        let _: ViewSeed = serde_json::from_value(seeds[5].clone()).unwrap();
        let _: DerivationSpecSeed = serde_json::from_value(seeds[6].clone()).unwrap();
        let _: RequirementProfileSeed = serde_json::from_value(seeds[7].clone()).unwrap();
        let _: ProofObligationSeed = serde_json::from_value(seeds[8].clone()).unwrap();
        let _: EvidenceStrategySeed = serde_json::from_value(seeds[9].clone()).unwrap();
    }
}
