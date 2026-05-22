//! Publish gate pure functions for the Semantic Registry.
//!
//! Gates are evaluated before any snapshot is persisted. They enforce:
//! - **Proof Rule**: Operational tier cannot have TrustClass::Proof
//! - **Security label validation**: Classification level must be valid
//! - **Governed approval**: Governed-tier snapshots require an approver
//! - **Version monotonicity**: New versions must be >= predecessor

pub mod governance;
pub mod technical;

use sem_os_types::{
    Classification, GovernanceTier, SecurityLabel, SnapshotMeta, SnapshotRow, TrustClass,
};

/// Result of a single gate check.
#[derive(Debug, Clone)]
#[must_use]
pub struct GateResult {
    pub gate_name: &'static str,
    pub passed: bool,
    pub reason: Option<String>,
}

impl GateResult {
    fn pass(gate_name: &'static str) -> Self {
        Self {
            gate_name,
            passed: true,
            reason: None,
        }
    }

    fn fail(gate_name: &'static str, reason: impl Into<String>) -> Self {
        Self {
            gate_name,
            passed: false,
            reason: Some(reason.into()),
        }
    }
}

/// Aggregated result of all publish gates.
#[derive(Debug, Clone)]
pub struct PublishGateResult {
    pub results: Vec<GateResult>,
}

impl PublishGateResult {
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }

    pub fn failures(&self) -> Vec<&GateResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    pub fn failure_messages(&self) -> Vec<String> {
        self.failures()
            .iter()
            .filter_map(|r| {
                r.reason
                    .as_ref()
                    .map(|msg| format!("[{}] {}", r.gate_name, msg))
            })
            .collect()
    }
}

// ── Individual gate checks ────────────────────────────────────

/// **Proof Rule**: Only governed-tier objects may have TrustClass::Proof.
/// This mirrors the DB CHECK constraint: `trust_class != 'proof' OR governance_tier = 'governed'`
pub fn check_proof_rule(tier: GovernanceTier, trust: TrustClass) -> GateResult {
    if trust == TrustClass::Proof && tier == GovernanceTier::Operational {
        GateResult::fail(
            "proof_rule",
            "Operational-tier objects cannot have TrustClass::Proof — \
             promote to Governed tier first",
        )
    } else {
        GateResult::pass("proof_rule")
    }
}

/// **Security label validation**: Ensure the classification is populated.
pub fn check_security_label(label: &SecurityLabel) -> GateResult {
    // PII data must have at least Confidential classification
    if label.pii
        && matches!(
            label.classification,
            Classification::Public | Classification::Internal
        )
    {
        return GateResult::fail(
            "security_label",
            "PII-flagged objects must have classification >= Confidential",
        );
    }

    GateResult::pass("security_label")
}

/// **Governed approval gate**: Governed-tier snapshots must have an approver.
pub fn check_governed_approval(meta: &SnapshotMeta) -> GateResult {
    if meta.governance_tier == GovernanceTier::Governed && meta.approved_by.is_none() {
        GateResult::fail(
            "governed_approval",
            "Governed-tier snapshots require an approver",
        )
    } else {
        GateResult::pass("governed_approval")
    }
}

/// **Version monotonicity**: New version must be >= predecessor version.
pub fn check_version_monotonicity(
    meta: &SnapshotMeta,
    predecessor: Option<&SnapshotRow>,
) -> GateResult {
    if let Some(pred) = predecessor {
        let new_version = (meta.version_major, meta.version_minor);
        let old_version = (pred.version_major, pred.version_minor);
        if new_version < old_version {
            return GateResult::fail(
                "version_monotonicity",
                format!(
                    "New version {}.{} is less than predecessor {}.{}",
                    meta.version_major, meta.version_minor, pred.version_major, pred.version_minor,
                ),
            );
        }
    }
    GateResult::pass("version_monotonicity")
}

// ── Aggregate evaluator ───────────────────────────────────────

/// Evaluate all publish gates for a snapshot.
///
/// Returns a `PublishGateResult` containing the outcome of every gate.
/// The caller should check `all_passed()` before persisting.
pub fn evaluate_publish_gates(
    meta: &SnapshotMeta,
    predecessor: Option<&SnapshotRow>,
) -> PublishGateResult {
    let results = vec![
        check_proof_rule(meta.governance_tier, meta.trust_class),
        check_security_label(&meta.security_label),
        check_governed_approval(meta),
        check_version_monotonicity(meta, predecessor),
    ];
    PublishGateResult { results }
}

