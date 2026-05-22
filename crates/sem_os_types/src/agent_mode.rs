//! Agent mode gating for the Research → Governed boundary.
//!
//! | Mode       | Allowed                                   | Blocked                            |
//! |------------|-------------------------------------------|------------------------------------|
//! | Research   | Authoring verbs, full db_introspect,      | governance.*, maintenance.*,       |
//! |            | changeset.*, registry.*, schema.*,         | authoring.publish                  |
//! |            | focus.*, audit.*, agent.*                  |                                    |
//! | Governed   | Business verbs, publish, governance.*,    | authoring exploration verbs,       |
//! |            | maintenance.*, registry.*, schema.*,       | changeset.*                        |
//! |            | focus.*, audit.*, agent.*                  |                                    |

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

/// Agent operating mode for the Research → Governed boundary.
///
/// Default is `Governed`. Mode switch requires explicit `agent.set-execution-mode` verb
/// with confirmation.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    Display,
    EnumString,
    AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AgentMode {
    /// Research plane: exploration, schema introspection, ChangeSet authoring.
    ///
    /// Allowed: propose, validate, dry_run, plan, diff, full db_introspect,
    /// SemReg read/search tools.
    /// Blocked: governed business verbs (cbu.*, entity.*, trading-profile.*, etc.)
    Research,

    /// Governed plane: validated business operations per SemReg policy.
    ///
    /// Allowed: business verbs (per SemReg), publish, rollback,
    /// limited db_introspect (verify_table_exists, describe_table).
    /// Blocked: authoring verbs (propose, validate, dry_run, plan).
    #[default]
    Governed,

    /// Maintenance plane: SemOS infrastructure operations.
    ///
    /// Allowed: maintenance.*, governance.*, registry.*, schema.*, focus.*,
    /// audit.*, agent.*, changeset.*, authoring.* (full surface).
    /// Blocked: business verbs (cbu.*, entity.*, trading-profile.*, etc.)
    Maintenance,
}

impl AgentMode {
    /// Whether this mode allows authoring verbs (propose, validate, dry_run, plan).
    pub fn allows_authoring(&self) -> bool {
        matches!(self, AgentMode::Research | AgentMode::Maintenance)
    }

    /// Whether this mode allows full db_introspect surface.
    /// Governed mode restricts to verify_table_exists and describe_table only.
    pub fn allows_full_introspect(&self) -> bool {
        matches!(self, AgentMode::Research | AgentMode::Maintenance)
    }

    /// Whether this mode allows governed business verbs.
    pub fn allows_business_verbs(&self) -> bool {
        matches!(self, AgentMode::Governed)
    }