// ── Unified gate evaluator ─────────────────────────────────────

/// Context for running the full extended gate suite.
///
/// Callers populate only the fields relevant to the snapshot being published.
/// Missing context fields are skipped gracefully (gates that need them produce
/// no failures rather than panicking).
#[derive(Default)]
pub struct ExtendedGateContext {
    /// Predecessor snapshot for version/integrity checks.
    pub predecessor: Option<SnapshotRow>,
    /// Taxonomy memberships for this object.
    pub memberships: Vec<String>,
    /// Known verb FQNs in the registry (for macro expansion checks).
    pub known_verb_fqns: std::collections::HashSet<String>,
    /// Current timestamp for review cycle checks.
    pub now: Option<chrono::DateTime<chrono::Utc>>,
}

/// Run all extended gates (technical + governance) against a snapshot.
///
/// Returns a `Vec<GateFailure>` — empty means all gates passed.
pub fn evaluate_extended_gates(
    snapshot: &SnapshotRow,
    ctx: &ExtendedGateContext,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();
    let tier = snapshot.governance_tier;

    // ── Technical gates ──────────────────────────────────────

    // T3: Security label presence
    failures.extend(technical::check_security_label_presence(snapshot));

    // T5: Snapshot integrity (predecessor checks)
    failures.extend(technical::check_snapshot_integrity(
        snapshot,
        ctx.predecessor.as_ref(),
    ));

    // ── Governance gates ─────────────────────────────────────

    // G1: Taxonomy membership
    let fqn = snapshot
        .definition
        .get("fqn")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    failures.extend(governance::check_taxonomy_membership(
        fqn,
        tier,
        &ctx.memberships,
    ));

    // G2: Stewardship
    failures.extend(governance::check_stewardship(snapshot, tier));

    // G4: Regulatory linkage
    failures.extend(governance::check_regulatory_linkage(snapshot, tier));

    // G5: Review cycle compliance
    let now = ctx.now.unwrap_or_else(chrono::Utc::now);
    failures.extend(governance::check_review_cycle_compliance(
        snapshot, tier, now,
    ));

    // G6: Version consistency (strict — requires > not >=)
    failures.extend(governance::check_version_consistency(
        snapshot,
        ctx.predecessor.as_ref(),
    ));

    // G7: Continuation completeness
    failures.extend(governance::check_continuation_completeness(snapshot));

    // G8: Macro expansion integrity (verb contracts only)
    if snapshot.object_type == sem_os_types::ObjectType::VerbContract {
        let verb_fqn = snapshot
            .definition
            .get("fqn")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        failures.extend(governance::check_macro_expansion_integrity(
            &snapshot.definition,
            verb_fqn,
            &ctx.known_verb_fqns,
        ));
    }

    failures
}

/// Evaluate ALL publish gates — both the simple 4-gate pipeline and extended gates.
///
/// Returns a unified `UnifiedPublishGateResult` that merges both frameworks.
/// A publish is blocked if:
/// - Any simple gate fails, OR
/// - Any extended gate has `GateSeverity::Error` in `GateMode::Enforce`
pub fn evaluate_all_publish_gates(
    meta: &SnapshotMeta,
    snapshot: &SnapshotRow,
    ctx: &ExtendedGateContext,
    mode: GateMode,
) -> UnifiedPublishGateResult {
    // Run the simple 4-gate pipeline
    let simple = evaluate_publish_gates(meta, ctx.predecessor.as_ref());

    // Run the extended gate suite
    let extended_failures = evaluate_extended_gates(snapshot, ctx);

    let extended = ExtendedPublishGateResult {
        failures: extended_failures,
        mode,
    };

    UnifiedPublishGateResult { simple, extended }
}

/// Unified result merging both simple and extended gate frameworks.
#[derive(Debug, Clone)]
pub struct UnifiedPublishGateResult {
    pub simple: PublishGateResult,
    pub extended: ExtendedPublishGateResult,
}

impl UnifiedPublishGateResult {
    /// Should this result block a publish?
    pub fn should_block(&self) -> bool {
        !self.simple.all_passed() || self.extended.should_block()
    }

    /// All failure messages from both frameworks.
    pub fn all_failure_messages(&self) -> Vec<String> {
        let mut msgs = self.simple.failure_messages();
        for f in &self.extended.failures {
            if f.severity == GateSeverity::Error {
                let fqn = f.object_fqn.as_deref().unwrap_or("unknown");
                msgs.push(format!("[{}] ({}) {}", f.gate_name, fqn, f.message));
            }
        }
        msgs
    }

    /// Total error count across both frameworks.
    pub fn error_count(&self) -> usize {
        self.simple.failures().len()
            + self
                .extended
                .failures
                .iter()
                .filter(|f| f.severity == GateSeverity::Error)
                .count()
    }

    /// Total warning count (extended only — simple has no warnings).
    pub fn warning_count(&self) -> usize {
        self.extended
            .failures
            .iter()
            .filter(|f| f.severity == GateSeverity::Warning)
            .count()
    }
}

// ── Phase 3: Evidence-specific gates ─────────────────────────────

/// Check that an evidence requirement referencing a Proof-class attribute
/// is itself at the Governed tier.
///
/// This enforces the invariant: operational-tier evidence cannot
/// substantiate a Proof-class claim.
pub fn check_evidence_proof_rule(
    evidence_tier: GovernanceTier,
    referenced_attribute_trust_class: TrustClass,
) -> GateResult {
    if referenced_attribute_trust_class == TrustClass::Proof
        && evidence_tier != GovernanceTier::Governed
    {
        GateResult::fail(
            "evidence_proof_rule",
            "Evidence requirement referencing a Proof-class attribute must be Governed tier",
        )
    } else {
        GateResult::pass("evidence_proof_rule")
    }
}

// ── Phase 6: Extended gate framework ─────────────────────────

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Gate enforcement mode — determines whether failures block or warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateMode {
    /// Hard-fail: blocks publish.
    Enforce,
    /// Soft: emits warnings, does not block.
    ReportOnly,
}

// `GateSeverity` lives in `sem_os_types::GateSeverity`; re-exported here for
// backwards compatibility with existing call sites under `gates::GateSeverity`.
pub use sem_os_types::GateSeverity;

/// Structured gate failure with metadata for audit and remediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateFailure {
    pub gate_name: String,
    pub severity: GateSeverity,
    pub object_type: String,
    pub object_fqn: Option<String>,
    pub snapshot_id: Option<Uuid>,
    pub message: String,
    pub remediation_hint: Option<String>,
}

impl GateFailure {
    /// Create an error-level gate failure.
    pub fn error(
        gate_name: impl Into<String>,
        object_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            gate_name: gate_name.into(),
            severity: GateSeverity::Error,
            object_type: object_type.into(),
            object_fqn: None,
            snapshot_id: None,
            message: message.into(),
            remediation_hint: None,
        }
    }

    /// Create a warning-level gate failure.
    pub fn warning(
        gate_name: impl Into<String>,
        object_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            gate_name: gate_name.into(),
            severity: GateSeverity::Warning,
            object_type: object_type.into(),
            object_fqn: None,
            snapshot_id: None,
            message: message.into(),
            remediation_hint: None,
        }
    }

    /// Builder: set the object FQN.
    pub fn with_fqn(mut self, fqn: impl Into<String>) -> Self {
        self.object_fqn = Some(fqn.into());
        self
    }

    /// Builder: set the snapshot ID.
    pub fn with_snapshot_id(mut self, id: Uuid) -> Self {
        self.snapshot_id = Some(id);
        self
    }

    /// Builder: set a remediation hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.remediation_hint = Some(hint.into());
        self
    }
}

/// Extended publish gate result with mode control.
#[derive(Debug, Clone)]
pub struct ExtendedPublishGateResult {
    pub failures: Vec<GateFailure>,
    pub mode: GateMode,
}

impl ExtendedPublishGateResult {
    /// Are there any error-level failures?
    pub fn has_errors(&self) -> bool {
        self.failures
            .iter()
            .any(|f| f.severity == GateSeverity::Error)
    }

    /// Are there any warning-level failures?
    pub fn has_warnings(&self) -> bool {
        self.failures
            .iter()
            .any(|f| f.severity == GateSeverity::Warning)
    }