    /// Whether this mode allows maintenance verbs.
    pub fn allows_maintenance(&self) -> bool {
        matches!(self, AgentMode::Maintenance | AgentMode::Governed)
    }

    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "research" => Some(AgentMode::Research),
            "governed" => Some(AgentMode::Governed),
            "maintenance" => Some(AgentMode::Maintenance),
            _ => None,
        }
    }

    /// db_introspect subcommands allowed in this mode.
    pub fn allowed_introspect_subcommands(&self) -> &[&str] {
        match self {
            AgentMode::Research | AgentMode::Maintenance => &[
                "list_tables",
                "describe_table",
                "verify_table_exists",
                "list_foreign_keys",
                "list_indexes",
            ],
            AgentMode::Governed => &["verify_table_exists", "describe_table"],
        }
    }

    /// Authoring verb prefixes that are gated by mode.
    const AUTHORING_VERB_PREFIXES: &[&str] = &[
        "authoring.propose",
        "authoring.validate",
        "authoring.dry-run",
        "authoring.plan",
        "authoring.diff",
    ];

    /// Domain prefixes blocked in Research mode (governed-plane only).
    const GOVERNED_ONLY_PREFIXES: &[&str] = &["governance.", "maintenance."];

    /// Domain prefixes blocked in Governed mode (research-plane only).
    const RESEARCH_ONLY_PREFIXES: &[&str] = &["changeset."];

    /// Check if a verb FQN is allowed in this mode.
    ///
    /// Returns `true` if the verb is allowed, `false` if it should be blocked.
    /// Verbs not explicitly gated are always allowed.
    ///
    /// Domain gating rules:
    /// - `registry.*`, `schema.*`, `focus.*`, `audit.*`, `agent.*` — both modes (read-only / self-mgmt)
    /// - `changeset.*` — Research only (authoring), blocked in Governed
    /// - `governance.*`, `maintenance.*` — Governed only (pipeline/ops), blocked in Research
    /// - `authoring.*` — Research allows propose/validate/dry-run/plan/diff; Governed allows publish
    ///
    /// Domain prefixes blocked in Maintenance mode (business-plane only).
    const BUSINESS_ONLY_PREFIXES: &[&str] = &[
        "cbu.",
        "entity.",
        "trading-profile.",
        "investor.",
        "custody.",
        "deal.",
        "billing.",
        "contract.",
        "document.",
        "kyc.",
        "screening.",
        "screening-ops.",
        "ownership.",
    ];

    pub fn is_verb_allowed(&self, verb_fqn: &str) -> bool {
        let is_authoring = Self::AUTHORING_VERB_PREFIXES
            .iter()
            .any(|prefix| verb_fqn.starts_with(prefix));

        match self {
            AgentMode::Research => {
                // Research mode blocks publish/rollback
                if verb_fqn == "authoring.publish" || verb_fqn == "authoring.publish-batch" {
                    return false;
                }
                // Research mode blocks governed-only domain prefixes
                if Self::GOVERNED_ONLY_PREFIXES
                    .iter()
                    .any(|prefix| verb_fqn.starts_with(prefix))
                {
                    return false;
                }
                true
            }
            AgentMode::Governed => {
                // Governed mode blocks authoring exploration verbs
                if is_authoring {
                    return false;
                }
                // Governed mode blocks research-only domain prefixes
                if Self::RESEARCH_ONLY_PREFIXES
                    .iter()
                    .any(|prefix| verb_fqn.starts_with(prefix))
                {
                    return false;
                }
                true
            }
            AgentMode::Maintenance => {
                // Maintenance mode blocks business domain verbs
                if Self::BUSINESS_ONLY_PREFIXES
                    .iter()
                    .any(|prefix| verb_fqn.starts_with(prefix))
                {
                    return false;
                }
                // Maintenance allows everything else: authoring, governance,
                // maintenance, registry, schema, focus, audit, agent, changeset, nav
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_governed() {
        assert_eq!(AgentMode::default(), AgentMode::Governed);
    }

    #[test]
    fn test_research_allows_authoring() {
        let mode = AgentMode::Research;
        assert!(mode.allows_authoring());
        assert!(mode.allows_full_introspect());
        assert!(!mode.allows_business_verbs());
    }

    #[test]
    fn test_governed_blocks_authoring() {
        let mode = AgentMode::Governed;
        assert!(!mode.allows_authoring());
        assert!(!mode.allows_full_introspect());
        assert!(mode.allows_business_verbs());
    }

    #[test]
    fn test_verb_gating_research() {
        let mode = AgentMode::Research;
        assert!(mode.is_verb_allowed("authoring.propose"));
        assert!(mode.is_verb_allowed("authoring.validate"));
        assert!(mode.is_verb_allowed("authoring.dry-run"));
        assert!(mode.is_verb_allowed("authoring.diff"));
        // Research blocks publish (that's governed-only)
        assert!(!mode.is_verb_allowed("authoring.publish"));
        assert!(!mode.is_verb_allowed("authoring.publish-batch"));
        // Business verbs allowed (no hard block in research)
        assert!(mode.is_verb_allowed("cbu.create"));
        assert!(mode.is_verb_allowed("entity.create"));
    }

    #[test]
    fn test_verb_gating_governed() {
        let mode = AgentMode::Governed;
        // Governed blocks authoring exploration verbs
        assert!(!mode.is_verb_allowed("authoring.propose"));
        assert!(!mode.is_verb_allowed("authoring.validate"));
        assert!(!mode.is_verb_allowed("authoring.dry-run"));
        assert!(!mode.is_verb_allowed("authoring.diff"));
        // Governed allows publish
        assert!(mode.is_verb_allowed("authoring.publish"));
        assert!(mode.is_verb_allowed("authoring.publish-batch"));
        // Business verbs allowed
        assert!(mode.is_verb_allowed("cbu.create"));
        assert!(mode.is_verb_allowed("kyc-case.create"));
    }

    #[test]
    fn test_domain_gating_both_modes_allowed() {
        // registry, schema, focus, audit, agent — allowed in both modes
        let research = AgentMode::Research;
        let governed = AgentMode::Governed;

        for verb in &[
            "registry.describe-object",
            "registry.search",
            "schema.introspect",
            "schema.extract-attributes",
            "focus.get",
            "focus.set",
            "audit.create-plan",
            "audit.record-decision",
            "agent.read-mode",
            "agent.set-authoring-mode",
        ] {
            assert!(
                research.is_verb_allowed(verb),
                "Research should allow {verb}"
            );
            assert!(
                governed.is_verb_allowed(verb),
                "Governed should allow {verb}"
            );
        }
    }

    #[test]
    fn test_domain_gating_changeset_research_only() {
        let research = AgentMode::Research;
        let governed = AgentMode::Governed;

        for verb in &[
            "changeset.compose",
            "changeset.add-item",
            "changeset.remove-item",
            "changeset.refine-item",
            "changeset.list",
            "changeset.get",
            "changeset.diff",
        ] {
            assert!(
                research.is_verb_allowed(verb),
                "Research should allow {verb}"
            );
            assert!(
                !governed.is_verb_allowed(verb),
                "Governed should block {verb}"
            );
        }
    }

    #[test]
    fn test_domain_gating_governance_governed_only() {
        let research = AgentMode::Research;
        let governed = AgentMode::Governed;

        for verb in &[
            "governance.gate-precheck",
            "governance.submit-for-review",
            "governance.validate",
            "governance.dry-run",
            "governance.publish",
            "governance.publish-batch",
            "governance.rollback",
        ] {
            assert!(
                !research.is_verb_allowed(verb),
                "Research should block {verb}"
            );
            assert!(
                governed.is_verb_allowed(verb),
                "Governed should allow {verb}"
            );
        }
    }

    #[test]
    fn test_domain_gating_maintenance_governed_only() {
        let research = AgentMode::Research;
        let governed = AgentMode::Governed;

        for verb in &[
            "maintenance.health-pending",
            "maintenance.cleanup",
            "maintenance.bootstrap-seeds",
            "maintenance.reindex-embeddings",
            "maintenance.validate-schema-sync",
        ] {
            assert!(
                !research.is_verb_allowed(verb),
                "Research should block {verb}"
            );
            assert!(
                governed.is_verb_allowed(verb),
                "Governed should allow {verb}"
            );
        }
    }

    #[test]
    fn test_introspect_subcommands() {
        let research = AgentMode::Research;
        assert_eq!(research.allowed_introspect_subcommands().len(), 5);

        let governed = AgentMode::Governed;
        assert_eq!(governed.allowed_introspect_subcommands().len(), 2);
        assert!(governed
            .allowed_introspect_subcommands()
            .contains(&"verify_table_exists"));
        assert!(governed
            .allowed_introspect_subcommands()
            .contains(&"describe_table"));
    }

    #[test]
    fn test_parse() {
        assert_eq!(AgentMode::parse("research"), Some(AgentMode::Research));
        assert_eq!(AgentMode::parse("governed"), Some(AgentMode::Governed));
        assert_eq!(AgentMode::parse("Research"), Some(AgentMode::Research));
        assert_eq!(AgentMode::parse("GOVERNED"), Some(AgentMode::Governed));
        assert_eq!(AgentMode::parse("invalid"), None);
    }

    #[test]
    fn test_display() {
        assert_eq!(AgentMode::Research.to_string(), "research");
        assert_eq!(AgentMode::Governed.to_string(), "governed");
    }

    #[test]
    fn test_serde_roundtrip() {
        let research = AgentMode::Research;
        let json = serde_json::to_string(&research).unwrap();
        assert_eq!(json, "\"research\"");
        let back: AgentMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AgentMode::Research);

        let governed = AgentMode::Governed;
        let json = serde_json::to_string(&governed).unwrap();
        assert_eq!(json, "\"governed\"");
        let back: AgentMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AgentMode::Governed);
    }
}