    /// Should this result block a publish?
    pub fn should_block(&self) -> bool {
        self.has_errors() && self.mode == GateMode::Enforce
    }

    /// Human-readable failure report.
    pub fn failure_report(&self) -> String {
        if self.failures.is_empty() {
            return "All gates passed.".into();
        }
        self.failures
            .iter()
            .map(|f| {
                let severity = match f.severity {
                    GateSeverity::Error => "ERROR",
                    GateSeverity::Warning => "WARN",
                };
                let fqn = f.object_fqn.as_deref().unwrap_or("unknown");
                format!("[{}] {} ({}): {}", severity, f.gate_name, fqn, f.message)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Machine-readable JSON output.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": format!("{:?}", self.mode).to_lowercase(),
            "blocked": self.should_block(),
            "error_count": self.failures.iter().filter(|f| f.severity == GateSeverity::Error).count(),
            "warning_count": self.failures.iter().filter(|f| f.severity == GateSeverity::Warning).count(),
            "failures": self.failures,
        })
    }
}

// ── Phase 5: Derivation-specific gates ───────────────────────

use sem_os_ontology::derivation_spec::DerivationSpecBody;
use sem_os_types::EvidenceGrade;

/// Check for cycles in a set of derivation specs.
///
/// Builds an input→output graph and checks for cycles via topological sort.
pub fn check_derivation_cycle(specs: &[DerivationSpecBody]) -> Vec<GateFailure> {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Build adjacency: output_fqn → set of input_fqns
    let mut output_to_spec: HashMap<&str, &DerivationSpecBody> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for spec in specs {
        output_to_spec.insert(&spec.output_attribute_fqn, spec);
        let inputs: Vec<&str> = spec
            .inputs
            .iter()
            .map(|i| i.attribute_fqn.as_str())
            .collect();
        adj.insert(spec.output_attribute_fqn.as_str(), inputs);
    }

    // Kahn's algorithm for cycle detection.
    // Edge direction: input → output (output depends on inputs).
    let all_nodes: HashSet<&str> = adj
        .keys()
        .copied()
        .chain(adj.values().flat_map(|v| v.iter().copied()))
        .collect();

    let mut in_degree: HashMap<&str, usize> = all_nodes.iter().map(|&n| (n, 0)).collect();
    let mut reverse_adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for (output, inputs) in &adj {
        // edge: each input → output
        for input in inputs {
            reverse_adj.entry(input).or_default().push(output);
        }
        // output has in_degree = number of inputs
        *in_degree.entry(output).or_default() = inputs.len();
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&n, _)| n)
        .collect();

    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(dependents) = reverse_adj.get(node) {
            for &dep in dependents {
                if let Some(count) = in_degree.get_mut(dep) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }
    }

    if visited < all_nodes.len() {
        // Cycle detected — find which specs are involved
        let cycle_nodes: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&n, _)| n)
            .collect();

        let cycle_specs: Vec<String> = cycle_nodes
            .iter()
            .filter_map(|n| output_to_spec.get(n).map(|s| s.fqn.clone()))
            .collect();

        vec![GateFailure::error(
            "derivation_cycle",
            "derivation_spec",
            format!(
                "Cycle detected in derivation graph involving: {}",
                cycle_specs.join(", ")
            ),
        )]
    } else {
        vec![]
    }
}

/// Check that operational derivations do not declare evidence-bearing grades.
///
/// Invariant: operational-tier derivations cannot be used as regulatory evidence.
pub fn check_derivation_evidence_grade(
    spec: &DerivationSpecBody,
    tier: GovernanceTier,
) -> Vec<GateFailure> {
    if tier == GovernanceTier::Operational
        && matches!(
            spec.evidence_grade,
            EvidenceGrade::AllowedWithConstraints | EvidenceGrade::RegulatoryEvidence
        )
    {
        vec![GateFailure::error(
            "derivation_evidence_grade",
            "derivation_spec",
            format!(
                "Derivation '{}' is operational-tier but has evidence_grade '{}' — \
                 promote to Governed tier or set evidence_grade to prohibited/none",
                spec.fqn, spec.evidence_grade
            ),
        )
        .with_fqn(&spec.fqn)]
    } else {
        vec![]
    }
}

/// Check that derivation inputs/outputs reference known attributes.
///
/// Validates that the output and all input attribute FQNs exist in the
/// provided set of known attribute FQNs.
pub fn check_derivation_type_compatibility(
    spec: &DerivationSpecBody,
    known_attribute_fqns: &std::collections::HashSet<String>,
) -> Vec<GateFailure> {
    let mut failures = Vec::new();

    if !known_attribute_fqns.contains(&spec.output_attribute_fqn) {
        failures.push(
            GateFailure::error(
                "derivation_type_compat",
                "derivation_spec",
                format!(
                    "Derivation '{}' output attribute '{}' not found in registry",
                    spec.fqn, spec.output_attribute_fqn
                ),
            )
            .with_fqn(&spec.fqn)
            .with_hint("Publish the output attribute definition first"),
        );
    }

    for input in &spec.inputs {
        if !known_attribute_fqns.contains(&input.attribute_fqn) {
            failures.push(
                GateFailure::error(
                    "derivation_type_compat",
                    "derivation_spec",
                    format!(
                        "Derivation '{}' input attribute '{}' not found in registry",
                        spec.fqn, input.attribute_fqn
                    ),
                )
                .with_fqn(&spec.fqn)
                .with_hint("Publish the input attribute definition first"),
            );
        }
    }

    failures
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sem_os_types::*;
    use uuid::Uuid;

    #[test]
    fn test_proof_rule_operational_proof_fails() {
        let result = check_proof_rule(GovernanceTier::Operational, TrustClass::Proof);
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Operational"));
    }

    #[test]
    fn test_proof_rule_governed_proof_passes() {
        let result = check_proof_rule(GovernanceTier::Governed, TrustClass::Proof);
        assert!(result.passed);
    }

    #[test]
    fn test_proof_rule_operational_convenience_passes() {
        let result = check_proof_rule(GovernanceTier::Operational, TrustClass::Convenience);
        assert!(result.passed);
    }

    #[test]
    fn test_proof_rule_operational_decision_support_passes() {
        let result = check_proof_rule(GovernanceTier::Operational, TrustClass::DecisionSupport);
        assert!(result.passed);
    }

    #[test]
    fn test_security_label_pii_requires_confidential() {
        let label = SecurityLabel {
            pii: true,
            classification: Classification::Internal,
            ..SecurityLabel::default()
        };
        let result = check_security_label(&label);
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Confidential"));
    }

    #[test]
    fn test_security_label_pii_confidential_passes() {
        let label = SecurityLabel {
            pii: true,
            classification: Classification::Confidential,
            ..SecurityLabel::default()
        };
        let result = check_security_label(&label);
        assert!(result.passed);
    }

    #[test]
    fn test_security_label_no_pii_public_passes() {
        let label = SecurityLabel {
            pii: false,
            classification: Classification::Public,
            ..SecurityLabel::default()
        };
        let result = check_security_label(&label);
        assert!(result.passed);
    }

    #[test]
    fn test_governed_approval_required() {
        let meta = SnapshotMeta {
            object_type: ObjectType::PolicyRule,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Proof,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "analyst".into(),
            approved_by: None, // missing!
            predecessor_id: None,
        };
        let result = check_governed_approval(&meta);
        assert!(!result.passed);
    }

    #[test]
    fn test_governed_approval_present_passes() {
        let meta = SnapshotMeta {
            object_type: ObjectType::PolicyRule,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Proof,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "analyst".into(),
            approved_by: Some("supervisor".into()),
            predecessor_id: None,
        };
        let result = check_governed_approval(&meta);
        assert!(result.passed);
    }

    #[test]
    fn test_operational_no_approval_needed() {
        let meta =
            SnapshotMeta::new_operational(ObjectType::VerbContract, Uuid::new_v4(), "scanner");
        let result = check_governed_approval(&meta);
        assert!(result.passed);
    }

    #[test]
    fn test_version_monotonicity_pass() {
        let meta = SnapshotMeta {
            version_major: 2,
            version_minor: 0,
            ..SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "test")
        };
        let pred = mock_predecessor(1, 3);
        let result = check_version_monotonicity(&meta, Some(&pred));
        assert!(result.passed);
    }

    #[test]
    fn test_version_monotonicity_fail() {
        let meta = SnapshotMeta {
            version_major: 1,
            version_minor: 0,
            ..SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "test")
        };
        let pred = mock_predecessor(2, 0);
        let result = check_version_monotonicity(&meta, Some(&pred));
        assert!(!result.passed);
    }

    #[test]
    fn test_version_monotonicity_no_predecessor() {
        let meta = SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "test");
        let result = check_version_monotonicity(&meta, None);
        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_all_gates_pass() {
        let meta = SnapshotMeta {
            object_type: ObjectType::VerbContract,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Governed,
            trust_class: TrustClass::Proof,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "analyst".into(),
            approved_by: Some("supervisor".into()),
            predecessor_id: None,
        };
        let gate = evaluate_publish_gates(&meta, None);
        assert!(gate.all_passed(), "Failures: {:?}", gate.failure_messages());
    }

    #[test]
    fn test_evaluate_gates_multiple_failures() {
        let meta = SnapshotMeta {
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Proof, // violates proof rule
            security_label: SecurityLabel {
                pii: true,
                classification: Classification::Public, // violates PII rule
                ..SecurityLabel::default()
            },
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            predecessor_id: None,
        };
        let gate = evaluate_publish_gates(&meta, None);
        assert!(!gate.all_passed());
        assert_eq!(gate.failures().len(), 2);
    }

    // Helper to construct a mock predecessor row
    fn mock_predecessor(major: i32, minor: i32) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: major,
            version_minor: minor,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({}),
            effective_from: chrono::Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            definition: serde_json::json!({}),
            created_at: chrono::Utc::now(),
        }
    }

    // ── Phase 3: Evidence Proof Rule tests ─────────────────────

    #[test]
    fn test_evidence_proof_rule_governed_proof_passes() {
        let result = check_evidence_proof_rule(GovernanceTier::Governed, TrustClass::Proof);
        assert!(result.passed);
    }

    #[test]
    fn test_evidence_proof_rule_operational_proof_fails() {
        let result = check_evidence_proof_rule(GovernanceTier::Operational, TrustClass::Proof);
        assert!(!result.passed);
        assert!(result.reason.unwrap().contains("Governed tier"));
    }

    #[test]
    fn test_evidence_proof_rule_operational_convenience_passes() {
        let result =
            check_evidence_proof_rule(GovernanceTier::Operational, TrustClass::Convenience);
        assert!(result.passed);
    }

    // ── Extended gate framework tests ─────────────────────────

    #[test]
    fn test_gate_mode_enforce_blocks_on_errors() {
        let result = ExtendedPublishGateResult {
            failures: vec![GateFailure::error("test_gate", "test", "something broke")],
            mode: GateMode::Enforce,
        };
        assert!(result.should_block());
        assert!(result.has_errors());
    }

    #[test]
    fn test_gate_mode_report_only_does_not_block() {
        let result = ExtendedPublishGateResult {
            failures: vec![GateFailure::error("test_gate", "test", "something broke")],
            mode: GateMode::ReportOnly,
        };
        assert!(!result.should_block());
        assert!(result.has_errors());
    }

    #[test]
    fn test_gate_warnings_do_not_block() {
        let result = ExtendedPublishGateResult {
            failures: vec![GateFailure::warning("test_gate", "test", "minor issue")],
            mode: GateMode::Enforce,
        };
        assert!(!result.should_block());
        assert!(result.has_warnings());
        assert!(!result.has_errors());
    }

    #[test]
    fn test_gate_failure_builder() {
        let id = Uuid::new_v4();
        let failure = GateFailure::error("test", "attr", "bad")
            .with_fqn("risk.score")
            .with_snapshot_id(id)
            .with_hint("fix it");
        assert_eq!(failure.object_fqn.as_deref(), Some("risk.score"));
        assert_eq!(failure.snapshot_id, Some(id));
        assert_eq!(failure.remediation_hint.as_deref(), Some("fix it"));
    }

    #[test]
    fn test_failure_report_format() {
        let result = ExtendedPublishGateResult {
            failures: vec![
                GateFailure::error("gate_a", "attr", "broken").with_fqn("x.y"),
                GateFailure::warning("gate_b", "verb", "questionable").with_fqn("a.b"),
            ],
            mode: GateMode::Enforce,
        };
        let report = result.failure_report();
        assert!(report.contains("[ERROR] gate_a (x.y)"));
        assert!(report.contains("[WARN] gate_b (a.b)"));
    }

    #[test]
    fn test_empty_failures_report() {
        let result = ExtendedPublishGateResult {
            failures: vec![],
            mode: GateMode::Enforce,
        };
        assert_eq!(result.failure_report(), "All gates passed.");
        assert!(!result.should_block());
    }

    // ── Derivation gate tests ─────────────────────────────────

    use sem_os_ontology::derivation_spec::*;

    fn make_spec(fqn: &str, output: &str, inputs: &[&str]) -> DerivationSpecBody {
        DerivationSpecBody {
            fqn: fqn.into(),
            name: fqn.into(),
            description: "test".into(),
            output_attribute_fqn: output.into(),
            inputs: inputs
                .iter()
                .map(|&i| DerivationInput {
                    attribute_fqn: i.into(),
                    role: "input".into(),
                    required: true,
                })
                .collect(),
            expression: DerivationExpression::FunctionRef {
                ref_name: "test".into(),
            },
            null_semantics: NullSemantics::Propagate,
            freshness_rule: None,
            security_inheritance: SecurityInheritanceMode::Strict,
            evidence_grade: EvidenceGrade::Prohibited,
            tests: vec![],
        }
    }

    #[test]
    fn test_derivation_no_cycle() {
        let specs = vec![
            make_spec("d1", "out.a", &["in.x", "in.y"]),
            make_spec("d2", "out.b", &["out.a", "in.z"]),
        ];
        let failures = check_derivation_cycle(&specs);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_derivation_cycle_detected() {
        // A → B → A (cycle)
        let specs = vec![
            make_spec("d1", "out.a", &["out.b"]),
            make_spec("d2", "out.b", &["out.a"]),
        ];
        let failures = check_derivation_cycle(&specs);
        assert!(!failures.is_empty());
        assert!(failures[0].message.contains("Cycle"));
    }

    #[test]
    fn test_derivation_evidence_grade_operational_prohibited_passes() {
        let spec = make_spec("d1", "out.a", &["in.x"]);
        let failures = check_derivation_evidence_grade(&spec, GovernanceTier::Operational);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_derivation_evidence_grade_operational_allowed_fails() {
        let mut spec = make_spec("d1", "out.a", &["in.x"]);
        spec.evidence_grade = EvidenceGrade::AllowedWithConstraints;
        let failures = check_derivation_evidence_grade(&spec, GovernanceTier::Operational);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("allowed_with_constraints"));
    }

    #[test]
    fn test_derivation_evidence_grade_governed_allowed_passes() {
        let mut spec = make_spec("d1", "out.a", &["in.x"]);
        spec.evidence_grade = EvidenceGrade::AllowedWithConstraints;
        let failures = check_derivation_evidence_grade(&spec, GovernanceTier::Governed);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_derivation_type_compat_all_known() {
        let spec = make_spec("d1", "out.a", &["in.x", "in.y"]);
        let known: std::collections::HashSet<String> = ["out.a", "in.x", "in.y"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let failures = check_derivation_type_compatibility(&spec, &known);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_derivation_type_compat_unknown_output() {
        let spec = make_spec("d1", "out.unknown", &["in.x"]);
        let known: std::collections::HashSet<String> =
            ["in.x"].iter().map(|s| s.to_string()).collect();
        let failures = check_derivation_type_compatibility(&spec, &known);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("output attribute"));
    }

    #[test]
    fn test_derivation_type_compat_unknown_input() {
        let spec = make_spec("d1", "out.a", &["in.x", "in.missing"]);
        let known: std::collections::HashSet<String> =
            ["out.a", "in.x"].iter().map(|s| s.to_string()).collect();
        let failures = check_derivation_type_compatibility(&spec, &known);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].message.contains("input attribute"));
    }

    // ── Unified gate evaluator tests ──────────────────────────

    fn mock_snapshot_row(tier: GovernanceTier, created_by: &str) -> SnapshotRow {
        SnapshotRow {
            snapshot_id: Uuid::new_v4(),
            snapshot_set_id: None,
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: tier,
            trust_class: TrustClass::Convenience,
            security_label: serde_json::json!({"classification": "internal"}),
            effective_from: chrono::Utc::now(),
            effective_until: None,
            predecessor_id: None,
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: created_by.into(),
            approved_by: None,
            definition: serde_json::json!({"fqn": "cbu.test_attr"}),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_evaluate_extended_gates_operational_minimal() {
        let snapshot = mock_snapshot_row(GovernanceTier::Operational, "scanner");
        let ctx = ExtendedGateContext::default();
        let failures = evaluate_extended_gates(&snapshot, &ctx);
        let errors: Vec<_> = failures
            .iter()
            .filter(|f| f.severity == GateSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "Unexpected errors: {:?}", errors,);
    }

    #[test]
    fn test_evaluate_extended_gates_governed_missing_steward() {
        let snapshot = mock_snapshot_row(GovernanceTier::Governed, "system");
        let ctx = ExtendedGateContext::default();
        let failures = evaluate_extended_gates(&snapshot, &ctx);
        let stewardship_errors: Vec<_> = failures
            .iter()
            .filter(|f| f.gate_name == "stewardship")
            .collect();
        assert_eq!(stewardship_errors.len(), 1);
    }

    #[test]
    fn test_evaluate_extended_gates_breaking_no_rationale() {
        let mut snapshot = mock_snapshot_row(GovernanceTier::Operational, "scanner");
        snapshot.change_type = ChangeType::Breaking;
        let ctx = ExtendedGateContext::default();
        let failures = evaluate_extended_gates(&snapshot, &ctx);
        let continuation_errors: Vec<_> = failures
            .iter()
            .filter(|f| f.gate_name == "continuation_completeness")
            .collect();
        assert_eq!(continuation_errors.len(), 1);
    }

    #[test]
    fn test_evaluate_all_publish_gates_blocks_on_simple_failure() {
        let meta = SnapshotMeta {
            object_type: ObjectType::AttributeDef,
            object_id: Uuid::new_v4(),
            version_major: 1,
            version_minor: 0,
            status: SnapshotStatus::Active,
            governance_tier: GovernanceTier::Operational,
            trust_class: TrustClass::Proof,
            security_label: SecurityLabel::default(),
            change_type: ChangeType::Created,
            change_rationale: None,
            created_by: "test".into(),
            approved_by: None,
            predecessor_id: None,
        };
        let snapshot = mock_snapshot_row(GovernanceTier::Operational, "test");
        let ctx = ExtendedGateContext::default();
        let result = evaluate_all_publish_gates(&meta, &snapshot, &ctx, GateMode::Enforce);
        assert!(result.should_block());
        assert!(result.error_count() >= 1);
    }

    #[test]
    fn test_evaluate_all_publish_gates_blocks_on_extended_error() {
        let meta =
            SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "scanner");
        let mut snapshot = mock_snapshot_row(GovernanceTier::Operational, "scanner");
        snapshot.change_type = ChangeType::Breaking;
        let ctx = ExtendedGateContext::default();
        let result = evaluate_all_publish_gates(&meta, &snapshot, &ctx, GateMode::Enforce);
        assert!(result.should_block());
        let continuation_msgs: Vec<_> = result
            .all_failure_messages()
            .into_iter()
            .filter(|m| m.contains("continuation_completeness"))
            .collect();
        assert!(!continuation_msgs.is_empty());
    }

    #[test]
    fn test_evaluate_all_publish_gates_report_only_does_not_block() {
        let meta =
            SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "scanner");
        let mut snapshot = mock_snapshot_row(GovernanceTier::Operational, "scanner");
        snapshot.change_type = ChangeType::Breaking;
        let ctx = ExtendedGateContext::default();
        let result = evaluate_all_publish_gates(&meta, &snapshot, &ctx, GateMode::ReportOnly);
        assert!(!result.should_block());
        assert!(result.error_count() >= 1);
    }

    #[test]
    fn test_unified_result_warning_count() {
        let meta =
            SnapshotMeta::new_operational(ObjectType::AttributeDef, Uuid::new_v4(), "scanner");
        let snapshot = mock_snapshot_row(GovernanceTier::Operational, "scanner");
        let ctx = ExtendedGateContext::default();
        let result = evaluate_all_publish_gates(&meta, &snapshot, &ctx, GateMode::Enforce);
        assert!(result.warning_count() >= 1);
    }
}
